# Project Research Summary

**Project:** rs485-logger
**Domain:** Rust embedded/IoT daemon — RS485/Modbus RTU PZEM-016 power meters → InfluxDB 3 time-series storage on Raspberry Pi
**Researched:** 2026-04-02
**Confidence:** HIGH

## Executive Summary

The rs485-logger is a narrow, purpose-built Rust daemon: poll PZEM-016 power meters over a shared RS485 bus via Modbus RTU, decode the register values, and write them to InfluxDB 3. The dominant architectural constraint is that RS485 is a half-duplex shared bus — **polling must be strictly sequential, never concurrent**. This single rule shapes everything: runtime choice (`current_thread` tokio), polling loop design (plain `for` loop with `.await`), and timeout strategy (per-device `tokio::time::timeout`). The recommended stack is proven and narrow: `tokio-modbus 0.17` + `tokio-serial 5.4.5` for the Modbus side, `reqwest 0.13` for HTTP writes (no official Rust InfluxDB 3 client exists), and `tracing` for structured async-aware logging.

The correct approach is to build this in four clearly ordered phases. First: scaffold the config parser, register decoder, and Modbus poller — unit-testable without hardware. Second: wire the InfluxDB 3 write path using the native `/api/v3/write_lp` endpoint (NOT v1 or v2 paths). Third: harden the full poll loop for real multi-device operation — error isolation, skip-and-continue, graceful shutdown. Fourth: productionise with a systemd unit, udev stable device path, and cross-compilation for ARM. This order is dictated by hard dependencies: you cannot test the InfluxDB write path without types, and you cannot test multi-device resilience without a working single-device poller.

The two highest-risk areas are **PZEM-016 register decoding** and **InfluxDB 3 write semantics**. The PZEM-016 stores 32-bit values (current, power, energy) in low-word-first order, contrary to standard Modbus big-endian convention — getting this wrong produces values off by a factor of 65,536. InfluxDB 3 locks field types on the first write — mixing integer `0i` and float `0.0` for the same field causes permanent schema conflicts requiring a destructive measurement drop. Both pitfalls must be addressed in Phase 1 and Phase 2 respectively, before any data reaches production storage.

---

## Key Findings

### Recommended Stack

The stack is dictated entirely by `tokio-modbus`'s dependency chain: it requires `tokio-serial 5.4.5` and `tokio 1.x`. No alternative async serial or Modbus crate is viable on Linux — `rmodbus` is `no_std`/embedded-focused, `serialport 4.x` is synchronous and incompatible as a transport. For InfluxDB, there is **no official Rust v3 client** — raw `reqwest` with three lines of code is the correct, zero-risk choice over the community `influxdb3` crate (210 total downloads). The `toml` + `serde` combination covers config without the overhead of the `config` crate. `tracing` is mandatory over `log` + `env_logger` because per-device error context in async polling requires structured spans.

**Core technologies:**
- `tokio 1.50.0` + `current_thread` flavor — async runtime; single-thread matches the serial bus constraint
- `tokio-modbus 0.17.0` (RTU feature) — only async Modbus RTU crate; uses `attach_slave()` API (not deprecated `connect()`)
- `tokio-serial 5.4.5` — tokio-native serial port, required by tokio-modbus; open once and reuse with `set_slave()`
- `reqwest 0.13.2` with `rustls-tls` — HTTP client for InfluxDB 3 writes; `rustls-tls` avoids OpenSSL cross-compilation issues
- `serde 1.0.228` + `toml 1.1.1` — TOML config parsing via `toml::from_str` + `#[derive(Deserialize)]`
- `tracing 0.1.44` + `tracing-subscriber 0.3.23` + `tracing-appender 0.2.4` — structured async logging with per-device spans and rolling file output
- `anyhow 1.0.102` — error propagation for a binary daemon; context chains in logs, no typed error variants needed
- `cargo cross` — cross-compilation to `aarch64-unknown-linux-gnu` or `armv7-unknown-linux-gnueabihf`; use `rustls-tls` to avoid OpenSSL linker errors

