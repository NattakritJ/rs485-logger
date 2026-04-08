# S06: Daily Energy Reset

**Status:** ✅ Completed 2026-04-03
**Goal:** Schedule and send Modbus FC 0x42 (Reset Energy) to each configured PZEM-016 device once per day at 00:00 Asia/Bangkok time, so the accumulated energy counter starts fresh each day.
**Demo:** At midnight Asia/Bangkok time, FC 0x42 is sent sequentially to all configured devices; energy counters reset to 0 in InfluxDB; `far_future()` parks the select! arm when energy reset is disabled in config.

## Must-Haves

- `EnergyResetConfig` in TOML (enabled flag, timezone, time-of-day)
- `chrono-tz` dependency for IANA timezone support at compile time
- `next_reset_instant()` function (TDD) — computes next reset instant from now
- `reset_energy()` on `ModbusPoller` — sends FC 0x42 to each device
- `main.rs` daily reset `select!` arm — biased select with `far_future()` to park when disabled

## Tasks

- [x] T01: EnergyResetConfig + chrono-tz deps + next_reset_instant() TDD
- [x] T02: reset_energy() on ModbusPoller + main.rs daily reset select! arm

## Files Likely Touched

- `src/config.rs`
- `src/poller.rs`
- `src/main.rs`
- `Cargo.toml`

## Key Decisions (from execution)

- `far_future()` parks the reset sleep arm when disabled — no conditional select! needed; clean biased select! structure
- `chrono-tz` chosen for IANA timezone support — bundles tz database at compile time; avoids runtime system tz dependency
