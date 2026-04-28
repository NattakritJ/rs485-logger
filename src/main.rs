mod config;
mod influx;
mod poller;
mod scheduler;
mod shm;
mod types;

use chrono::Utc;
use config::load_config;
use influx::InfluxWriter;
use poller::ModbusPoller;
use scheduler::next_reset_instant;
use shm::{ShmWriter, ShmReading};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Resolves when SIGTERM or SIGINT (Ctrl+C) is received.
/// Pinned outside the poll loop so the signal subscription persists
/// across iterations (OPS-01).
async fn shutdown_signal() {
    let ctrl_c = async {
        match tokio::signal::ctrl_c().await {
            Ok(()) => (),
            Err(e) => {
                tracing::warn!("Failed to wait for Ctrl+C: {e}");
            }
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                let _ = sig.recv().await;
            }
            Err(e) => {
                tracing::warn!("Failed to install SIGTERM handler: {e}");
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("SIGINT received, initiating graceful shutdown");
        },
        _ = terminate => {
            tracing::info!("SIGTERM received, initiating graceful shutdown");
        },
    }
}

/// Returns a `tokio::time::Instant` far in the future (100 years).
/// Used to park the reset arm when energy reset is disabled.
fn far_future() -> tokio::time::Instant {
    tokio::time::Instant::now() + std::time::Duration::from_secs(365 * 24 * 3600 * 10)
}