**Critical InfluxDB 3 API facts (differ significantly from v1/v2):**
- Endpoint: `POST /api/v3/write_lp?db=<DATABASE>` (NOT `/api/v2/write`)
- Auth: `Authorization: Bearer <token>` (NOT `Token <token>`)
- No `org=` parameter; `db=` only; success is `HTTP 204`
- Precision defaults to `auto` — specify explicitly with `?precision=second` or `?precision=ns`

### Expected Features

All PZEM-016 features derive from a single hardware capability: FC 0x04 reads 10 consecutive 16-bit registers starting at 0x0000, yielding all 6 measurements in one round-trip. The poll loop is a sequential `for` over the device list; error isolation per device is the core reliability promise.

**Must have (table stakes — v1):**
- TOML config with device list (address + name), serial port (path, baud, parity), global poll interval, InfluxDB endpoint + token + database
- FC 0x04 register read for all 6 PZEM-016 fields (voltage, current, power, energy, frequency, power factor)
- Sequential polling loop with `tokio::time::interval` tick
- Skip-and-log on device error: `tracing::warn!`, continue to next device — daemon must never exit on a single device failure
- InfluxDB 3 write via `POST /api/v3/write_lp`, per-device measurement name = device name from config
- InfluxDB write failure: log error and continue (data from failed write is dropped in v1)
- Graceful shutdown on SIGTERM/SIGINT: complete current poll cycle, then exit
- Structured logging to stdout (journald captures it); optional log to file with configurable path
- Config validation at startup with clear error messages before entering the poll loop
- Systemd `.service` unit file with `Restart=always`, `RestartSec=5`

**Should have (competitive — v1.x after validation):**
- In-memory write-failure buffer with bounded retry (ring buffer, no disk persistence — avoids SD card wear)
- Startup InfluxDB connectivity check: fail fast before entering poll loop
- Per-device consecutive error counter: distinguish flap from extended offline
- CLI `--config <path>` argument: default `/etc/rs485-logger/config.toml`

**Defer (v2+):**
- Log rotation config (document `logrotate` first)
- Configurable per-device Modbus read timeout (500ms default is sufficient for PZEM-016)
- Multiple serial port support
- Cross-compilation CI release pipeline

**Anti-features (never build for v1):**
- Per-device polling intervals (incompatible with RS485 bus single-master model)
- Disk-persistent write buffer (SD card write amplification is a real Pi failure mode)
- Hot-reload via SIGHUP (`systemctl restart` is instantaneous; partial config reload is risky)
- Modbus TCP support (out of stated scope)
- Web UI / REST API (Grafana + InfluxDB already provides this)

### Architecture Approach

The daemon is a single-crate, single-binary project with 5 modules. The `current_thread` tokio runtime is correct because the RS485 bus is inherently sequential; `set_slave()` on the shared `SerialStream` context avoids re-opening the port per device. All communication between components is direct function calls — no channels needed. `reqwest::Client` lives inside `InfluxWriter` as a singleton for connection pool reuse. Pure functions (`decode_registers`, `to_line_protocol`) are fully unit-testable without hardware.

**Major components:**
1. **Config Loader** (`config.rs`) — parse and validate `config.toml` at startup; fail fast; `AppConfig` / `DeviceConfig` / `InfluxConfig` structs
2. **Modbus Poller** (`poller.rs`) — owns the `SerialStream` singleton; calls `set_slave()` per device; reads FC 0x04 registers 0x0000–0x0009 with a 500ms timeout; returns `PowerReading`
3. **Types + Register Decoder** (`types.rs`) — `PowerReading` struct; `decode_registers()` scales raw `u16[]` to physical units using the PZEM low-word-first decode
4. **InfluxDB Writer** (`influx.rs`) — `to_line_protocol()` pure function; `InfluxWriter` struct with singleton `reqwest::Client`; logs and swallows HTTP errors
5. **Poll Loop + Signal Handler** (`main.rs`) — `tokio::time::interval` tick; sequential `for device in &cfg.devices`; `tokio::select!` on SIGTERM/SIGINT for graceful shutdown

### Critical Pitfalls

