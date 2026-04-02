---
phase: "07"
plan: "03"
subsystem: main-poll-loop
tags: [reliability, serial-recovery, modbus, influxdb, backoff]
dependency_graph:
  requires: [07-01]
  provides: [serial-recovery-exit, modbus-drain-delay, influxdb-health-tracking]
  affects: [src/main.rs]
tech_stack:
  added: []
  patterns:
    - consecutive-failure-counter-with-exit
    - state-machine-health-tracking
    - post-error-drain-delay
key_files:
  created: []
  modified:
    - src/main.rs
decisions:
  - "CRIT-02 uses exit+systemd-restart strategy (not in-process reconnect) — simpler, more reliable, no hardware to test reconnect with"
  - "MAX_CONSECUTIVE_ALL_FAIL=10 as a const in main.rs — no config knob added per plan; can be exposed later if needed"
  - "influx_healthy flag is per-daemon not per-device — all devices write to same InfluxDB instance so one flag is correct"
  - "HIGH-04 drain delay is 100ms, placed before bus_delay() so error path gets: 100ms drain + bus_delay (net extra ~100ms per failed device)"
metrics:
  duration: "~2 min"
  completed: "2026-04-03"
  tasks_completed: 4
  files_modified: 1
---

# Phase 07 Plan 03: Runtime Resilience Summary

**One-liner:** Exit-for-restart on serial failure, 100ms Modbus drain delay on timeout, and InfluxDB health-state machine to suppress repeated write-failure log spam.

## What Was Built

Three runtime resilience improvements to `src/main.rs` poll loop addressing the three findings in Plan 07-03:

### T1: Serial Recovery via Exit (CRIT-02)

Added `consecutive_all_fail: u32` counter and `MAX_CONSECUTIVE_ALL_FAIL: u32 = 10` constant before the poll loop. In the tick arm, `any_ok` tracks whether at least one device succeeded per cycle. If no device succeeds, the counter increments and a `tracing::warn!` fires. At 10 consecutive all-fail cycles, `tracing::error!` fires and the loop `break`s, causing `main()` to return `Ok(())` and exit with code 0 — systemd `Restart=always` restarts the process, which re-opens the serial port.

### T2: Modbus Stale-Frame Drain Delay (HIGH-04)

Added `tokio::time::sleep(Duration::from_millis(100))` inside the `Err(e)` arm of the per-device poll match, placed before the existing `poller.bus_delay()`. This 100ms sleep only fires on poll error — not on success — so it doesn't impact normal throughput. It gives the serial buffer time to drain any late or partial Modbus response frame that arrived after the timeout, preventing the stale bytes from corrupting the next device's response.

### T3: InfluxDB Health State Machine (MED-04)

Added `influx_healthy: bool = true` before the poll loop. In the InfluxDB write result handling:
- On error: if `influx_healthy` is true, log `WARN` once and set `influx_healthy = false`; subsequent errors are silently dropped while unhealthy
- On success: if `influx_healthy` is false, log `INFO "InfluxDB connection restored"` and set `influx_healthy = true`

This eliminates log spam during extended InfluxDB outages, while still providing clear transition events in the log.

### T4: Verification

`cargo test`: 39 tests pass (4 ignored — hardware/integration)
`cargo clippy -- -D warnings`: clean, no warnings

## Commits

| Task | Commit | Description |
|------|--------|-------------|
| T1+T2+T3 | `5c1b3f0` | feat(07-03): serial recovery + Modbus drain delay + InfluxDB health tracking |
| T4 | `f56bf07` | test(07-03): verify build — cargo test (39 pass) and cargo clippy clean |

## Deviations from Plan

None — plan executed exactly as written.

The three tasks are implemented precisely as specified in the plan pseudocode. No architectural changes were needed.

## Known Stubs

None — all three resilience mechanisms are fully wired into the poll loop.

## Self-Check: PASSED

- [x] `src/main.rs` modified with all three resilience mechanisms
- [x] Commit `5c1b3f0` exists: `git log --oneline | grep 5c1b3f0`
- [x] Commit `f56bf07` exists: `git log --oneline | grep f56bf07`
- [x] 39 tests pass, clippy clean
