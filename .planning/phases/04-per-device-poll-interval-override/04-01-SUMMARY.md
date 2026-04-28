---
phase: 04-per-device-poll-interval-override
plan: 01
status: complete
started: 2026-04-28T14:09:00Z
completed: 2026-04-28T14:12:00Z
---

# Plan 04-01 Summary: Config schema — add poll_interval_ms to DeviceConfig

## What Was Built

Added `poll_interval_ms: Option<u64>` to `DeviceConfig` struct, enabling per-device poll interval overrides in the TOML config. The field is optional — configs without it parse exactly as before (backward compatible). Validation rejects `poll_interval_ms = 0` per device, matching the global interval validation rule.

## Tasks Completed

| # | Task | Status |
|---|------|--------|
| 1 | Add poll_interval_ms to DeviceConfig + validation + tests | ✓ |
| 2 | Fix test struct literals in poller.rs + update config files | ✓ |

## Key Files

### Created
- (none)

### Modified
- `src/config.rs` — DeviceConfig struct + validate_config + 2 new tests + updated 3 helper functions
- `src/poller.rs` — Updated 2 test DeviceConfig literals with `poll_interval_ms: None`
- `config.toml.example` — Added commented `poll_interval_ms` example in first [[devices]] block

## Self-Check: PASSED

- `cargo test` — 53 passed, 0 failed, 4 ignored
- `DeviceConfig` has `poll_interval_ms: Option<u64>`
- `validate_config` rejects `poll_interval_ms = 0`
- `validate_config` accepts `poll_interval_ms = 500`
- Configs without per-device interval parse without error
- All existing tests pass (no regressions)

## Deviations

None — executed as planned.
