---
phase: "07"
plan: "01"
subsystem: influx-client-hardening
tags: [reliability, security, influxdb, logging, git-hygiene]
dependency_graph:
  requires: []
  provides: [influx-timeout, db-name-validation, config-secret-hygiene, log-rotation]
  affects: [src/influx.rs, src/config.rs, src/main.rs]
tech_stack:
  added: []
  patterns: [reqwest-client-builder-timeout, anyhow-ensure-validation, rolling-daily-appender]
key_files:
  created: [config.toml.example]
  modified: [src/influx.rs, src/config.rs, src/main.rs, src/poller.rs, .gitignore]
decisions:
  - "InfluxWriter::new() returns anyhow::Result<Self> — timeout config via Client::builder() requires fallible build()"
  - "Database name validated at config time (not URL-encoded at runtime) — simple identifiers are safer and more predictable"
  - "config.toml removed from git tracking; config.toml.example with placeholder token serves as reference"
  - "rolling::daily chosen over never — date-suffix rotation is standard tracing-appender behavior"
  - "far_future() reduced from 100 years to 10 years — avoids potential Duration overflow on 32-bit platforms"
metrics:
  duration_secs: 236
  completed_date: "2026-04-02"
  tasks_completed: 6
  files_changed: 6
---

# Phase 7 Plan 1: InfluxDB Client Hardening + Git Hygiene + Log Rotation Summary

## One-liner

Fixed 6 reliability and security findings: 5s/10s HTTP timeouts, database name validation, token removed from git, daily log rotation, reduced far_future duration.

## What Was Built

Six targeted hardening fixes addressing critical and high-priority findings from the daemon reliability verification report.

### T1: HTTP Timeouts (CRIT-01 + HIGH-01) — commit `1475543`

Added `connect_timeout(5s)` and `timeout(10s)` to `reqwest::Client::builder()`. The `.timeout()` covers the entire request lifecycle including response body read, fixing both CRIT-01 (no request timeout) and HIGH-01 (unbounded error body read). `InfluxWriter::new()` now returns `anyhow::Result<Self>` to propagate the fallible `build()` call. All test callsites updated with `.expect()`.

### T2: Database Name Validation (HIGH-03) — commit `f6c82e1`

Added `anyhow::ensure!` check in `validate_config()`: database name must consist only of alphanumeric characters, underscores, or dashes. This prevents URL injection in the manually-constructed query string (`?db={}&precision=ns`). Added 4 tests covering slash, space, `?`, and valid names.

### T3: Git Hygiene — API Token (CRIT-03) — commit `dd8b040`

- `config.toml` removed from git tracking (`git rm --cached`) — contained a live API token
- `config.toml` added to `.gitignore` to prevent future accidental commits
- `config.toml.example` created with `YOUR_INFLUXDB_TOKEN_HERE` placeholder and updated documentation comments

### T4: Rolling Log Rotation (MED-01) — commit `5f26a84`

Replaced `tracing_appender::rolling::never(dir, filename)` with `rolling::daily(dir, filename)`. Daily rotation creates files like `rs485.log.2026-04-03`, preventing unbounded log file growth on long-running Pi deployments.

### T5: far_future() Duration (MED-02) — commit `f8c3ede`

Reduced `365 * 24 * 3600 * 100` (100 years) to `365 * 24 * 3600 * 10` (10 years). Avoids potential Duration overflow on 32-bit platforms while still being effectively infinite for parking the disabled reset arm.

### T6: Verification — commit `5219261`

- `cargo test`: 39 tests pass, 4 integration tests properly `#[ignore]`-gated
- `cargo clippy -- -D warnings`: clean, no warnings
- Also committed `bus_delay()` method to `ModbusPoller` (100ms inter-frame delay per Modbus RTU spec) that was wired into `main.rs` from a prior quick task but not yet in git

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Functionality] Committed pre-existing poller bus_delay() function**
- **Found during:** T6 verification
- **Issue:** `bus_delay()` was already wired into `main.rs` from quick task 260403-0gn but `src/poller.rs` had not been committed
- **Fix:** Committed `src/poller.rs` changes as part of T6 verification
- **Files modified:** `src/poller.rs`
- **Commit:** `5219261`

**2. [Rule 2 - Missing Functionality] Pre-existing device name validation already in config.rs**
- **Found during:** T2
- **Issue:** The file on disk already had device name validation (alphanumeric + underscore check) from a prior uncommitted quick task change. The T1 commit swept it in along with the new database validation.
- **Fix:** Incorporated naturally — the device name validation was good code that belonged in the repo
- **Files modified:** `src/config.rs` (swept in with T1 commit `1475543`)

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| `InfluxWriter::new()` returns `Result` | `reqwest::Client::builder().build()` is fallible; propagating the error is correct Rust |
| Validate database name at config time | Simpler than URL-encoding; database names should be plain identifiers |
| `config.toml` untracked, not deleted | Local dev file stays on disk but never re-enters git history |
| `rolling::daily` appender | Standard behavior; date suffix makes log rotation predictable for logrotate too |
| 10-year far_future | Still effectively infinite; avoids edge cases on 32-bit targets |

## Test Coverage

- `test_influx_writer_constructs` — verifies new `Result`-returning constructor succeeds
- `test_influx_writer_trims_trailing_slash` — regression guard on URL construction
- `test_database_name_with_slash_rejected` — HIGH-03: `/` rejected
- `test_database_name_with_space_rejected` — HIGH-03: space rejected
- `test_database_name_with_question_mark_rejected` — HIGH-03: URL injection char rejected
- `test_database_name_alphanumeric_underscore_dash_passes` — HIGH-03: valid names pass

## Self-Check: PASSED

All files verified to exist:
- `config.toml.example` — FOUND
- `src/influx.rs` — FOUND (InfluxWriter::new returns Result, timeouts added)
- `src/config.rs` — FOUND (database name validation added)
- `.gitignore` — FOUND (config.toml added)

All commits verified to exist:
- `1475543` — FOUND (T1: HTTP timeouts)
- `f6c82e1` — FOUND (T2: database validation)
- `dd8b040` — FOUND (T3: git hygiene)
- `5f26a84` — FOUND (T4: rolling log)
- `f8c3ede` — FOUND (T5: far_future)
- `5219261` — FOUND (T6: verification)