1. **PZEM-016 32-bit register word order is low-word-first, not Modbus-standard big-endian** — Current, power, and energy are 32-bit values in two 16-bit registers. Correct decode: `(reg[n+1] as u32) << 16 | reg[n] as u32`. Wrong order produces values off by exactly 65,536x. Unit-test with known arrays before touching hardware.

2. **InfluxDB 3 field types are locked on first write — always write floats, never integers** — Use `power=0.0`, never `power=0i`. One integer write locks the field as INT64; all subsequent float writes are rejected with HTTP 422. Recovery requires dropping the measurement (all historical data lost). Address in Phase 2 before writing any real data.

3. **Sequential polling is mandatory on RS485 — never use `tokio::join!` or concurrent futures for device reads** — RS485 is half-duplex; concurrent requests cause frame collisions and CRC errors. Use a plain `for` loop with `.await`. Add ≥50ms inter-device delay for USB-RS485 adapter RTS direction-switch.

4. **InfluxDB 3 endpoint is `/api/v3/write_lp`, not `/write` or `/api/v2/write`** — Using v1 or v2 endpoint gives 404 or silently drops data. Auth is `Bearer <token>`, not `Token <token>`. Verify with `curl` smoke test before integrating.

5. **Timestamp precision must be explicit** — `SystemTime::now().as_millis()` interpreted as nanoseconds places all writes in 1970. Specify `?precision=second` in the write URL and verify with a post-write SELECT query.

6. **Serial port path is not stable across reboots** — Create a udev rule pinned to USB VID/PID (`/dev/ttyRS485`). Service user must be in `dialout` group. Do NOT use `PrivateDevices=true` in the systemd unit — it excludes USB serial devices from the private `/dev`.

7. **Energy register resets to 0 on power loss** — Write raw gauge values; detect resets (new < last) and log `WARN`. Use `non_negative_derivative()` in Grafana for derived energy calculations.

---

## Implications for Roadmap

Based on research, the build order is fully determined by component dependencies: types → poller → writer → loop. Phases 1–3 can all be verified without deployment hardware (unit tests, local InfluxDB, Modbus simulator). Phase 4 is the only hardware/OS-dependent phase.

### Phase 1: Foundation — Config, Types, and Modbus Polling

**Rationale:** Everything depends on the register decoder and config structs. Types can be fully unit-tested without hardware using known register arrays. This phase has the most domain-specific pitfalls (word order, FC 0x04 only, inter-device delay) and must be solid before any data flows.

**Delivers:** A `cargo test` suite for config parsing and register decoding; a `poller.rs` that can read a single PZEM-016 device.

**Addresses (from FEATURES.md):**
- TOML config (device list, serial port, poll interval)
- FC 0x04 register read for all 6 PZEM-016 fields
- Config validation at startup

**Avoids (from PITFALLS.md):**
- PZEM 32-bit word order (unit-test `decode_registers` with known raw arrays)
- PZEM function code FC 0x04 only (never use `read_holding_registers`)
- tokio-modbus `attach_slave()` API (not deprecated `connect()`)
- Serial port opened once, `set_slave()` per device
- Energy register rollover detection (log WARN when new < last)

**Research flag:** MEDIUM — PZEM-016 register map from ESPHome source, not official datasheet. Plan hardware validation checkpoint.

### Phase 2: InfluxDB Write Integration

**Rationale:** The write path is independent of multi-device complexity and can be tested against a local InfluxDB 3 instance. The InfluxDB API pitfalls (wrong endpoint, field type locking, timestamp precision) must be caught here, before any data accumulates in production.

**Delivers:** `influx.rs` with unit-tested `to_line_protocol()` and integration-tested `InfluxWriter`. A curl smoke test documents the exact endpoint + auth + precision parameters.

**Addresses (from FEATURES.md):**
- InfluxDB 3 write via `/api/v3/write_lp` with `Authorization: Bearer`
- Per-device measurement name = device name
- InfluxDB write failure: log error and continue
- Startup InfluxDB connectivity check (v1.x differentiator, can add here)

**Avoids (from PITFALLS.md):**
- Wrong InfluxDB endpoint (use `/api/v3/write_lp`, verify with curl)
- Field type locking (always write floats — apply scaling, never `0i`)
- Timestamp precision mismatch (use `?precision=second`; verify with SELECT post-write)
- `reqwest::Client` created once and reused (not per-write)

