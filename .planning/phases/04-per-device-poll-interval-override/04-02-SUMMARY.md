---
phase: 04-per-device-poll-interval-override
plan: 02
status: complete
started: 2026-04-28T14:13:00Z
completed: 2026-04-28T14:16:00Z
---

# Plan 04-02 Summary: Tick-at-minimum scheduler with per-device due check

## What Was Built

Transformed the poll loop from "poll all → sleep global_interval" to "tick at minimum interval → poll only due devices". The scheduler computes `tick_interval_ms` as the minimum of the global interval and all per-device overrides, tracks `last_polled_at` per device, and only polls devices whose effective interval has elapsed since their last poll.

## Tasks Completed

| # | Task | Status |
|---|------|--------|
| 1 | Compute tick_interval_ms + init last_polled_at + startup logging | ✓ |
| 2 | Transform poll loop to per-device due check + adapt CRIT-02 and cycle warning | ✓ |

## Key Files

### Created
- (none)

### Modified
- `src/main.rs` — tick_interval_ms computation, last_polled_at tracking, per-device due check, adapted CRIT-02 and cycle warning

## Self-Check: PASSED

- `cargo build` — compiles cleanly
- `cargo test` — 53 passed, 0 failed, 4 ignored
- `tick_interval_ms` computed from `min(global, all device overrides)`
- `last_polled_at` initialized so all devices are due immediately
- Ticker uses `tick_interval_ms` (not `poll_interval_ms`)
- `any_polled` flag guards CRIT-02 counter against empty ticks
- Cycle warning references `tick_interval_ms`
- Startup log shows per-device intervals when overrides are configured
- When no overrides exist, `tick_interval_ms == poll_interval_ms` (Phase 3 regression-free)

## Deviations

None — executed as planned.
