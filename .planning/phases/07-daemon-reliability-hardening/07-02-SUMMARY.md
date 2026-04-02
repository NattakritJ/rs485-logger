---
phase: "07"
plan: "02"
subsystem: config-validation
tags: [config, validation, security, reliability]
dependency_graph:
  requires: []
  provides: [device-name-sanitization, energy-reset-early-validation, clock-warning]
  affects: [config.rs, types.rs]
tech_stack:
  added: []
  patterns: [chrono-tz-parse, atomicbool-once-warning, anyhow-ensure]
key_files:
  created: []
  modified:
    - src/config.rs
    - src/types.rs
decisions:
  - "Device name validation uses chars().all() whitelist (alphanumeric + underscore) — rejects spaces, commas, newlines that break InfluxDB line protocol measurement names"
  - "Energy reset timezone/time eagerly validated in validate_config() — turns silent runtime failure into startup fatal error"
  - "System clock warning uses AtomicBool swap to log only once across all poll iterations — prevents log spam"
  - "Invalid energy_reset when disabled is silently accepted — validation only runs when enabled=true"
metrics:
  duration_secs: 90
  completed_date: "2026-04-03"
  tasks_completed: 4
  files_modified: 2
---

# Phase 07 Plan 02: Config Validation Hardening Summary

**One-liner:** Startup-time validation for device names (InfluxDB safety), energy reset timezone/time (early error), and system clock sanity warning (log once).

## What Was Built

Added three validation layers to prevent misconfiguration from causing silent failures at runtime:

1. **Device name sanitization (HIGH-02)** — `validate_config()` now rejects any device name with characters that break InfluxDB line protocol (spaces, commas, newlines, special chars). Only alphanumeric and underscore allowed. Empty names also rejected with a clear address-based error.

2. **Energy reset eager validation (MED-05)** — When `energy_reset.enabled = true`, `validate_config()` parses the timezone via `chrono_tz::Tz` and parses the time via `chrono::NaiveTime::parse_from_str("%H:%M")` at startup. Invalid configs cause a fatal error immediately instead of silently failing at midnight.

3. **System clock sanity warning (LOW-02)** — `decode_registers()` in `types.rs` checks if the computed `timestamp_secs` is before `1_704_067_200` (2024-01-01 UTC). If the clock appears wrong, a `tracing::warn!` is emitted exactly once using an `AtomicBool` to prevent log spam on every poll.

## Tasks Completed

| Task | Description | Commit |
|------|-------------|--------|
| T1 | Device name validation — empty + invalid chars rejected | 1475543 |
| T2 | Energy reset timezone/time eager validation at startup | 1475543 |
| T3 | System clock warning with AtomicBool once-guard | 1475543 |
| T4 | Unit tests: 9 new tests covering all validation branches | 1475543 |

> **Note:** All four tasks were implemented in a single commit (`1475543`) by the parallel 07-01 agent which included 07-02 scope. Verified via `cargo test` — 35 tests pass, 4 ignored (hardware/integration).

## Tests Added (T4)

- `test_device_name_with_space_rejected` — spaces rejected
- `test_device_name_with_comma_rejected` — commas rejected  
- `test_device_name_with_newline_rejected` — newlines rejected
- `test_device_name_empty_rejected` — empty name rejected
- `test_device_name_valid_alphanumeric_and_underscore_passes` — valid name passes
- `test_invalid_timezone_rejected_when_enabled` — bad timezone → startup error
- `test_invalid_time_format_rejected_when_enabled` — bad time format → startup error
- `test_invalid_timezone_not_checked_when_disabled` — disabled reset → no validation
- `test_valid_energy_reset_passes` — valid Asia/Bangkok + 00:00 → passes

## Deviations from Plan

None — all four tasks executed exactly as specified. Tasks were pre-implemented by the parallel 07-01 agent and verified complete.

## Known Stubs

None — all validations are fully wired and tested.

## Self-Check: PASSED

- `src/config.rs` — exists, all validation code present ✓
- `src/types.rs` — exists, CLOCK_WARNED AtomicBool + timestamp check present ✓  
- Commit `1475543` — exists in git log ✓
- 35 tests pass, 0 failures ✓