**Research flag:** LOW — official InfluxDB 3 docs are high confidence. Standard patterns apply.

### Phase 3: Full Poll Loop, Error Isolation, and Graceful Shutdown

**Rationale:** With Modbus poller and InfluxDB writer proven independently, wire the full production loop. This phase validates the core reliability promise: one device failing must not affect others, and the daemon must handle InfluxDB outages without crashing.

**Delivers:** A complete running daemon that polls all configured devices sequentially, isolates per-device errors, handles InfluxDB write failures gracefully, and shuts down cleanly on SIGTERM.

**Addresses (from FEATURES.md):**
- Sequential polling loop with global interval
- Skip-and-log on device error (continue to next device)
- Graceful SIGTERM/SIGINT shutdown (complete current cycle)
- Structured logging to stdout + optional file
- Per-device consecutive error counter (v1.x differentiator)
- In-memory write-failure buffer with retry (v1.x differentiator)

**Avoids (from PITFALLS.md):**
- RS485 bus contention (sequential `for` loop, ≥50ms inter-device gap)
- Panicking on device read error (match on Result, warn and continue)
- One HTTP POST per device per field (batch all device lines into one POST body)

**Research flag:** LOW — sequential async polling and error isolation are standard Rust/tokio patterns.

### Phase 4: Deployment, Hardening, and Cross-Compilation

**Rationale:** The daemon is functionally complete after Phase 3. This phase makes it production-deployable on a real Raspberry Pi: stable device path, correct permissions, properly hardened (but not over-hardened) systemd unit, and a repeatable cross-compilation workflow.

**Delivers:** A deployable binary (native Pi or cross-compiled), a systemd service unit that survives reboots, a udev rule for stable device naming, and deployment documentation covering `dialout` group membership, config file permissions, and udev setup.

**Addresses (from FEATURES.md):**
- Systemd `.service` unit with `Restart=always`, `RestartSec=5`
- USB device path stability (udev rule → `/dev/ttyRS485`)
- Deployment documentation

**Avoids (from PITFALLS.md):**
- Serial port path instability (udev rule, not `/dev/ttyUSB0`)
- Serial port permissions (service user in `dialout` group)
- `PrivateDevices=true` in systemd (breaks serial access; use `DeviceAllow=/dev/ttyRS485 rw`)
- Cross-compilation OpenSSL/libudev linker failures (`cargo cross` + `rustls-tls` + `serialport default-features = false`)
- InfluxDB token world-readable (`chmod 640 /etc/rs485-logger/config.toml`)

**Research flag:** LOW — systemd unit authoring, udev rules, and `cargo cross` are well-documented operations.

### Phase Ordering Rationale

- **Config and types first:** `poller.rs` and `influx.rs` both depend on `AppConfig` and `PowerReading`. Register decode unit tests catch the word-order bug in CI before it reaches hardware.
- **InfluxDB write before the full loop:** The field-type-locking pitfall is catastrophic and unrecoverable if discovered after data accumulation. Validate and lock the schema first.
- **Full loop before deployment:** Error-isolation behavior (skip-and-continue) must be verified with simulated device failures in a dev environment before the daemon is systemd-managed.
- **Deployment last:** systemd, udev, and cross-compilation are environment concerns, not code concerns. Get the binary right before hardening its runtime environment.

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 1:** PZEM-016 register map should be validated against actual hardware. The ESPHome `pzemac.cpp` source is the best available reference but the official Peacefair datasheet is vendor-walled. Plan a hardware validation checkpoint: read raw registers from one real device and sanity-check all 6 values.

Phases with standard patterns (skip research-phase):
- **Phase 2:** InfluxDB 3 write API fully documented with official sources. No additional research needed.
- **Phase 3:** Sequential async polling loop and error isolation are standard Rust/tokio patterns. No research needed.
- **Phase 4:** systemd units, udev rules, and `cargo cross` are well-documented. No research needed.

