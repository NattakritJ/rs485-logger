# Plan 06-02 Summary — reset_energy() + main.rs select! wiring

## Status: COMPLETED

## What was built

### src/poller.rs
- Added `use std::borrow::Cow;` import
- Implemented `ModbusPoller::reset_energy(&DeviceConfig) -> anyhow::Result<()>`
  - Sets slave address, sends `Request::Custom(0x42, Cow::Borrowed(&[]))` via `ctx.call()`
  - 500ms timeout consistent with `poll_device`
  - `Response::Custom(0x42, _)` → `Ok(())`
  - `Response::Custom(0xC2, data)` → WARN log + `Ok(())` (skip-and-log, D-02/D-12)
  - Any other response → `Err(...)`
  - Added `test_reset_energy_signature_compiles` (hardware-ignored compile test)

### src/main.rs
- Added imports: `use chrono::Utc`, `use scheduler::next_reset_instant`
- Added `far_future()` helper — returns Instant 100 years ahead (disabled arm park)
- Added `log_next_reset()` helper — logs next reset in local timezone (D-13)
- Before the loop: computes `initial_reset_deadline` from `next_reset_instant()` or `far_future()`
- Pinned `reset_sleep = tokio::time::sleep_until(initial_reset_deadline)` outside loop
- Added third `tokio::select!` arm: `_ = &mut reset_sleep, if reset_enabled`
  - Logs "Daily energy reset starting" (D-11)
  - Loops over all devices, calls `poller.reset_energy(device)` per device (D-01)
  - WARN on per-device failure, continues to next (D-12)
  - Recomputes next deadline via `next_reset_instant()` after firing (D-08)
  - Calls `reset_sleep.as_mut().reset(next_deadline)` — no drift (D-08)
- Disabled case: `reset_sleep` parks at `far_future()`, `if reset_enabled` guard prevents firing

## Test results
```
cargo test: 26 passed, 0 failed, 4 ignored
cargo build --release: 0 errors
```

## Key design decisions implemented
- **D-01**: Per-device loop — `reset_energy(device)` called individually
- **D-02**: 0xC2 WARN + continue (skip-and-log)
- **D-03**: `ctx.call(Request::Custom(...))` via tokio-modbus
- **D-04**: `Cow::Borrowed(&[])` — empty PDU data bytes
- **D-05**: 500ms timeout
- **D-08**: `next_reset_instant(Utc::now(), ...)` after each fire — no drift
- **D-09**: Third `tokio::select!` arm (not a spawned task)
- **D-11/D-12/D-13**: Structured tracing logs at appropriate levels
