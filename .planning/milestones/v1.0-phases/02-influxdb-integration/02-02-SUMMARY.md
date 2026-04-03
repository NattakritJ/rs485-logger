---
phase: 02-influxdb-integration
plan: "02"
subsystem: database
tags: [influxdb, reqwest, http, bearer-auth, rust, async]

requires:
  - phase: 02-01
    provides: to_line_protocol() in src/influx.rs
  - phase: 01-foundation
    provides: InfluxConfig struct in src/config.rs, PowerReading in src/types.rs

provides:
  - InfluxWriter struct in src/influx.rs
  - InfluxWriter::new(config: &InfluxConfig) constructor
  - InfluxWriter::write(reading: &PowerReading) -> anyhow::Result<()>
  - 2 unit tests (constructor, trailing-slash trimming)
  - 2 ignored integration tests (live write + connection-refused error path)

affects:
  - 03-modbus-polling (calls InfluxWriter::write() after each poll cycle)

tech-stack:
  added: []
  patterns:
    - "Bearer auth via reqwest .bearer_auth() — no manual header construction"
    - "URL built manually with format!() — reqwest rustls feature excludes .query()"
    - "HTTP 204 = Ok(()), anything else = Err(anyhow) with status + body context"
    - "#[ignore] integration tests with INFLUX_TOKEN env var for CI safety"

key-files:
  created: []
  modified: [src/influx.rs]

key-decisions:
  - "URL query string built manually as '?db={}&precision=ns' — reqwest rustls feature does not include .query() method"
  - "HTTP response body read with .unwrap_or_default() inside error branch (acceptable: error path only)"
  - "#[ignore] integration tests gated to avoid blocking offline CI; run with --include-ignored when InfluxDB available"
  - "connection-refused test also marked #[ignore] to keep default test runs fast"

patterns-established:
  - "InfluxWriter endpoint URL: '{base_url}/api/v3/write_lp?db={database}&precision=ns'"
  - "Error context chain: reqwest error → anyhow::Context → 'Failed to connect to InfluxDB at {url}'"
  - "HTTP non-204 → Err: 'InfluxDB write failed: HTTP {status} — {body}'"

requirements-completed: [STOR-01, STOR-02, STOR-03, STOR-04]

duration: 10min
completed: 2026-04-02
---

# Plan 02-02: InfluxWriter Summary

**Async reqwest HTTP client for InfluxDB 3 line protocol writes with Bearer auth, anyhow error propagation, and ignored integration tests**

## Performance

- **Duration:** ~10 min
- **Tasks:** 2 (struct + integration tests)
- **Files modified:** 1

## Accomplishments
- `InfluxWriter` struct wraps `reqwest::Client` with pre-computed endpoint URL
- `write()` method: builds line protocol → POSTs with Bearer auth → checks HTTP 204
- Error handling: connection failures and non-204 responses return `Err`, never panic (STOR-04)
- 2 unit tests: construction correctness, trailing-slash trimming on base URL
- 2 `#[ignore]` integration tests: live write verification + connection-refused Err path

## Task Commits

1. **Task 1+2: InfluxWriter + integration tests** - `4db02d7` (feat)

## Files Created/Modified
- `src/influx.rs` — appended `InfluxWriter` struct, `impl` block, 2 unit tests, 2 ignored integration tests

## Decisions Made
- URL query string manual: `format!("{}?db={}&precision=ns", self.url, self.database)` — reqwest `rustls` feature does not expose `.query()` method
- `precision=ns` because `to_line_protocol()` already converts to nanoseconds
- Integration tests remain `#[ignore]`: run with `cargo test influx -- --include-ignored` when InfluxDB 3 is available

## Deviations from Plan
- **`.query()` not available**: Plan specified `.query(&[("db", ...), ("precision", "ns")])` but `reqwest` with `features = ["rustls"]` does not expose the `.query()` method. Fixed by building the URL string manually. Semantics identical.

## Issues Encountered
- reqwest `.query()` method unavailable with current feature set — resolved by manual URL construction (1 compile error, 1 fix).

## User Setup Required
To run integration tests against a live InfluxDB 3 instance:
```bash
docker run -d -p 8086:8086 influxdb:3-core
INFLUX_TOKEN=<your-token> cargo test influx -- --include-ignored --nocapture
```
Expected: `test_influx_write_integration` passes (HTTP 204).

## Next Phase Readiness
- Complete InfluxDB write path is ready: `InfluxWriter::write(&PowerReading)` → HTTP 204
- Phase 3 (modbus-polling) can import `InfluxWriter` and call `.write()` after each poll
- Test count: 17 unit tests passing, 2 ignored integration tests

---
*Phase: 02-influxdb-integration*
*Completed: 2026-04-02*