#[tokio::main(flavor = "multi_thread")]
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

    // D-05: Resolve effective poll interval from config fields.
    // poll_interval_ms takes precedence; fall back to poll_interval_secs * 1000; default 10,000ms.
    let poll_interval_ms: u64 = cfg.poll_interval_ms
        .unwrap_or_else(|| cfg.poll_interval_secs.unwrap_or(10) * 1000);

    // D-04/D-05: Compute tick cadence — minimum of global interval and all device overrides.
    // If no device has a per-device override, tick_interval_ms == poll_interval_ms (no regression).
    let tick_interval_ms: u64 = cfg.devices.iter()
        .filter_map(|d| d.poll_interval_ms)
        .chain(std::iter::once(poll_interval_ms))
        .min()
        .unwrap(); // safe: chain guarantees at least one element

    // D-04: Track last poll time per device. Initialize so all devices are due immediately.
    let mut last_polled_at: Vec<std::time::Instant> = cfg.devices.iter().map(|d| {
        let effective = d.poll_interval_ms.unwrap_or(poll_interval_ms);
        std::time::Instant::now() - std::time::Duration::from_millis(effective)
    }).collect();

    // D-08: Warn at startup if the interval is very short.
    if poll_interval_ms < 500 {
        tracing::warn!(
            poll_interval_ms = poll_interval_ms,
            "Poll interval is very short ({}ms) — a 5-device cycle takes ~450ms, polling may not complete within the interval",
            poll_interval_ms
        );
    }

    tracing::info!(
        devices = cfg.devices.len(),
        interval_ms = poll_interval_ms,
        "rs485-logger starting"
    );

    // Log per-device interval overrides if any are configured.
    let override_count = cfg.devices.iter().filter(|d| d.poll_interval_ms.is_some()).count();
    if override_count > 0 {
        tracing::info!(
            tick_interval_ms = tick_interval_ms,
            overrides = override_count,
            "Per-device poll intervals active"
        );
        for device in &cfg.devices {
            let effective = device.poll_interval_ms.unwrap_or(poll_interval_ms);
            tracing::info!(device = %device.name, interval_ms = effective, "Device poll interval");
        }
    }

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
            poller.bus_delay_reset().await;
        }
        tracing::info!("--clear mode: done");
        return Ok(());
    }

    let mut poller = ModbusPoller::new(&cfg.serial)?;
    let writer = InfluxWriter::new(&cfg.influxdb)?;

    // Phase 05: Initialize shared memory writer if configured
    let shm_enabled = cfg.shm.as_ref().map(|s| s.enabled).unwrap_or(false);
    let mut shm_writer: Option<ShmWriter> = if shm_enabled {
        let shm_cfg = cfg.shm.as_ref().unwrap(); // safe: shm_enabled implies Some
        match ShmWriter::new(&shm_cfg.path) {
            Ok(w) => {
                tracing::info!(path = %shm_cfg.path, "Shared memory writer enabled");
                Some(w)
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to initialize shared memory writer — continuing without shm");
                None
            }
        }
    } else {
        if cfg.shm.is_some() {
            tracing::info!("Shared memory configured but disabled (enabled = false)");
        }
        None
    };

    // CRIT-02: Track consecutive poll cycles where ALL devices fail.
    // If the counter reaches MAX_CONSECUTIVE_ALL_FAIL, exit with code 1
    // so systemd `Restart=always` restarts the process and re-opens the serial port.
    let mut consecutive_all_fail: u32 = 0;
    const MAX_CONSECUTIVE_ALL_FAIL: u32 = 10;

    // MED-04: Track InfluxDB health to suppress repeated write-failure log spam.
    // On transition healthy → unhealthy: log WARN.
    // On transition unhealthy → healthy: log INFO.
    // While unhealthy: silently drop per-write errors.
    // Arc<AtomicBool> so spawned write tasks can share and update the flag.
    let influx_healthy = Arc::new(AtomicBool::new(true));

    let mut ticker = tokio::time::interval(
        std::time::Duration::from_millis(tick_interval_ms),
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
        // are ready simultaneously.  Priority: shutdown > reset > tick.
        //
        // Shutdown is checked FIRST so that:
        //   (a) the signal waker is registered on the very first iteration, and
        //   (b) SIGTERM is honoured immediately even when the ticker is also ready.
        // Since shutdown resolves at most once (it is otherwise perpetually
        // pending), placing it first adds zero overhead to the normal poll path.
        //
        // Reset is checked before tick to prevent the ticker from polling devices
        // on the RS485 bus at the same instant the energy reset fires (both
        // deadlines fire at midnight).  Without `biased` tokio::select! picks
        // randomly — if the ticker wins it sends FC0x04 read commands to all
        // devices; by the time the reset arm runs, the bus is still settling and
        // device 2's reset command gets no response within the 500 ms window.
        tokio::select! {
            biased;

            // Shutdown arm — checked FIRST so SIGTERM/SIGINT is never starved by
            // the ticker.  The future is pending until a signal arrives, so this
            // arm has no impact on normal poll throughput.
            _ = &mut shutdown => {
                tracing::info!("Shutdown signal received, exiting cleanly");
                break;
            }

            // Daily energy reset arm (D-09) — fires at next midnight local time.
            // Checked second (before ticker) so it wins when both are ready.
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
                    poller.bus_delay_reset().await;
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
                // D-09: Track cycle duration to warn when poll exceeds tick interval.
                let cycle_start = std::time::Instant::now();
                let now = std::time::Instant::now();

                // CRIT-02: Track whether at least one device succeeded this cycle.
                let mut any_ok = false;
                let mut any_polled = false;
                let mut shm_readings: Vec<(&str, f64)> = Vec::new();

                for (i, device) in cfg.devices.iter().enumerate() {
                    let effective_interval = std::time::Duration::from_millis(
                        device.poll_interval_ms.unwrap_or(poll_interval_ms)
                    );
                    if now.duration_since(last_polled_at[i]) < effective_interval {
                        continue; // not due yet
                    }
                    any_polled = true;
                    last_polled_at[i] = now;

                    match poller.poll_device(device).await {
                        Ok(reading) => {
                            any_ok = true;
                            // Collect for shm write if device has shm_name
                            if let Some(ref shm_name) = device.shm_name {
                                shm_readings.push((shm_name.as_str(), reading.power));
                            }
                            tracing::info!(
                                device = %device.name,
                                "Poll success"
                            );
                            // MED-04: InfluxDB health-state tracking — fire-and-forget write.
                            {
                                let writer_clone = writer.clone();
                                let health = Arc::clone(&influx_healthy);
                                let device_name = device.name.clone();
                                tokio::spawn(async move {
                                    match writer_clone.write(&reading).await {
                                        Ok(()) => {
                                            if !health.load(Ordering::Relaxed) {
                                                health.store(true, Ordering::Relaxed);
                                                tracing::info!("InfluxDB connection restored");
                                            }
                                        }
                                        Err(e) => {
                                            if health.swap(false, Ordering::Relaxed) {
                                                tracing::warn!(
                                                    device = %device_name,
                                                    error = %e,
                                                    "InfluxDB write failed — suppressing further warnings until restored"
                                                );
                                            }
                                        }
                                    }
                                });
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                device = %device.name,
                                error = %e,
                                "Device poll failed, skipping"
                            );
                            // HIGH-04: Drain stale Modbus frames after error.
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        }
                    }
                    // RS-485 inter-frame delay between consecutive device polls.
                    poller.bus_delay_read().await;
                }

                // Phase 05: Write to shared memory after all devices polled (D-07)
                if let Some(ref mut shm_w) = shm_writer {
                    if !shm_readings.is_empty() {
                        let entries: Vec<ShmReading<'_>> = shm_readings.iter()
                            .map(|(name, power)| ShmReading { shm_name: name, power_watts: *power })
                            .collect();
                        if let Err(e) = shm_w.write(&entries) {
                            tracing::warn!(error = %e, "Shared memory write failed");
                        }
                    }
                }

                // CRIT-02: Only count failures when at least one device was actually polled.
                // Ticks where no device was due should not affect the counter.
                if !any_polled {
                    // No devices were due this tick — don't change the counter
                } else if any_ok {
                    consecutive_all_fail = 0;
                } else {
                    consecutive_all_fail += 1;
                    tracing::warn!(
                        consecutive_failures = consecutive_all_fail,
                        "All devices failed this poll cycle"
                    );
                    if consecutive_all_fail >= MAX_CONSECUTIVE_ALL_FAIL {
                        tracing::error!(
                            consecutive_failures = consecutive_all_fail,
                            "All devices failed {} consecutive polls — exiting for systemd restart",
                            MAX_CONSECUTIVE_ALL_FAIL
                        );
                        break;
                    }
                }

                // D-09: Warn if the poll cycle took longer than the tick interval.
                let cycle_ms = cycle_start.elapsed().as_millis();
                if cycle_ms > tick_interval_ms as u128 {
                    tracing::warn!(
                        cycle_ms = cycle_ms as u64,
                        interval_ms = tick_interval_ms,
                        "Poll cycle took {}ms, exceeds tick interval of {}ms — consider reducing read_timeout_ms or device count",
                        cycle_ms,
                        tick_interval_ms
                    );
                }
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
