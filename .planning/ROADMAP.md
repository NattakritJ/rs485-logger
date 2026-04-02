# Roadmap: rs485-logger

## Overview

A focused, 4-phase build: stand up the Rust project foundation with config parsing and PZEM-016 data types first (no hardware needed), then build and validate the InfluxDB 3 write path, then integrate real Modbus RTU hardware and wire the full polling loop with error handling, and finally package the daemon for production deployment on Raspberry Pi via systemd. Each phase is fully testable before the next begins.

## Phases

- [x] **Phase 1: Foundation** - Config parsing, data types, and register decoder — fully unit-testable, no hardware or network required (completed 2026-04-02)
- [ ] **Phase 2: InfluxDB Integration** - Line protocol builder and HTTP write client — validates data destination against a local InfluxDB instance before hardware is needed
- [x] **Phase 3: Modbus + Poll Loop** - Hardware integration, full polling loop, error handling, logging, and graceful shutdown (completed 2026-04-02)
- [ ] **Phase 4: Systemd Deployment** - Production packaging: systemd service unit, udev stable device naming, serial permissions, and cross-compilation

## Phase Details

### Phase 1: Foundation
**Goal**: Parse and validate TOML config; define `PowerReading` struct with correct PZEM-016 register decode logic; all logic is unit-testable with no external dependencies.
**Depends on**: Nothing (first phase)
**Requirements**: CFG-01, CFG-02, CFG-03, CFG-04, CFG-05
**Success Criteria** (what must be TRUE):
  1. `cargo test` passes with a sample `config.toml` — device list, serial config, InfluxDB endpoint all deserialize correctly
  2. `decode_registers()` unit test passes: known raw register bytes (from PZEM-016 datasheet) produce correct voltage/current/power/energy/frequency/power_factor values with correct scaling and low-word-first 32-bit reconstruction
  3. Config validation rejects bad input (empty device list, malformed URL, missing token) with a clear error message at startup, not a panic
**Plans**: 3 plans

Plans:
- [x] 01-01-PLAN.md — Project skeleton: Cargo.toml + src stubs that compile (Wave 1)
- [x] 01-02-PLAN.md — Config structs + TOML parsing + startup validation TDD (Wave 2)
- [x] 01-03-PLAN.md — PowerReading struct + decode_registers() TDD (Wave 2, parallel with 01-02)

### Phase 2: InfluxDB Integration
**Goal**: Build the line protocol formatter and InfluxDB 3 HTTP write client; validate end-to-end write path against a local InfluxDB instance using hardcoded `PowerReading` values.
**Depends on**: Phase 1
**Requirements**: STOR-01, STOR-02, STOR-03, STOR-04
**Success Criteria** (what must be TRUE):
  1. `to_line_protocol()` unit test passes: `PowerReading` with known field values produces the correct line protocol string (measurement name = device name, all fields as floats, correct timestamp)
  2. Integration test: `InfluxWriter.write()` POSTs to a running local InfluxDB 3 instance and gets HTTP 204; the written record is queryable via `SELECT *`
  3. Writing a zero-power reading (`power=0.0`) produces a float field, not an integer — verified by querying `SHOW COLUMNS` to confirm `DOUBLE` type
  4. InfluxDB write failure (connection refused) is logged as an error and does NOT panic or block the caller
**Plans**: 2 plans

Plans:
- [ ] 02-01-PLAN.md — `to_line_protocol()` TDD — float-typed line protocol from PowerReading (Wave 1)
- [ ] 02-02-PLAN.md — `InfluxWriter` struct with reqwest HTTP POST + error handling + integration test (Wave 2)

### Phase 3: Modbus + Poll Loop
**Goal**: Integrate `tokio-modbus` RTU client with real PZEM-016 hardware; wire config → poller → writer into the full sequential poll loop with skip-and-continue error handling, structured logging, and graceful shutdown.
**Depends on**: Phase 2
**Requirements**: POLL-01, POLL-02, POLL-03, OPS-01, OPS-02, OPS-03
**Success Criteria** (what must be TRUE):
  1. Single-device poll: `ModbusPoller.poll_device()` reads a live PZEM-016 and returns a `PowerReading` with physically plausible values (voltage 100–260V, frequency 49–51Hz, power factor 0–1.0)
  2. Multi-device poll: daemon polls all configured devices sequentially; each device's data appears as a separate measurement in InfluxDB
  3. Skip-and-continue: disconnect one PZEM-016 — daemon logs a `WARN`, continues polling other devices, and does not restart or crash
  4. SIGTERM: `systemctl stop` (or `kill -SIGTERM`) causes the daemon to complete the current poll cycle and exit cleanly within 5 seconds
  5. Log output: `journalctl` shows structured log lines with device name, measurement values, and any errors
**Plans**: 3 plans

Plans:
- [x] 03-01-PLAN.md — `ModbusPoller` TDD: SerialStream open-once, set_slave(), FC 0x04 read, 500ms timeout (Wave 1)
- [x] 03-02-PLAN.md — Main poll loop: tokio::time::interval, sequential devices, skip-and-warn on error, InfluxDB write per device (Wave 2)
- [x] 03-03-PLAN.md — Signal handling (SIGTERM/SIGINT graceful exit) + tracing-subscriber init + optional file appender (Wave 3)

### Phase 4: Systemd Deployment
**Goal**: Package the daemon for production on Raspberry Pi — systemd service unit, stable `/dev/ttyRS485` udev symlink, serial port permissions, and cross-compiled release binary.
**Depends on**: Phase 3
**Requirements**: OPS-04
**Success Criteria** (what must be TRUE):
  1. `systemctl start rs485-logger` starts the daemon; `systemctl status` shows `active (running)`
  2. After `systemctl stop rs485-logger` (SIGTERM), the service stops cleanly; `systemctl start` restarts it
  3. After `sudo reboot`, the daemon starts automatically within 10 seconds of boot without manual intervention
  4. `/dev/ttyRS485` symlink exists after reboot and after unplugging/replugging the USB-RS485 adapter (udev rule in place)
  5. Cross-compiled `aarch64-unknown-linux-gnu` release binary (`cargo cross build --release --target aarch64-unknown-linux-gnu`) builds without linker errors
**Plans**: 2 plans

Plans:
- [x] 04-01-PLAN.md — systemd `.service` unit + udev rule `/dev/ttyRS485` + `install.sh` deployment script (Wave 1)
- [x] 04-02-PLAN.md — `Cross.toml` for aarch64/armv7 targets + `deploy/build-release.sh` + cross-compiled release binary verification (Wave 1)

## Progress

**Execution Order:** 1 → 2 → 3 → 4

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Foundation | 3/3 | Complete   | 2026-04-02 |
| 2. InfluxDB Integration | 0/2 | Not started | - |
| 3. Modbus + Poll Loop | 3/3 | Complete   | 2026-04-02 |
| 4. Systemd Deployment | 0/2 | Not started | - |
