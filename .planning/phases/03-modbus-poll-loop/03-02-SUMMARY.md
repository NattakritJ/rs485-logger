---
phase: 03-modbus-poll-loop
plan: "02"
subsystem: main-poll-loop
tags: [poll-loop, main, tokio-interval, skip-and-continue, dead_code-cleanup]
dependency_graph:
  requires: [03-01]
  provides: [full-daemon-loop, POLL-02, POLL-03]
  affects: [src/main.rs, src/config.rs, src/influx.rs, src/types.rs]
tech_stack:
  added: []
  patterns: [tokio::time::interval, skip-and-warn error handling, structured tracing::warn!]
key_files:
  created: []
  modified:
    - src/main.rs
    - src/config.rs
    - src/influx.rs
    - src/types.rs
decisions:
  - "tokio::time::interval ticks immediately on first call — daemon polls at t=0, not t+interval (correct for startup)"
  - "InfluxDB write errors logged as WARN (not ERROR) — recoverable, next tick retries automatically"
  - "Device poll errors: WARN and continue (skip-and-continue = POLL-03 resilience property)"
metrics:
  duration_secs: 124
  completed: "2026-04-02"
  tasks: 2
  files_modified: 4
---

# Phase 03 Plan 02: Poll Loop Wiring Summary

**One-liner:** Sequential tokio::time::interval poll loop with skip-and-warn per-device error handling wired into main().

## What Was Built

Wired `src/main.rs` into the full daemon poll loop: config load → `ModbusPoller::new` → `InfluxWriter::new` → `tokio::time::interval` tick loop → sequential per-device poll → write to InfluxDB or log WARN and continue.

Cleaned all `#![allow(dead_code)]` and `#[allow(dead_code)]` attributes from the four module files now that `main()` actively imports and uses all of them.

## Tasks Completed

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | Wire the poll loop in main() | `203e959` | src/main.rs |
| 2 | Remove dead_code allows now that all modules are live | `cb3074b` | src/config.rs, src/influx.rs, src/types.rs |

## Verification

```
cargo build  → Finished (zero errors, zero warnings)
cargo test   → 17 passed; 0 failed; 3 ignored (hardware/influx-gated)
```

## Key Implementation Details

**Poll loop pattern (src/main.rs):**
- `tokio::time::interval` ticks immediately at t=0 — daemon polls on startup without waiting one full interval
- `for device in &cfg.devices` — sequential iteration, single RS485 bus cannot be concurrent
- `match poller.poll_device(device).await` — `Ok(reading)` → write to InfluxDB; `Err(e)` → `tracing::warn!` and continue
- Both error arms use structured fields: `device = %device.name, error = %e` (satisfies OPS-02)
- InfluxDB write failures are `WARN` (not `ERROR`) — recoverable transient failures, auto-retried next tick

**Why `#![allow(dead_code)]` is now gone:**
- `config.rs`, `types.rs`: module-level allows removed — all structs/functions are reachable via `main.rs` imports
- `influx.rs`: item-level allows on `to_line_protocol`, `InfluxWriter`, and `impl` block removed — `InfluxWriter::new` and `write` are called from `main()`
- `poller.rs`: had no dead_code attributes (was already clean from 03-01)

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| First tick at t=0 (interval default) | Correct daemon behavior: poll on startup, no silent gap |
| InfluxDB errors → WARN not ERROR | Write failures are transient/recoverable; ERROR implies fatal/action required |
| Device errors → WARN + continue | Core resilience requirement POLL-03: one offline device must not stop others |

## Deviations from Plan

None — plan executed exactly as written.

## Known Stubs

None — all data flows from hardware (ModbusPoller) through to InfluxDB (InfluxWriter). No placeholder data.

## Self-Check: PASSED
