# Plan 06-01 Summary — EnergyResetConfig + scheduler.rs TDD

## Status: COMPLETED

## What was built

### Cargo.toml
- Added `chrono = { version = "0.4", features = ["clock"] }`
- Added `chrono-tz = "0.10"` (bundles IANA tz database at compile time)

### src/config.rs
- Added `EnergyResetConfig` struct with `enabled: bool`, `timezone: String`, `time: String`
- Added `energy_reset: Option<EnergyResetConfig>` to `AppConfig`
- Updated `test_empty_device_list_rejected` to include `energy_reset: None`

### src/scheduler.rs (new file)
- Implemented `next_reset_instant(now: DateTime<Utc>, tz_str: &str, time_str: &str) -> anyhow::Result<std::time::Instant>`
- Pure function — fully unit-testable without hardware or network
- Handles: timezone parsing, HH:MM time parsing, today vs tomorrow logic (already-passed case)
- 5 unit tests — all passing:
  - `test_next_reset_midnight_bangkok` — 15:00 Bangkok → midnight Apr 3 Bangkok = Apr 2 17:00 UTC
  - `test_next_reset_already_passed` — 01:00 Bangkok (midnight passed) → next midnight
  - `test_next_reset_custom_timezone_utc` — UTC timezone handling
  - `test_energy_reset_config_deserializes` — TOML [energy_reset] section roundtrip
  - `test_energy_reset_absent_is_none` — absent section → None

### src/main.rs
- Added `mod scheduler;` module declaration

## Test results
```
cargo test: 26 passed, 0 failed, 4 ignored
```

## Key design decisions implemented
- **D-06**: `chrono-tz::Tz` for IANA timezone parsing
- **D-07**: Candidate > now check, advance to tomorrow if already past
- **D-10**: `Option<EnergyResetConfig>` — absent TOML section means disabled
