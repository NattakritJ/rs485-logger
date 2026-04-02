---
phase: 03-modbus-poll-loop
plan: "03"
subsystem: daemon-lifecycle
tags: [signal-handling, logging, tracing, graceful-shutdown, ops]
dependency_graph:
  requires: [03-02]
  provides: [signal-handling, structured-logging, file-appender]
  affects: [src/main.rs, src/config.rs]
tech_stack:
  added: []
  patterns:
    - tokio::select! with tokio::pin! for graceful poll-loop shutdown
    - tracing_subscriber::registry() for dual stderr+file layer composition
    - _file_guard pattern to extend non_blocking appender lifetime to end of main()
    - Config-first init: load config before logging to enable file appender setup
key_files:
  created: []
  modified:
    - src/main.rs
    - src/config.rs
decisions:
  - Config loaded before tracing init (using eprintln! for errors) — enables file appender from config without double-init complexity
  - RUST_LOG env var takes priority over log_level config field — EnvFilter::try_from_default_env() first, then try_new(log_level)
  - SubscriberExt + SubscriberInitExt traits imported locally inside the if-branch — keeps outer scope clean
  - shutdown_signal() pinned outside poll loop — signal subscriptions persist across iterations (one SIGTERM handler, not per-tick)
metrics:
  duration: "~6 min"
  completed: "2026-04-02"
  tasks: 2
  files_modified: 2
---

# Phase 03 Plan 03: Signal Handling & Structured Logging Summary

**One-liner:** Graceful SIGTERM/SIGINT shutdown via `tokio::select!` + dual stderr/file structured logging via `tracing_subscriber` with EnvFilter and `_file_guard` lifetime fix.

## What Was Built

### Task 1: Optional log_file / log_level fields in AppConfig

Added two `Option<T>` fields to `AppConfig` in `src/config.rs`:
- `log_file: Option<String>` — absolute path for file log output (OPS-03)
- `log_level: Option<String>` — runtime log level override (default "info")

Serde's `Option<T>` deserialization means fields absent from TOML become `None` — no TOML changes needed for existing deployments. Updated `test_empty_device_list` which constructs `AppConfig` directly with struct literal syntax to include `log_file: None` and `log_level: None`.

### Task 2: tracing init + graceful shutdown in main()

**`shutdown_signal()` async fn:**
- Handles SIGINT (`ctrl_c`) and SIGTERM (`signal::unix::signal(SignalKind::terminate())`)
- `#[cfg(unix)]` / `#[cfg(not(unix))]` conditional compilation for cross-platform correctness
- Combined via `tokio::select!` — first signal to fire resolves the future

**Logging initialization:**
- Config loaded first (before tracing) using `eprintln!` for fatal errors — enables file appender from config values
- `EnvFilter`: `RUST_LOG` env var takes priority → `log_level` config field → fallback "info"
- File appender branch: `tracing_appender::rolling::never` + `non_blocking` writer; `SubscriberExt`/`SubscriberInitExt` imported locally to enable `registry().with()` chain
- Dual output: `fmt::Layer` to file + `fmt::Layer` to stderr (journald compatible)
- `_file_guard` declared before branch and assigned inside — ensures the non-blocking appender worker thread lives until `main()` returns
- Stderr-only branch: simple `tracing_subscriber::fmt().with_env_filter().with_writer(stderr).init()`

**Graceful shutdown poll loop:**
- Startup log: `tracing::info!(devices = N, interval_secs = S, "rs485-logger starting")`
- `shutdown_signal()` pinned outside loop (`tokio::pin!(shutdown)`) — one signal registration persists across ticks
- `tokio::select!` on `ticker.tick()` vs `&mut shutdown` — SIGTERM/SIGINT breaks after completing current cycle (never mid-loop)
- All poll/write logs use structured fields: `device = %device.name`
- Shutdown log: `tracing::info!("rs485-logger stopped")`

## Verification Results

| Check | Result |
|-------|--------|
| `cargo build` | ✅ Finished (zero errors) |
| `cargo test` | ✅ 17 passed, 3 ignored (hardware/InfluxDB) |
| `shutdown_signal()` present | ✅ |
| `tokio::pin!(shutdown)` | ✅ |
| `tokio::select!` on ticker + shutdown | ✅ |
| `_file_guard` lifetime fix | ✅ |
| `tracing_subscriber` with EnvFilter | ✅ |
| `log_file`/`log_level` in AppConfig | ✅ |

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Added `use tracing_subscriber::layer::SubscriberExt` and `use tracing_subscriber::util::SubscriberInitExt`**
- **Found during:** Task 2 — `cargo build` failed with `no method named 'with' found for struct Registry`
- **Issue:** `SubscriberExt` trait (which provides `.with()` on `Registry`) was not in scope; the plan's code snippet used `registry().with()` without importing the trait
- **Fix:** Added local `use tracing_subscriber::layer::SubscriberExt; use tracing_subscriber::util::SubscriberInitExt;` imports inside the `if let Some(ref log_path)` branch — keeps outer scope clean
- **Files modified:** `src/main.rs`
- **Commit:** 9d7531c

## Known Stubs

None — all OPS requirements fully implemented with real functionality.

## Self-Check: PASSED
