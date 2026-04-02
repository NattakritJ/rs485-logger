mod config;
mod influx;
mod poller;
mod types;

use config::load_config;
use influx::InfluxWriter;
use poller::ModbusPoller;

/// Resolves when SIGTERM or SIGINT (Ctrl+C) is received.
/// Pinned outside the poll loop so the signal subscription persists
/// across iterations (OPS-01).
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    // Load config first — use eprintln! for pre-logging errors (OPS-02/OPS-03
    // require the config to know whether to activate file logging).
    let cfg = load_config("config.toml").unwrap_or_else(|e| {
        eprintln!("Fatal: failed to load config: {e}");
        std::process::exit(1);
    });

    // OPS-02: structured logging to stderr (journald compatible)
    // OPS-03: optional file appender from config
    // Determine log level: cfg.log_level > RUST_LOG env var > "info" fallback
    let log_level = cfg.log_level.as_deref().unwrap_or("info");
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            tracing_subscriber::EnvFilter::try_new(log_level)
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        });

    // _file_guard must outlive the entire main() so file logging continues
    // until the process exits.  Declared before the branch; assigned inside.
    let _file_guard;

    if let Some(ref log_path) = cfg.log_file {
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;
        let path = std::path::Path::new(log_path);
        let dir = path.parent().unwrap_or(std::path::Path::new("."));
        let filename = path
            .file_name()
            .unwrap_or(std::ffi::OsStr::new("rs485-logger.log"));
        let file_appender = tracing_appender::rolling::never(dir, filename);
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        _file_guard = Some(guard);
        tracing_subscriber::registry()
            .with(env_filter)
            .with(
                tracing_subscriber::fmt::Layer::new()
                    .with_writer(non_blocking),
            )
            .with(
                tracing_subscriber::fmt::Layer::new()
                    .with_writer(std::io::stderr),
            )
            .init();
    } else {
        _file_guard = None;
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_writer(std::io::stderr)
            .init();
    }

    tracing::info!(
        devices = cfg.devices.len(),
        interval_secs = cfg.poll_interval_secs,
        "rs485-logger starting"
    );

    let mut poller = ModbusPoller::new(&cfg.serial)?;
    let writer = InfluxWriter::new(&cfg.influxdb);

    let mut ticker = tokio::time::interval(
        std::time::Duration::from_secs(cfg.poll_interval_secs),
    );

    // Pin the shutdown future outside the loop so the signal subscription
    // persists across iterations (calling shutdown_signal() fresh each time
    // would register a new signal handler on every tick).
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                for device in &cfg.devices {
                    match poller.poll_device(device).await {
                        Ok(reading) => {
                            tracing::info!(
                                device = %device.name,
                                "Poll success"
                            );
                            if let Err(e) = writer.write(&reading).await {
                                tracing::warn!(
                                    device = %device.name,
                                    error = %e,
                                    "InfluxDB write failed"
                                );
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                device = %device.name,
                                error = %e,
                                "Device poll failed, skipping"
                            );
                        }
                    }
                }
            }
            _ = &mut shutdown => {
                tracing::info!("Shutdown signal received, exiting cleanly");
                break;
            }
        }
    }

    tracing::info!("rs485-logger stopped");
    Ok(())
}
