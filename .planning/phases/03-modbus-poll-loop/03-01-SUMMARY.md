---
phase: "03-modbus-poll-loop"
plan: "01"
subsystem: "poller"
tags: ["modbus", "rtu", "serial", "tokio-modbus", "rs485"]
dependency_graph:
  requires: ["src/types.rs (decode_registers)", "src/config.rs (SerialConfig, DeviceConfig)"]
  provides: ["src/poller.rs (ModbusPoller::new, ModbusPoller::poll_device)"]
  affects: ["src/main.rs (mod poller declared)"]
tech_stack:
  added: []
  patterns:
    - "tokio-modbus rtu::attach(port) — opens SerialStream once, reuses for all devices"
    - "set_slave(Slave(addr)) before each read — switches Modbus target on shared bus"
    - "triple .with_context()? chain — handles timeout, transport error, Modbus exception in order"
    - "tokio::time::timeout(Duration::from_millis(500), ...) — 500ms per-device read limit"
key_files:
  created: ["src/poller.rs"]
  modified: ["src/main.rs"]
decisions:
  - "Used rtu::attach(port) not rtu::attach_slave(port, slave) — slave address is set dynamically per call"
  - "Triple .with_context()? correctly unwraps tokio_modbus::Result<T> = Result<Result<T, ExceptionCode>, Error>"
  - "Struct stores client::Context (not SerialStream) — context is the ownable API surface in tokio-modbus 0.17"
metrics:
  duration_secs: 698
  completed_date: "2026-04-02"
  tasks_completed: 2
  files_created: 1
  files_modified: 1
---

# Phase 03 Plan 01: ModbusPoller Implementation Summary

**One-liner:** `ModbusPoller` using `rtu::attach(port)` + `set_slave` per-call + FC 0x04 with 500ms timeout and triple-context error unwrap for `tokio_modbus::Result<T>`

## What Was Built

`src/poller.rs` — `ModbusPoller` struct that opens the RS485 serial port once at construction and reuses the single Modbus RTU `Context` for all devices on the daisy chain. Each `poll_device()` call:

1. Switches Modbus slave address via `ctx.set_slave(Slave(device.address))`
2. Issues FC 0x04 (`read_input_registers(0x0000, 10)`) with a 500ms timeout
3. Unwraps the nested `tokio_modbus::Result<Vec<Word>>` (= `Result<Result<Vec<Word>, ExceptionCode>, Error>`) via three `.with_context()?` calls
4. Decodes the 10 raw registers into a `PowerReading` via `decode_registers()`

All errors propagate as `anyhow::Result` with descriptive context. No panics, no `.unwrap()` in the production path.

## Tasks Completed

| Task | Name | Type | Commit | Files |
|------|------|------|--------|-------|
| 1 | RED: ModbusPoller stub + compile tests | TDD RED | 41b2716 | src/poller.rs (created), src/main.rs |
| 2 | GREEN: Implement ModbusPoller | TDD GREEN | 95f7d0b | src/poller.rs |

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| `rtu::attach(port)` not `rtu::attach_slave(port, slave)` | Slave address changes per device; attaching without a fixed slave lets `set_slave()` switch cleanly before each read |
| `client::Context` stored in struct | `Context` is the owned, dyn-dispatched API surface in tokio-modbus 0.17 — it wraps `Box<dyn Client>` and implements all Reader/Writer traits |
| Triple `.with_context()?` for error chain | `tokio_modbus::Result<T>` is `Result<Result<T, ExceptionCode>, Error>`; timeout adds a third layer. Each `.with_context()?` peels one layer with a device-specific message |
| 500ms timeout | Matches plan spec; gives PZEM-016 reasonable response time on 9600 baud without blocking the poll loop for too long on a dead device |

## Verification Results

```
cargo build   → Finished dev profile with 2 expected dead_code warnings (poller not yet called from main)
cargo test    → 17 passed; 0 failed; 3 ignored (2 InfluxDB + 1 hardware)
```

Success criteria checklist:
- [x] `src/poller.rs` compiles cleanly as part of the workspace
- [x] `ModbusPoller::new(&SerialConfig)` opens serial port with anyhow error context
- [x] `poll_device(&DeviceConfig)` issues FC 0x04 for 10 registers at 0x0000 with 500ms timeout
- [x] All errors return `Err(anyhow)` — no panics, no `.unwrap()` in production path
- [x] `mod poller` declared in `src/main.rs`
- [x] `cargo test` passes (all non-ignored tests)

## Deviations from Plan

None — plan executed exactly as written.

The plan documented `read_input_registers` as returning `Result<Result<Vec<u16>>>` (double Result). Verification against the source confirms: `tokio_modbus::Result<T> = std::result::Result<std::result::Result<T, ExceptionCode>, Error>`. Combined with `tokio::time::timeout` wrapping, the full chain is three levels deep — matching the plan's triple `.with_context()?` exactly.

## Known Stubs

None. `ModbusPoller::new()` and `poll_device()` are fully implemented. The `#[ignore]`-gated test is intentionally hardware-gated (not a stub — it's an integration test that requires a physical RS485 device).

## Self-Check: PASSED

| Item | Status |
|------|--------|
| src/poller.rs | FOUND |
| src/main.rs | FOUND |
| SUMMARY.md | FOUND |
| commit 41b2716 (test RED) | FOUND |
| commit 95f7d0b (feat GREEN) | FOUND |
