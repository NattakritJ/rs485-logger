# Plan 03-01 Summary

## What was built

Extended the TOML configuration to support millisecond-level poll intervals, enabling sub-second polling of PZEM-016 devices.

## Changes

### src/config.rs
- Changed `poll_interval_secs` from `u64` to `Option<u64>`
- Added `poll_interval_ms: Option<u64>` field to `AppConfig`
- Updated `validate_config` to independently check both fields (reject 0 for either)
- Updated all 4 struct-literal helpers to use `Some(10)` / `None`
- Added 4 new tests: `test_poll_interval_ms_accepted`, `test_poll_interval_ms_zero_rejected`, `test_poll_interval_both_absent_valid`, `test_poll_interval_ms_wins_when_both_present`

### src/main.rs
- Added resolution formula: `cfg.poll_interval_ms.unwrap_or_else(|| cfg.poll_interval_secs.unwrap_or(10) * 1000)`
- Added sub-500ms startup warning (`tracing::warn!`)
- Changed startup log field from `interval_secs` to `interval_ms`
- Changed ticker to `Duration::from_millis(poll_interval_ms)`
- Changed cycle-too-long warning to use `interval_ms` field and ms units
- Removed all direct references to `cfg.poll_interval_secs` outside the resolution expression

### config.toml.example
- Updated comments documenting both `poll_interval_secs` and `poll_interval_ms`
- Added commented-out `poll_interval_ms = 500` example line

## Verification

- `cargo test` — 51 passed, 0 failed, 4 ignored
- No stale `interval_secs` references remain in main.rs
- Ticker uses `from_millis(poll_interval_ms)`
- Backward compatible: existing configs with `poll_interval_secs = 10` continue to work

## Commits

- `49cd4c9` — feat(config): add poll_interval_ms field with Option<u64> types
- `ce9374b` — feat(main): wire resolved poll_interval_ms through ticker and logs

## Decisions implemented

D-01 through D-13 from phase context — all implemented in this plan.
