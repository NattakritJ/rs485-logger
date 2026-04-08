# S03: Modbus + Poll Loop

**Status:** ✅ Completed 2026-04-02
**Goal:** Integrate `tokio-modbus` RTU client with real PZEM-016 hardware; wire config → poller → writer into the full sequential poll loop with skip-and-continue error handling, structured logging, and graceful shutdown.
**Demo:** Single-device poll returns physically plausible values (voltage 100–260V, frequency 49–51Hz); multi-device poll isolates failures (disconnect one device — daemon logs WARN and continues); SIGTERM causes clean exit within 5 seconds.

## Must-Haves

- `ModbusPoller` with SerialStream open-once, `set_slave()`, FC 0x04 read, 500ms timeout
- Main poll loop: `tokio::time::interval`, sequential devices, skip-and-warn on error, InfluxDB write per device
- Signal handling (SIGTERM/SIGINT graceful exit) + tracing-subscriber init + optional file appender

## Tasks

- [x] T01: `ModbusPoller` TDD — SerialStream open-once, set_slave(), FC 0x04 read, 500ms timeout
- [x] T02: Main poll loop — sequential devices, skip-and-warn on error, InfluxDB write per device
- [x] T03: Signal handling (SIGTERM/SIGINT graceful exit) + tracing-subscriber init + optional file appender

## Files Likely Touched

- `src/poller.rs`
- `src/main.rs`

## Key Decisions (from execution)

- Used `rtu::attach(port)` not `rtu::attach_slave(port, slave)` — slave address is set dynamically per call via `set_slave()`
- Triple `.with_context()?` correctly unwraps `tokio_modbus::Result<T>` = `Result<Result<T, ExceptionCode>, Error>`
- `client::Context` stored in struct (not `SerialStream`) — context is the owned API surface in tokio-modbus 0.17