---

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | All crate versions verified against crates.io API; InfluxDB 3 API verified against official docs; tokio-modbus 0.17 API verified against docs.rs |
| Features | HIGH | PZEM-016 register structure verified via ESPHome source; InfluxDB write patterns from official docs; feature prioritization matches stated project requirements |
| Architecture | HIGH | All major claims backed by official crate docs and tokio-modbus official examples; component boundaries follow established single-binary Rust daemon patterns |
| Pitfalls | HIGH | 12 pitfalls documented, each with source reference; PZEM word order from ESPHome battle-tested source; InfluxDB pitfalls from official docs |

**Overall confidence:** HIGH

### Gaps to Address

- **PZEM-016 register map (MEDIUM confidence):** ESPHome source (`pzemac.cpp`) is the most trusted community reference, but the official Peacefair datasheet is behind a vendor wall. Phase 1 must include a hardware validation step: read raw registers and sanity-check (voltage: 100–260V, frequency: 45–55Hz, power factor: 0–1.0).

- **Multi-device bus timing:** The ≥50ms inter-device delay recommendation is based on CH340/CP2102 characterization. Validate by running a 3+ device poll for an extended period in Phase 3 and monitoring for CRC error rate.

- **InfluxDB 3 Core error response format:** Some pitfall sources referenced the Cloud Serverless docs. Field type immutability is the same for both editions, but verify the exact HTTP error code (400 vs 422) on the specific InfluxDB 3 Core build in use.

- **tokio-modbus `set_slave` API name in v0.17:** Architecture Pattern 2 shows `ctx.set_slave(...)`. Confirm this method name in v0.17.0 docs.rs — verify it was not renamed during the 0.8→0.17 version gap.

- **Energy register rollover simulation:** Phase 3 should include a simulated reset test (manually reset PZEM energy counter via FC 0x42) to confirm rollover detection logic fires the expected `WARN` log.

---

## Sources

### Primary (HIGH confidence)
- Official InfluxDB 3 write API — endpoint, auth, precision, line protocol — https://docs.influxdata.com/influxdb3/core/write-data/http-api/v3-write-lp/
- Official InfluxDB 3 client library list (no Rust v3 client exists) — https://docs.influxdata.com/influxdb3/core/reference/client-libraries/v3/
- InfluxDB 3 line protocol reference — field types, timestamp precision, escaping — https://docs.influxdata.com/influxdb3/cloud-serverless/reference/syntax/line-protocol/
- docs.rs `tokio-modbus 0.17.0` — RTU client API, `attach_slave()`, feature flags — https://docs.rs/tokio-modbus/latest/tokio_modbus/
- docs.rs `tokio-serial 5.4.5` — SerialStream, dependency chain — https://docs.rs/tokio-serial/latest/tokio_serial/
- tokio-modbus CHANGELOG — v0.8.0 `attach()` API change, v0.8.2 RX buffer clear fix — https://github.com/slowtec/tokio-modbus/blob/main/CHANGELOG.md
- crates.io API — all crate versions verified (tokio 1.50.0, tokio-modbus 0.17.0, tokio-serial 5.4.5, reqwest 0.13.2, serde 1.0.228, toml 1.1.1, tracing 0.1.44, anyhow 1.0.102)
- Cargo cross-compilation reference — https://doc.rust-lang.org/cargo/reference/config.html#target

### Secondary (MEDIUM confidence)
- ESPHome `pzemac.cpp` — PZEM-016 register layout, word order, FC 0x04, scaling factors — https://github.com/esphome/esphome/blob/dev/esphome/components/pzemac/pzemac.cpp
- InfluxDB 3 schema design guide — field type conflicts, column schema enforcement — https://docs.influxdata.com/influxdb3/cloud-serverless/write-data/best-practices/schema-design/
- PZEM-004T v3.0 library and register documentation — https://github.com/mandulaj/PZEM-004T-v30
- GitHub Modbus ecosystem survey — modbus-rtu (379 repos), modbus-logger (2 repos)

### Tertiary (LOW confidence)
- `influxdb3` community crate (0.2.0, 210 downloads) — evaluated and rejected; no InfluxData backing
- PyScada SCADA — competitor feature comparison only

---
*Research completed: 2026-04-02*
*Ready for roadmap: yes*
