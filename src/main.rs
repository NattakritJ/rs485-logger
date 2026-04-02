mod config;
mod influx;
mod poller;
mod scheduler;
mod types;

use chrono::Utc;
use config::load_config;
use influx::InfluxWriter;
use poller::ModbusPoller;
use scheduler::next_reset_instant;

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

/// Returns a `tokio::time::Instant` far in the future (100 years).
/// Used to park the reset arm when energy reset is disabled.
fn far_future() -> tokio::time::Instant {
    tokio::time::Instant::now() + std::time::Duration::from_secs(365 * 24 * 3600 * 100)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    // Parse --config <path> and --clear from CLI args (used by systemd ExecStart).
    // Falls back to "config.toml" in the current directory for local testing.
    // --clear: send energy reset command to all devices and exit immediately.
    let (config_path, clear_mode) = {
        let mut args = std::env::args().skip(1);
        let mut path = "config.toml".to_string();
        let mut clear = false;
        while let Some(arg) = args.next() {
            if arg == "--config" {
                if let Some(p) = args.next() {
                    path = p;
                } else {
                    eprintln!("Fatal: --config requires a path argument");
                    std::process::exit(1);
                }
            } else if arg == "--clear" {
                clear = true;
            }
        }
        (path, clear)
    };

    // Load config first — use eprintln! for pre-logging errors (OPS-02/OPS-03
    // require the config to know whether to activate file logging).
    let cfg = load_config(&config_path).unwrap_or_else(|e| {
        eprintln!("Fatal: failed to load config: {e}");
        std::process::exit(1);
    });

    // OPS-02: structured logging to stderr (journald compatible)
    // OPS-03: optional file appender from config
    // Determine log level: cfg.log_level > RUST_LOG env var > "info" fallback
    let env_filter = if let Some(ref level) = cfg.log_level {
        // Config takes highest priority: use it directly, fall back to "info" if invalid
        tracing_subscriber::EnvFilter::try_new(level)
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
    } else {
        // No config value — honour RUST_LOG, then fall back to "info"
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
    };

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
        let file_appender = tracing_appender::rolling::daily(dir, filename);
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

    // --clear mode: send energy reset to every device, then exit.
    // Opens the Modbus port, iterates all configured devices sequentially,
    // logs per-device success/failure, and returns before the poll loop.
    if clear_mode {
        tracing::info!("--clear mode: sending energy reset to all devices");
        let mut poller = ModbusPoller::new(&cfg.serial)?;
        for device in &cfg.devices {
            tracing::info!(device = %device.name, "Energy reset sending command");
            match poller.reset_energy(device).await {
                Ok(()) => tracing::info!(device = %device.name, "Energy reset OK"),
                Err(e) => tracing::warn!(device = %device.name, error = %e,
                                          "Energy reset failed, skipping"),
            }
            // RS-485 inter-frame delay: give the bus time to settle before
            // addressing the next device (prevents second-device timeout).
            poller.bus_delay().await;
        }
        tracing::info!("--clear mode: done");
        return Ok(());
    }

    let mut poller = ModbusPoller::new(&cfg.serial)?;
    let writer = InfluxWriter::new(&cfg.influxdb)?;

    let mut ticker = tokio::time::interval(
        std::time::Duration::from_secs(cfg.poll_interval_secs),
    );
    // Skip missed ticks: if the energy reset loop (or any other blocking arm)
    // causes a tick to be missed, do NOT burst-fire to catch up — simply skip
    // the missed intervals.  This prevents a flood of back-to-back polls
    // immediately after a reset that might interfere with the RS485 bus.
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // Pin the shutdown future outside the loop so the signal subscription
    // persists across iterations (calling shutdown_signal() fresh each time
    // would register a new signal handler on every tick).
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);

    // --- Energy reset scheduling (Phase 06) ---
    // Determine if energy reset is enabled from config.
    let reset_enabled = cfg
        .energy_reset
        .as_ref()
        .map(|r| r.enabled)
        .unwrap_or(false);

    if !reset_enabled && cfg.energy_reset.is_some() {
        tracing::info!("Energy reset configured but disabled (enabled = false)");
    }

    // `reset_sleep` is always pinned in the select! loop.
    // When disabled, it points to far_future() and never fires.
    // When enabled, it is reset to the actual next midnight after each fire (D-08/D-09).
    let initial_reset_deadline = if reset_enabled {
        let er = cfg.energy_reset.as_ref().unwrap(); // safe: reset_enabled implies Some
        match next_reset_instant(Utc::now(), &er.timezone, &er.time) {
            Ok(std_instant) => {
                log_next_reset(std_instant, &er.timezone); // D-13
                tokio::time::Instant::from_std(std_instant)
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to compute next reset time — energy reset disabled");
                far_future()
            }
        }
    } else {
        far_future()
    };

    let reset_sleep = tokio::time::sleep_until(initial_reset_deadline);
    tokio::pin!(reset_sleep);

    loop {
        // `biased` ensures arms are checked in declaration order when multiple
        // are ready simultaneously.  Priority: reset > tick > shutdown.
        //
        // This prevents the ticker from polling devices on the RS485 bus at the
        // same instant the energy reset fires (both deadlines fire at midnight).
        // Without `biased`, tokio::select! picks randomly — if the ticker wins it
        // sends FC0x04 read commands to all devices; by the time the reset arm
        // runs, the bus is still settling and device 2's reset command gets no
        // response within the 500 ms window (timeout).
        tokio::select! {
            biased;

            // Daily energy reset arm (D-09) — fires at next midnight local time.
            // Checked first (biased) so it wins over the ticker when both are ready.
            // When disabled, reset_sleep points to far_future() and never resolves.
            _ = &mut reset_sleep, if reset_enabled => {
                let er = cfg.energy_reset.as_ref().unwrap(); // safe: only fires when reset_enabled
                tracing::info!("Daily energy reset starting"); // D-11

                // Strictly sequential per-device: send → wait for response → delay → next device.
                for device in &cfg.devices {
                    tracing::info!(device = %device.name, "Energy reset sending command"); // D-11
                    match poller.reset_energy(device).await {
                        Ok(()) => {
                            tracing::info!(device = %device.name, "Energy reset OK"); // D-11
                        }
                        Err(e) => {
                            tracing::warn!(
                                device = %device.name,
                                error = %e,
                                "Energy reset failed, skipping" // D-12
                            );
                        }
                    }
                    // RS-485 inter-frame delay: give the bus time to settle before
                    // addressing the next device (prevents second-device timeout).
                    poller.bus_delay().await;
                }
                // Recompute next reset (D-08 — recompute from now, don't drift by adding 86400s)
                let next_deadline = match next_reset_instant(Utc::now(), &er.timezone, &er.time) {
                    Ok(std_instant) => {
                        log_next_reset(std_instant, &er.timezone); // D-13
                        tokio::time::Instant::from_std(std_instant)
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "Failed to recompute next reset — parking arm far in the future"
                        );
                        far_future()
                    }
                };
                reset_sleep.as_mut().reset(next_deadline);
            }
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
                    // RS-485 inter-frame delay: ensure the bus settles between
                    // consecutive device polls.  Previously the InfluxDB HTTP
                    // write provided an accidental delay on the success path,
                    // but the error path had none — make it explicit everywhere.
                    poller.bus_delay().await;
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

/// Log the next scheduled reset time in local timezone (D-13).
fn log_next_reset(std_instant: std::time::Instant, tz_str: &str) {
    let now_std = std::time::Instant::now();
    let delay = std_instant.checked_duration_since(now_std).unwrap_or_default();
    let next_utc = Utc::now() + chrono::Duration::from_std(delay).unwrap_or_default();
    let tz: chrono_tz::Tz = tz_str.parse().unwrap_or(chrono_tz::Asia::Bangkok);
    let next_local = next_utc.with_timezone(&tz);
    tracing::info!(
        next_reset = %next_local.format("%Y-%m-%dT%H:%M:%S%:z"),
        "Next energy reset scheduled"
    );
}
