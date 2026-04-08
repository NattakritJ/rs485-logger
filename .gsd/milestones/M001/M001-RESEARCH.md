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

# Architecture Research

**Domain:** Rust Modbus RTU polling daemon (PZEM-016 → InfluxDB 3)
**Researched:** 2026-04-02
**Confidence:** HIGH (all major claims verified against official crate docs and InfluxDB 3 documentation)

---

## Standard Architecture

### System Overview

```
┌────────────────────────────────────────────────────────────────┐
│                        Daemon Process                           │
├────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌─────────────┐   startup    ┌──────────────────────────────┐ │
│  │ Config      │ ────────────▶│  Scheduler / Poll Loop       │ │
│  │ Loader      │              │  (tokio::time::interval)     │ │
│  │ (config.rs) │              └──────────┬───────────────────┘ │
│  └─────────────┘                         │ for each device      │
│                                          │ (sequential)         │
│  ┌─────────────────────────────────────┐ │                      │
│  │         Modbus Poller               │◀┘                      │
│  │  (tokio-modbus rtu::attach_slave)   │                        │
│  │  SerialStream (tokio-serial)        │                        │
│  │  Re-address slave per device        │                        │
│  └──────────────┬──────────────────────┘                       │
│                 │ Result<PowerReading>                          │
│                 │                                               │
│  ┌──────────────▼──────────────────────┐                       │
│  │     Measurement Struct              │                        │
│  │  PowerReading { voltage, current,   │                        │
│  │    power, energy, freq, pf,         │                        │
│  │    device_name, timestamp }         │                        │
│  └──────────────┬──────────────────────┘                       │
│                 │                                               │
│  ┌──────────────▼──────────────────────┐                       │
│  │      Line Protocol Builder          │                        │
│  │  (format string → String body)      │                        │
│  └──────────────┬──────────────────────┘                       │
│                 │                                               │
│  ┌──────────────▼──────────────────────┐                       │
│  │      InfluxDB Writer                │                        │
│  │  (reqwest HTTP POST /api/v3/        │                        │
│  │   write_lp?db=…)                    │                        │
│  └─────────────────────────────────────┘                       │
│                                                                 │
│  ┌─────────────────────────────────────┐                       │
│  │  Logger (tracing + tracing-appender)│ (spans all components)│
│  └─────────────────────────────────────┘                       │
└────────────────────────────────────────────────────────────────┘
       │                                        │
  /dev/ttyUSBx                           InfluxDB 3 HTTP
  RS485 bus                              /api/v3/write_lp
```

---

### Component Responsibilities

| Component | Responsibility | Key Types |
|-----------|----------------|-----------|
| **Config Loader** (`config.rs`) | Parse `config.toml` at startup; panic on missing/invalid config | `AppConfig`, `DeviceConfig`, `InfluxConfig` structs (serde + toml) |
| **Scheduler / Poll Loop** (`main.rs`) | Drive the global polling interval; iterate over devices sequentially; route errors to logger | `tokio::time::interval`, `loop` |
| **Modbus Poller** (`poller.rs`) | Open and hold a single `SerialStream`; re-address the Modbus context per device using `set_slave`; issue `read_input_registers(0x0000, 10)` (FC 0x04); decode raw `u16` register values | `tokio_modbus::rtu::attach_slave`, `tokio_serial::SerialStream` |
| **PowerReading struct** (`types.rs`) | Typed representation of one successful poll; owns `device_name` (for InfluxDB table name) and `timestamp` | `PowerReading { voltage: f32, current: f32, power: f32, energy: f32, frequency: f32, power_factor: f32, device_name: String, timestamp_ns: i64 }` |
| **Line Protocol Builder** (`influx.rs`) | Convert `PowerReading` → InfluxDB 3 line protocol string; use device_name as table name (no tags needed, device is implied by table) | `fn to_line_protocol(reading: &PowerReading) -> String` |
| **InfluxDB Writer** (`influx.rs`) | POST line protocol body to `/api/v3/write_lp`; attach `Authorization: Bearer <token>` header; log HTTP errors, do not panic | `reqwest::Client` (kept alive across writes), async POST |
| **Logger** (wired in `main.rs`) | Structured logs to stderr + rolling file; use `tracing` macros throughout; configure via `tracing-subscriber` + `tracing-appender` | `tracing`, `tracing-subscriber`, `tracing-appender` |

---

## Recommended Project Structure

Single Rust crate (no workspace needed — this is one small binary):

```
rs485-logger/
├── Cargo.toml
├── config.toml              # runtime config (not in src/)
├── rs485-logger.service     # systemd unit file
└── src/
    ├── main.rs              # tokio::main, tracing init, poll loop
    ├── config.rs            # AppConfig / DeviceConfig / InfluxConfig structs + load_config()
    ├── types.rs             # PowerReading struct, register decode logic
    ├── poller.rs            # ModbusPoller struct, poll_device() async fn
    └── influx.rs            # to_line_protocol(), InfluxWriter struct, write() async fn
```

### Structure Rationale

- **Single crate, no workspace:** The project is one binary with ~5 modules. Workspace adds complexity with no benefit at this scale.
- **`config.rs` separate from `main.rs`:** Config logic can be unit-tested independently. Struct definitions are co-located with parsing.
- **`types.rs` for `PowerReading`:** Central place for the canonical data type. Keeps decode logic (raw `u16[]` → floats) close to the struct definition.
- **`poller.rs` owns the `SerialStream`:** The serial port is a singleton resource. Wrapping it in a `ModbusPoller` struct with methods makes it easy to test and mock.
- **`influx.rs` owns HTTP client:** `reqwest::Client` should be created once and reused (connection pool). Keeping it in a struct means `main` doesn't need to pass it around.

---

## Architectural Patterns

### Pattern 1: Single-Threaded Async Loop (`current_thread` runtime)

**What:** Use `#[tokio::main(flavor = "current_thread")]` and a single async polling loop that awaits each device in sequence. No concurrent device polling.

**When to use:** Always, for RS485 Modbus RTU. The RS485 bus is half-duplex — only one master transaction can be in flight at a time. Concurrent polling would corrupt responses. `current_thread` also minimises memory overhead on the Raspberry Pi.

**Trade-offs:**
- ✅ No bus contention — sequential `.await` guarantees only one transaction at a time
- ✅ Lowest memory footprint (no thread pool overhead)
- ✅ No `Send` bounds needed on futures
- ❌ If one poll blocks for a very long time the interval drifts — mitigated by setting a short per-device timeout

**Example:**

```rust
#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let cfg = config::load_config("config.toml")?;
    let mut poller = poller::ModbusPoller::new(&cfg.serial).await?;
    let writer = influx::InfluxWriter::new(&cfg.influx);
    let mut interval = tokio::time::interval(
        std::time::Duration::from_secs(cfg.poll_interval_secs)
    );
    loop {
        interval.tick().await;
        for device in &cfg.devices {
            match poller.poll_device(device).await {
                Ok(reading) => {
                    let lp = influx::to_line_protocol(&reading);
                    if let Err(e) = writer.write(&lp).await {
                        tracing::error!(device = %device.name, error = %e, "InfluxDB write failed");
                    }
                }
                Err(e) => {
                    tracing::warn!(device = %device.name, error = %e, "Modbus poll failed, skipping");
                }
            }
        }
    }
}
```

---

### Pattern 2: Re-address One Serial Context Per Poll Cycle

**What:** Open the serial port once at startup. On each poll cycle, call `ctx.set_slave(Slave(device.address))` before issuing the read — do **not** reopen the serial port for each device.

**When to use:** Multiple devices on the same RS485 bus share one physical serial port (`/dev/ttyUSB0`). Opening/closing the port per device adds latency and can miss the RS485 turnaround window.

**Trade-offs:**
- ✅ One port open for entire daemon lifetime — no port contention or re-open errors
- ✅ Much faster per-device poll (no OS open overhead)
- ❌ If the serial port disconnects (USB unplug), the whole daemon must be restarted — mitigated by systemd `Restart=always`

**Example:**

```rust
// poller.rs
pub struct ModbusPoller {
    ctx: tokio_modbus::client::Context,
}

impl ModbusPoller {
    pub async fn new(cfg: &SerialConfig) -> anyhow::Result<Self> {
        let builder = tokio_serial::new(&cfg.device, cfg.baud_rate);
        let port = tokio_serial::SerialStream::open(&builder)?;
        // Initial slave address doesn't matter; will be set per poll
        let ctx = tokio_modbus::prelude::rtu::attach_slave(
            port,
            tokio_modbus::Slave(cfg.devices[0].address),
        );
        Ok(Self { ctx })
    }

    pub async fn poll_device(&mut self, device: &DeviceConfig) -> anyhow::Result<PowerReading> {
        self.ctx.set_slave(tokio_modbus::Slave(device.address));
        // PZEM-016: FC 0x04, start addr 0x0000, 10 registers
        let regs = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            self.ctx.read_input_registers(0x0000, 10),
        )
        .await
        .map_err(|_| anyhow::anyhow!("timeout"))??;
        Ok(decode_registers(&regs, &device.name))
    }
}
```

---

### Pattern 3: Line Protocol String Building (no external client library)

**What:** Format InfluxDB 3 line protocol as a `String` using Rust `format!`. No need for the `influxdb3-client` crate (which is not published as a stable standalone crate for InfluxDB 3 Core). POST to `/api/v3/write_lp?db=<database>` with `reqwest`.

**When to use:** InfluxDB 3 write API is a simple HTTP POST of newline-delimited text. A dedicated client library adds a heavy dependency for something expressible in 10 lines of Rust.

**Trade-offs:**
- ✅ Zero extra dependencies — just `reqwest`
- ✅ Full control over precision (send `precision=second` query param, use Unix timestamp in seconds to avoid nanosecond overflow concerns)
- ❌ Escaping special characters in measurement names is manual — but device names in TOML are under operator control, so this is acceptable for v1

**Line protocol format for PZEM-016:**

```
# Format: <table> <fields> <timestamp_seconds>
# table = device name (from config), no tags needed
living_room voltage=230.1,current=1.52,power=350.2,energy=1234.5,frequency=50.0,power_factor=0.98 1743590400
```

**Example:**

```rust
// influx.rs
pub fn to_line_protocol(r: &PowerReading) -> String {
    format!(
        "{} voltage={},current={},power={},energy={},frequency={},power_factor={} {}",
        r.device_name,
        r.voltage, r.current, r.power, r.energy, r.frequency, r.power_factor,
        r.timestamp_secs
    )
}

pub struct InfluxWriter {
    client: reqwest::Client,
    url: String,      // e.g. "http://localhost:8181/api/v3/write_lp?db=power&precision=second"
    token: String,
}

impl InfluxWriter {
    pub async fn write(&self, line_protocol: &str) -> anyhow::Result<()> {
        let resp = self.client
            .post(&self.url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Content-Type", "text/plain; charset=utf-8")
            .body(line_protocol.to_owned())
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!("InfluxDB returned {}: {}", resp.status(), resp.text().await?);
        }
        Ok(())
    }
}
```

---

### Pattern 4: PZEM-016 Register Decoding

**What:** The PZEM-016 returns 10 consecutive 16-bit input registers starting at address 0x0000 via Function Code 0x04. Registers must be scaled.

**Register map (verified from PZEM-016 datasheet and ESPHome community sources):**

| Register | Field | Scale | Type |
|----------|-------|-------|------|
| 0x0000 | Voltage | ÷ 10 → V | u16 |
| 0x0001 | Current (low word) | — | u16 |
| 0x0002 | Current (high word) | combined ÷ 1000 → A | u16 |
| 0x0003 | Power (low word) | — | u16 |
| 0x0004 | Power (high word) | combined ÷ 10 → W | u16 |
| 0x0005 | Energy (low word) | — | u16 |
| 0x0006 | Energy (high word) | combined → Wh | u16 |
| 0x0007 | Frequency | ÷ 10 → Hz | u16 |
| 0x0008 | Power Factor | ÷ 100 | u16 |
| 0x0009 | Alarm status | 0 = no alarm | u16 |

**Confidence:** MEDIUM — Register map is from community/ESPHome sources, not the official Peacefair datasheet (which is behind a vendor wall). Validate against a real device before finalising.

```rust
// types.rs
fn decode_registers(regs: &[u16], device_name: &str) -> PowerReading {
    let voltage = regs[0] as f32 / 10.0;
    let current = ((regs[2] as u32) << 16 | regs[1] as u32) as f32 / 1000.0;
    let power   = ((regs[4] as u32) << 16 | regs[3] as u32) as f32 / 10.0;
    let energy  = ((regs[6] as u32) << 16 | regs[5] as u32) as f32;
    let freq    = regs[7] as f32 / 10.0;
    let pf      = regs[8] as f32 / 100.0;
    PowerReading { voltage, current, power, energy, frequency: freq, power_factor: pf,
                   device_name: device_name.to_string(),
                   timestamp_secs: chrono::Utc::now().timestamp() }
}
```

---

## Data Flow

### Primary Poll Flow (Happy Path)

```
tokio::time::interval::tick()
        │
        ▼
for device in cfg.devices (sequential, single-threaded)
        │
        ▼
ctx.set_slave(device.address)          // re-address, no reopen
        │
        ▼
ctx.read_input_registers(0x0000, 10)   // FC 0x04, 10 x u16
        │
        ▼
Result<Vec<u16>>
        │ Ok
        ▼
decode_registers(regs, device.name)    // scale raw u16 → f32 fields
        │
        ▼
PowerReading { voltage, current, power, energy, freq, pf, name, timestamp }
        │
        ▼
to_line_protocol(reading)              // format! → String
        │
        ▼
POST /api/v3/write_lp?db=power&precision=second
  Authorization: Bearer <token>
  Body: "living_room voltage=230.1,... 1743590400"
        │
        ▼
HTTP 204 No Content (success)
```

### Error Path (Device Offline / Timeout)

```
ctx.read_input_registers(0x0000, 10)
        │
        │ Err(timeout) or Err(CRC error)
        ▼
tracing::warn!("poll failed, skipping")
        │
        ▼
continue to next device          // daemon stays alive
```

### Error Path (InfluxDB Unreachable)

```
POST /api/v3/write_lp …
        │
        │ Err(connection refused) or HTTP 5xx
        ▼
tracing::error!("InfluxDB write failed: {e}")
        │
        ▼
continue to next device          // reading is DROPPED (no local buffer in v1)
```

**Note:** v1 does not buffer writes on InfluxDB failure. This means data gaps occur if InfluxDB is unreachable. Acceptable per project requirements; a write buffer / retry queue is out of scope.

---

## Suggested Build Order

**Rule:** Each step must compile and run before moving to the next. Build the scaffold before wiring live hardware.

| Step | Component | What You Can Test |
|------|-----------|-------------------|
| 1 | **Config loader** (`config.rs`) | `cargo test` — parse a sample `config.toml`, assert struct fields |
| 2 | **Types + register decoder** (`types.rs`) | `cargo test` — unit-test `decode_registers` with known raw register arrays |
| 3 | **Line protocol builder** (`influx.rs` — format only) | `cargo test` — assert output string matches expected line protocol |
| 4 | **InfluxDB writer** (`influx.rs` — HTTP) | Integration test: start InfluxDB locally, POST a hardcoded point, verify it lands |
| 5 | **Modbus poller** (`poller.rs`) | Integration test with real hardware OR with a Modbus RTU simulator (e.g. `diagslave`) |
| 6 | **Poll loop** (`main.rs`) | End-to-end: full daemon reading one real device and writing to InfluxDB |
| 7 | **Logger wiring** | Verify console + file output at correct levels |
| 8 | **systemd unit** | `systemctl start rs485-logger`, verify restart-on-failure behaviour |

**Critical dependency chains:**
- Config must exist before Poller (poller reads serial path + baud from config)
- Types must exist before Poller (poll returns `PowerReading`)
- Line protocol builder must exist before InfluxDB writer (writer calls builder)
- All of steps 1–4 can be built and tested without hardware

---

## RS485 Bus Contention: Why Sequential Polling Is Mandatory

RS485 is a half-duplex differential bus. Modbus RTU is a strict master/slave protocol: the master sends a request, waits for the slave's response, and only then may send the next request. On a daisy-chained bus:

1. **Only one transaction in flight at a time.** If the daemon issued two `read_input_registers` concurrently (e.g. via `tokio::join!`), the second request would be placed on the wire while the first device is still replying. The responses would collide, both CRCs would fail, and both reads would error.

2. **Inter-frame gap required.** Modbus RTU requires a 3.5-character silent gap between frames. The `tokio-modbus` library handles this, but only if requests are issued sequentially.

3. **Implementation:** Use a simple `for` loop with `.await` — do NOT use `tokio::join_all`, `FuturesUnordered`, or any concurrent dispatch. The single-threaded async runtime (`current_thread`) makes this the natural default.

4. **Timeout per device:** Wrap each `read_input_registers` in `tokio::time::timeout(Duration::from_millis(500), ...)`. If a device doesn't respond within 500 ms, abort that device's read and move on. This prevents a dead device from blocking the entire poll cycle. At 9600 baud, a 10-register response is ~30 ms — 500 ms is generous.

---

## Scaling Considerations

This is a Raspberry Pi daemon reading ≤16 devices. "Scaling" means adding more PZEM-016 units, not users.

| Scenario | Impact | Recommendation |
|----------|--------|----------------|
| 1–4 devices | Baseline | Any poll interval ≥ 5 s is fine; full poll cycle < 2 s |
| 5–16 devices | ~8 s per cycle at 500 ms timeout each | Keep poll interval ≥ 30 s; reduce timeout to 300 ms |
| > 16 devices | Outside PZEM-016 address range (1–16) | Not supported by PZEM-016 hardware |
| InfluxDB remote (LAN) | +network latency per write | Batch all device readings into a single `\n`-joined POST body to reduce round-trips |
| Multiple RS485 buses | Need second USB adapter + second port | Instantiate two `ModbusPoller`s, run two poll loops in separate tokio tasks (safe because each has its own `SerialStream`) |

---

## Anti-Patterns

### Anti-Pattern 1: Concurrent Device Polling

**What people do:** Use `futures::future::join_all` to poll all devices simultaneously for speed.

**Why it's wrong:** RS485 is half-duplex. Concurrent requests corrupt the bus — both requests and responses collide. All reads fail with CRC errors.

**Do this instead:** Sequential `for` loop with `.await`. With 9600 baud and a 500 ms timeout, 8 devices take ~4 s worst-case — fully acceptable for sensor data.

---

### Anti-Pattern 2: Re-opening the Serial Port Per Device

**What people do:** `SerialStream::open` inside the per-device loop to get a "fresh" context.

**Why it's wrong:** Opening a serial port is expensive (~10–100 ms on Linux for device enumeration and driver setup). With 8 devices at 10 s intervals, this wastes ~1 s per cycle. More critically, USB-to-RS485 adapters can fail to re-enumerate quickly, causing spurious errors.

**Do this instead:** Open once at startup, use `set_slave()` to change the addressed device.

---

### Anti-Pattern 3: Multi-Threaded Tokio Runtime for Serial I/O

**What people do:** Use the default `#[tokio::main]` (multi-threaded) because it's the default.

**Why it's wrong:** Serial I/O doesn't benefit from parallelism (the bus is serial). The multi-threaded runtime doubles memory use and adds `Send` bound requirements on futures, which can complicate code with non-`Send` serial port handles.

**Do this instead:** `#[tokio::main(flavor = "current_thread")]` — matches the tokio-modbus RTU example exactly.

---

### Anti-Pattern 4: Panicking on Device Read Error

**What people do:** Use `.unwrap()` or `?` directly in the poll loop, causing the daemon to exit when a single device fails.

**Why it's wrong:** Power meters get disconnected, cables fail, and devices lock up. A single device failure should not take down monitoring for all other devices.

**Do this instead:** `match` on the `Result`, log the error with `tracing::warn!`, and `continue` to the next device. The loop must never `?`-propagate device-level errors.

---

### Anti-Pattern 5: Building Line Protocol Manually Without Escaping Table Names

**What people do:** Use `format!("{} ...", device.name, ...)` without sanitising `device.name`.

**Why it's wrong:** InfluxDB 3 line protocol requires spaces and commas in table names to be backslash-escaped. A device named `"living room"` would produce invalid line protocol.

**Do this instead:** Either (a) document that device names in `config.toml` must be snake_case/no-spaces, and validate at startup, or (b) replace spaces with `_` in the builder. For v1, option (a) is simpler.

---

## Integration Points

### External Services

| Service | Integration Pattern | Notes |
|---------|---------------------|-------|
| RS485 / PZEM-016 | `tokio-modbus` RTU client over `tokio-serial` `SerialStream` | Open once, `set_slave()` per device; FC 0x04, registers 0x0000–0x0009 |
| InfluxDB 3 | HTTP POST to `/api/v3/write_lp?db=<db>&precision=second` with `Authorization: Bearer <token>` | InfluxDB 3 Core, not v1/v2 — use `/api/v3/` endpoint, not `/api/v2/write` |
| systemd | Standard unit file with `Restart=always`, `RestartSec=5` | Handles USB disconnects, InfluxDB down-time, and crash recovery |

### Internal Boundaries

| Boundary | Communication | Notes |
|----------|---------------|-------|
| `main.rs` ↔ `poller.rs` | Direct function call: `poller.poll_device(&device) → Result<PowerReading>` | No channel needed — single-threaded loop |
| `main.rs` ↔ `influx.rs` | Direct async call: `writer.write(&lp) → Result<()>` | `reqwest::Client` lives inside `InfluxWriter`, not passed in |
| `poller.rs` ↔ `types.rs` | `decode_registers(regs, name) → PowerReading` | Pure function, no I/O — easy to unit-test |
| `influx.rs` ↔ `types.rs` | `to_line_protocol(reading) → String` | Pure function — easy to unit-test |

---

## Sources

- tokio-modbus 0.17.0 docs + RTU client example: https://docs.rs/tokio-modbus/latest/tokio_modbus/
- tokio-modbus RTU client example (official): https://raw.githubusercontent.com/slowtec/tokio-modbus/main/examples/rtu-client.rs
- tokio-serial 5.4.5 docs: https://docs.rs/tokio-serial/latest/tokio_serial/
- InfluxDB 3 Core line protocol reference: https://docs.influxdata.com/influxdb3/core/reference/line-protocol/
- InfluxDB 3 Core v3 write_lp API: https://docs.influxdata.com/influxdb3/core/write-data/http-api/v3-write-lp/
- tracing 0.1.44 docs: https://docs.rs/tracing/latest/tracing/
- PZEM-016 register map: MEDIUM confidence (community sources / ESPHome); validate against physical device

---

*Architecture research for: Rust Modbus RTU polling daemon (rs485-logger)*
*Researched: 2026-04-02*

# Stack Research

**Domain:** Rust embedded/IoT daemon — RS485/Modbus RTU → InfluxDB 3 time-series logger
**Researched:** 2026-04-02
**Confidence:** HIGH (all versions verified against crates.io; InfluxDB 3 write API verified against official docs)

---

## Recommended Stack

### Core Technologies

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| `tokio` | 1.50.0 | Async runtime | The de-facto Rust async runtime; `tokio-modbus` and `reqwest` both depend on it — no alternative viable. Feature `full` or targeted `rt-multi-thread,time,signal,io-util` |
| `tokio-modbus` | 0.17.0 | Modbus RTU client | Only async Modbus RTU crate that integrates directly with tokio-serial; maintained (Dec 2024 release), uses tokio ^1.35, has RTU + sync variants. The `rtu` feature (default) is exactly what's needed |
| `tokio-serial` | 5.4.5 | Async serial port (tokio I/O) | tokio-modbus depends on `tokio-serial ^5.4.4`; wraps `mio-serial` and exposes a `tokio::io::AsyncRead/Write` stream; the only tokio-native serial crate |
| `reqwest` | 0.13.2 | HTTP client for InfluxDB writes | Async HTTP client, built on hyper/tokio; InfluxDB 3 writes are simple `POST /api/v3/write_lp` calls with line protocol body — reqwest handles auth headers, retries cleanly |
| `serde` | 1.0.228 | Config deserialization framework | Essential for `#[derive(Deserialize)]` on config structs; used by `toml` crate |
| `toml` | 1.1.1 | TOML config parsing | Project requirement; `toml::from_str::<T>()` with serde is the standard idiomatic pattern |
| `tracing` | 0.1.44 | Structured logging facade | Superior to `log`: structured fields, async-aware, spans for per-device context; integrates with `tracing-subscriber` for output routing |
| `tracing-subscriber` | 0.3.23 | Log output routing (console + file) | Provides `EnvFilter` (runtime log level control), `fmt` layer for human-readable output, and composes with `tracing-appender` for file output |
| `tracing-appender` | 0.2.4 | Non-blocking file log writer | Part of the tokio-rs/tracing family; provides `rolling::daily`/`rolling::never` file appender + non-blocking writer so file I/O doesn't block the async runtime |
| `anyhow` | 1.0.102 | Error handling | Ergonomic error propagation with context chaining; correct for a binary daemon (not a library) — avoids boilerplate `Box<dyn Error>` or custom error enums for glue code |

### Supporting Libraries

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `serde_derive` | (bundled in `serde` with `features = ["derive"]`) | `#[derive(Deserialize, Serialize)]` macros | Always — enable via `serde = { features = ["derive"] }` |
| `tokio-util` | 0.7.x | Codec framing utilities | Only if you drop `tokio-modbus` and implement the RTU framer manually (not recommended) |
| `thiserror` | 2.x | Typed error definitions | Use if you add a library layer or want typed errors at module boundaries; not needed for the top-level binary glue |

### Development Tools

| Tool | Purpose | Notes |
|------|---------|-------|
| `cargo cross` | Cross-compilation for ARM targets | Essential for building on x86 Mac/Linux and deploying to Raspberry Pi; `cross build --target aarch64-unknown-linux-gnu --release` or `armv7-unknown-linux-gnueabihf` for Pi 2/3 32-bit |
| `cargo clippy` | Lint enforcement | Run with `-- -D warnings` in CI to catch common Rust mistakes |
| `cargo-audit` | Dependency vulnerability scanning | Run before releases; tokio ecosystem is well-maintained but good hygiene |
| `systemd` service unit | Process supervision | No crate needed; write a `.service` file with `Restart=always`, `RestartSec=5`, logging via journald + tracing-appender file |

---

## Installation (`Cargo.toml` dependencies)

```toml
[dependencies]
# Async runtime
tokio = { version = "1", features = ["rt-multi-thread", "time", "signal", "macros"] }

# Modbus RTU over RS485
tokio-modbus = { version = "0.17", default-features = false, features = ["rtu"] }
tokio-serial = "5.4"

# InfluxDB 3 HTTP writes
reqwest = { version = "0.13", default-features = false, features = ["rustls-tls", "json"] }

# Config
serde = { version = "1", features = ["derive"] }
toml = "1"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
tracing-appender = "0.2"

# Error handling
anyhow = "1"

[profile.release]
opt-level = "z"   # Minimize binary size on Pi
strip = true
lto = true
```

---

## InfluxDB 3 Write API — Critical Details

**This differs significantly from v1/v2 — read carefully.**

| Aspect | InfluxDB v2 | InfluxDB v3 (use this) |
|--------|-------------|------------------------|
| Endpoint | `POST /api/v2/write?bucket=…&org=…` | `POST /api/v3/write_lp?db=<DATABASE>` |
| Auth header | `Authorization: Token <token>` | `Authorization: Bearer <token>` |
| Org parameter | Required (`org=…`) | **Not used** in v3 endpoint |
| Precision parameter | `?precision=ns` (required) | `?precision=<unit>` (optional; default `auto` detects from timestamp magnitude) |
| Success response | `HTTP 204` | `HTTP 204` |
| Content type | `text/plain` | Line protocol plaintext (same format) |

**Minimal write request (reqwest example):**
```rust
client
    .post(format!("{}/api/v3/write_lp", config.influxdb.url))
    .bearer_auth(&config.influxdb.token)
    .query(&[("db", &config.influxdb.database)])
    .body(line_protocol_string)
    .send()
    .await?;
```

**Line protocol format for PZEM-016:**
```
# measurement=device_name, no tags needed (already one measurement per device)
kitchen_panel voltage=230.5,current=12.3,power=2839.65,energy=1234.5,frequency=50.0,power_factor=0.98 1712000000000000000
```

---

## Alternatives Considered

| Recommended | Alternative | Why Not |
|-------------|-------------|---------|
| `tokio-modbus 0.17` | `rmodbus 0.12.2` | `rmodbus` is `no_std`/embedded-focused with manual frame handling — requires you to implement the serial framing loop yourself. Extra code for no benefit on Linux |
| `tokio-modbus 0.17` | Custom RTU over `tokio-serial` | Re-inventing what `tokio-modbus` already does correctly (CRC, frame timeouts, retry logic) |
| `reqwest 0.13` | `influxdb3 0.2.0` crate | 210 total downloads, GitLab-hosted, community crate with no InfluxData backing. InfluxData's official Rust client library **does not exist yet** (only Go, Python, Java, JS, C# listed in official docs). Raw `reqwest` with 3 lines of code is safer and has zero dependency risk |
| `reqwest 0.13` | `influxdb2` crate (v2 client) | Targets v2 API — wrong endpoint, wrong auth format, wrong org/bucket semantics |
| `tracing` | `log` + `env_logger` | `log` has no structured fields and no async spans. `tracing` is the 2025 standard for async Rust daemons |
| `toml` direct | `config` crate | `config` crate adds complexity (layered configs, env overrides) that the project explicitly out-of-scopes; `toml::from_str` + serde is 10 lines and zero magic |
| `anyhow` | `thiserror` | `thiserror` is for libraries that expose typed errors to callers. A daemon binary benefits from `anyhow` context chains in logs, not typed error variants |
| `tokio-serial 5.4.5` | `serialport 4.9.0` (sync) | `serialport` is synchronous; wrapping it in `spawn_blocking` inside tokio works but is clunky. `tokio-serial` is the tokio-native wrapper that `tokio-modbus` expects |

---

## What NOT to Use

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| `influxdb` crate (v1 client) | Targets InfluxDB 1.x line protocol endpoint `/write` — incompatible with v3 API routes and auth | `reqwest` with `POST /api/v3/write_lp` |
| `influxdb2` crate | Targets `/api/v2/write` with `Token` header and `org=` param — both wrong for v3 | `reqwest` with `Bearer` header and `db=` param |
| `rmodbus` for Linux daemon | Designed for `no_std` embedded; requires manual byte buffer management on Linux | `tokio-modbus` |
| `async-std` runtime | Incompatible with tokio-modbus and reqwest (both require tokio); mixing runtimes causes panics | `tokio` |
| `serialport 4.x` (sync) | Forces `spawn_blocking` wrappers; `tokio-modbus` does not accept it as a transport | `tokio-serial 5.4.5` |
| `log` + `env_logger` | No structured fields, no spans, no async context — insufficient for a multi-device polling daemon where per-device error context matters | `tracing` + `tracing-subscriber` |
| `config` crate | Over-engineered for TOML-only config; adds implicit env-var behavior that project explicitly excludes | `toml::from_str` + `serde` |

---

## Stack Patterns by Variant

**If deploying to 32-bit Pi (Pi 2/3 without 64-bit OS):**
- Cross-compile target: `armv7-unknown-linux-gnueabihf`
- Add `reqwest` with `rustls-tls` (not `native-tls`) — avoids OpenSSL system library dependency during cross-compilation

**If deploying to 64-bit Pi OS (Pi 3B+/4/5 with arm64):**
- Cross-compile target: `aarch64-unknown-linux-gnu`
- Both `rustls-tls` and `native-tls` work; prefer `rustls-tls` for static linking simplicity

**If compiling directly on the Pi (slow but zero cross-compile setup):**
- Install Rust via `rustup` on the Pi
- `cargo build --release` works natively; expect 5-15 min compile time for first build

**If InfluxDB instance uses self-signed TLS:**
- Add `.danger_accept_invalid_certs(true)` to `reqwest::ClientBuilder` OR
- Bundle the CA cert and use `.add_root_certificate(cert)` — prefer the latter for production

---

## Version Compatibility

| Package | Compatible With | Notes |
|---------|-----------------|-------|
| `tokio-modbus 0.17` | `tokio ^1.35`, `tokio-serial ^5.4.4` | Verified from docs.rs dependency list |
| `tokio-serial 5.4.5` | `tokio 1.x`, `mio-serial` | Last updated Dec 2024; 5.4.5 is latest stable |
| `reqwest 0.13` | `tokio 1.x`, `hyper 1.x` | Reqwest 0.13 moved to hyper 1.x — major internal change from 0.11/0.12; do not mix with reqwest 0.11 middleware |
| `tracing-subscriber 0.3` | `tracing 0.1` | Same major version family; all 0.1.x tracing crates are compatible |
| `tracing-appender 0.2.4` | `tracing-subscriber 0.3` | Compose via `tracing_subscriber::registry().with(layer)` |
| `toml 1.x` | `serde 1.x` | `toml` 1.x (spec 1.1.0) is a major rewrite from 0.7; use `1` not `0.8` |
| `serde 1.0.228` | Universal | Stable semver; no breaking changes since 1.0 |

---

## Sources

- crates.io API — `tokio-serial` max_stable_version: `5.4.5` (updated 2024-12-31)
- crates.io API — `tokio-modbus` default_version: `0.17.0`
- docs.rs tokio-modbus 0.17.0 — feature flags (`rtu`, `tcp`), dependency list (tokio-serial ^5.4.4 confirmed)
- crates.io API — `serialport` max_stable_version: `4.9.0` (updated 2026-03-16)
- crates.io API — `reqwest` default_version: `0.13.2`
- crates.io API — `tokio` default_version: `1.50.0`
- crates.io API — `serde` default_version: `1.0.228`
- crates.io API — `toml` default_version: `1.1.1+spec-1.1.0`
- crates.io API — `tracing` default_version: `0.1.44`
- crates.io API — `tracing-subscriber` default_version: `0.3.23`
- crates.io API — `tracing-appender` default_version: `0.2.4`
- crates.io API — `anyhow` default_version: `1.0.102`
- crates.io API — `rmodbus` default_version: `0.12.2`
- crates.io API — `influxdb3` default_version: `0.2.0` (community crate, 210 total downloads — LOW confidence for production use)
- **Official InfluxDB 3 docs** — `POST /api/v3/write_lp`, `Authorization: Bearer`, `?db=` parameter, `HTTP 204` success — HIGH confidence — https://docs.influxdata.com/influxdb3/core/write-data/http-api/v3-write-lp/
- **Official InfluxDB 3 client library list** — No official Rust v3 client exists; Go/Python/Java/JS/C# only — https://docs.influxdata.com/influxdb3/core/reference/client-libraries/v3/

---
*Stack research for: Rust RS485/Modbus RTU → InfluxDB 3 daemon on Raspberry Pi*
*Researched: 2026-04-02*

# Feature Research

**Domain:** Modbus RTU RS485 data logger daemon (embedded Linux / Raspberry Pi → InfluxDB)
**Researched:** 2026-04-02
**Confidence:** HIGH

---

## Feature Landscape

### Table Stakes (Users Expect These)

Features that a production-quality Modbus data logger _must_ have. Missing any of these means the tool is broken or unreliable.

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Configurable serial port path + baud + parity | The device path (`/dev/ttyUSB0`) changes between hosts; 9600/8N1 is PZEM-016 default but must be overridable | LOW | TOML `[serial]` section; standard `tokio-serial` / `serialport` crate params |
| TOML config with device list (address + name) | Operators define how many devices exist and name them; not hardcoded | LOW | Serde deserialize into `Vec<DeviceConfig>` |
| Poll all PZEM-016 registers (voltage, current, power, energy, frequency, power factor) via FC 0x04 | All 6 fields are the product's entire value proposition | MEDIUM | Single FC04 call per device reads all 10 registers at once (0x0000–0x0009) |
| Sequential polling across all devices per interval | RS485 is a shared bus — only one master transaction at a time; concurrent reads corrupt responses | LOW | `tokio::time::interval` tick → sequential `for device in devices` loop |
| Global polling interval (seconds) in TOML | Operators must be able to change poll cadence without recompiling | LOW | `interval_secs: u64` in TOML |
| Skip failed device, log error, continue poll cycle | A failed PZEM-016 (powered off, wrong address) must not stall the entire loop | LOW | `Result::err` path → `tracing::warn!` → continue; this is the stated core reliability requirement |
| Write to InfluxDB 3 via HTTP line protocol (`/api/v3/write_lp`) | InfluxDB 3 write endpoint; `Authorization: Bearer <token>` header | MEDIUM | `reqwest` async HTTP client; line format: `<device_name> voltage=229.6,current=0.10,...` |
| Per-device InfluxDB measurement name (= device name from config) | Allows per-device queries without tag filtering | LOW | Measurement name = device name string; one write call per device per poll cycle |
| Structured logging to stderr/stdout (INFO by default) | systemd captures stdout/stderr via `journald`; operators need to see what's happening | LOW | `tracing` + `tracing-subscriber` with `EnvFilter`; level from config or `RUST_LOG` |
| Optional log to file (configurable path) | Pi operators often want a persistent log file for post-mortem; journald may be large | LOW | `tracing-appender` rolling file or simple file writer; configurable path + level |
| Graceful shutdown on SIGTERM / SIGINT | systemd sends SIGTERM before killing; clean shutdown avoids corrupted line-protocol mid-write | LOW | `tokio::signal::unix::signal(SIGTERM)` + `select!`; complete current poll cycle, flush, exit |
| Systemd service unit file | Operators use `systemctl start/stop/enable`; auto-restart on crash is mandatory for 24/7 operation | LOW | `[Service] Type=simple Restart=always RestartSec=5`; included in repo |
| Config file validation at startup with clear error messages | Mistyped device address or bad URL should fail fast with a human-readable message, not a cryptic panic | LOW | `serde` deserialization errors surfaced as `anyhow`/`thiserror` with context |
| InfluxDB write failure: log error and continue polling | Network blip or InfluxDB restart must not crash the daemon or stall device polling | LOW | HTTP error → `tracing::error!` → continue; data from the failed write is discarded (no gap in future polls) |
| USB device path stability after reboot | `/dev/ttyUSB0` numbering is arbitrary; production setups must use stable paths | LOW | Document using `udev` rule to create `/dev/rs485-pzem` symlink; not a code feature, but a deployment requirement to document |

---

### Differentiators (Competitive Advantage)

Features not expected in minimal Modbus loggers but that make this daemon production-quality.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| In-memory write-failure buffer with bounded retry | InfluxDB briefly unavailable (restart, network) → buffer N failed batches in memory → retry on next successful cycle; prevents gaps in data | MEDIUM | Ring buffer (`VecDeque`) of `(timestamp, line_protocol_batch)` with configurable max size; flush oldest first on recovery. **Do not use disk** — adds complexity and SD card wear |
| InfluxDB connectivity check at startup | Fail fast with clear error if InfluxDB is unreachable at launch rather than silently losing data | LOW | One test write or `HEAD /ping` on startup before entering poll loop |
| Per-device consecutive error counter in logs | Distinguish "device flapped once" from "device has been offline for 50 cycles" without querying InfluxDB | LOW | `HashMap<DeviceId, u32>` error count; log escalation at thresholds (e.g., warn at 3, error at 10) |
| Configurable Modbus RTU timeout per-device-poll | PZEM-016 needs ~100ms for response; too-short timeout causes false errors, too-long stalls the bus | LOW | `timeout_ms: u64` in TOML `[serial]`; default 500ms; passed to `tokio_modbus` read call |
| Startup summary log line | On boot, emit one INFO log showing: serial port, baud rate, device count, InfluxDB URL, poll interval | LOW | Confirms config was parsed correctly at a glance via `journalctl` |
| Config file path as CLI argument | Allows running multiple instances (unusual but possible with multiple USB adapters) or non-default config location | LOW | `clap` with `--config <path>`; default `/etc/rs485-logger/config.toml` |
| Log rotation for file appender | On a Pi running 24/7 for months, unbounded log files fill the SD card | LOW | `tracing-appender` rolling daily; or document `logrotate` config |
| Explicit InfluxDB 3 database/table model in docs | InfluxDB 3 uses "database + table" not "org + bucket + measurement" as in v1/v2; this trips up users | LOW | In README and example config, explain the v3 API's `?db=` parameter mapping |

---

### Anti-Features (Commonly Requested, Often Problematic)

Features to deliberately _not_ build in v1.

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| Per-device polling intervals | "Device X is slow, Device Y needs faster data" | Turns a simple `for` loop into a scheduler; adds concurrency to a serial bus that is fundamentally single-threaded; all PZEM-016 respond in <200ms anyway | Single global interval is sufficient; a 5-second interval is fine for power monitoring |
| Disk-persistent write buffer (SQLite/CSV fallback) | "Don't lose data when InfluxDB is down for hours" | Adds significant complexity; SD card write amplification is a real failure mode on Pi; InfluxDB on the same LAN almost never goes down for >minutes | Bounded in-memory ring buffer handles brief outages; long outages mean accepting data gaps |
| Hot-reload of config file (SIGHUP) | "Change device list without restart" | Config affects serial port setup and device state; partial reload is risky; PZEM-016 setups rarely change | Just restart the daemon (`systemctl restart rs485-logger`); it starts in <1 second |
| Modbus TCP support | "I want to use this with a TCP gateway" | Different framing, different library code path, doubles test surface; out of stated scope | Fork or use a Modbus TCP tool; this is RTU-only |
| Web UI / REST API for status | "I want to see device status in a browser" | Grafana connected to InfluxDB already does this; adding a web server increases binary size and attack surface | Use InfluxDB + Grafana; expose device status via a periodic health measurement in InfluxDB itself |
| Alerting / threshold notifications | "Alert me when voltage drops below 200V" | Complex to do well (hysteresis, dedup, notification channels); InfluxDB 3 processing engine or Grafana alerting already provides this | Use InfluxDB 3's built-in threshold alerting or Grafana alerting rules |
| OAuth / env-var / secret-store credential sourcing | "Don't put the token in the config file" | Over-engineering for a single-device Pi setup; token in a file with `chmod 600` is fine | Document `chmod 600 /etc/rs485-logger/config.toml`; v2 can add env-var override |
| One-shot / cron mode | "Run on demand, not continuously" | Adds a second code path and startup/shutdown overhead; daemon with systemd timer would be redundant | Run as daemon; for ad-hoc reads, use `mbpoll` or `modpoll` CLI tool directly |
| Auto-discovery of PZEM-016 devices | "Scan the bus and find all devices automatically" | RS485 bus scan requires iterating all 247 addresses; takes minutes; PZEM-016 needs a connected load to respond; fragile on production systems | Explicit device list in config; provide docs on how to find/set PZEM addresses with `mbpoll` |

---

## Feature Dependencies

```
[Serial port config (path, baud, parity)]
    └──required by──> [Modbus RTU client initialization]
                          └──required by──> [Per-device polling (FC04 read)]
                                                └──required by──> [InfluxDB write]

[TOML config (device list, interval, InfluxDB URL/token/db)]
    └──required by──> [All runtime behavior]

[Per-device polling]
    └──required by──> [Skip-and-continue error handling]
    └──required by──> [Per-device error counter] (differentiator)

[InfluxDB write]
    └──required by──> [Write-failure in-memory buffer] (differentiator)
    └──required by──> [Startup connectivity check] (differentiator)

[Graceful shutdown (SIGTERM)]
    └──enhances──> [InfluxDB write] (flush in-flight batch before exit)

[Systemd service unit]
    └──enhances──> [Graceful shutdown] (systemd sends SIGTERM before SIGKILL)

[Structured logging to stdout/stderr]
    └──enhances──> [Systemd service unit] (journald captures stdout)

[Log to file] ──optional──> [Log rotation]
```

### Dependency Notes

- **Serial port config requires Modbus client:** `tokio-modbus` RTU client takes a `tokio-serial` port built from the config's path + baud settings.
- **TOML config required by all runtime behavior:** Everything derives from the parsed config; startup validation failure must prevent the poll loop from starting.
- **Graceful shutdown enhances InfluxDB write:** On SIGTERM, complete the current in-progress poll cycle and flush any buffered writes before exiting — avoids partial line-protocol payloads mid-HTTP-request.
- **Write-failure buffer requires InfluxDB write:** The buffer only makes sense if there is a write target; it holds `(timestamp, batch)` tuples for retry.
- **Log-to-file is independent of stdout logging:** Both can operate simultaneously; `tracing-subscriber` supports multiple layers.

---

## MVP Definition

### Launch With (v1)

Minimum viable product that fulfills the stated core value: *"Reliable, continuous power data from every PZEM-016 flowing into InfluxDB without data gaps."*

- [x] **TOML config** — device list (address + name), serial port, poll interval, InfluxDB endpoint + token + db — because nothing works without it
- [x] **Modbus RTU serial initialization** — open port at configured baud/parity — foundation for all reads
- [x] **FC04 register read for all 6 PZEM-016 fields** per device — the entire point of the daemon
- [x] **Sequential polling loop** — tick on interval, read each device in order — core collection behavior
- [x] **Skip-and-log on device error** — error counted, message logged, loop continues — directly required by PROJECT.md
- [x] **InfluxDB 3 write via `/api/v3/write_lp`** with `Authorization: Bearer` — per-device measurement name = device name
- [x] **InfluxDB write failure: log error and continue** — network blip must not crash the daemon
- [x] **Graceful shutdown on SIGTERM/SIGINT** — systemd needs this; completes current cycle before exiting
- [x] **Structured log to stdout** + optional **log to file** — configurable path and level; systemd/journald integration
- [x] **Config validation at startup** — clear error messages before entering poll loop
- [x] **Systemd `.service` unit file** — in repo; `Restart=always`

### Add After Validation (v1.x)

Features to add once the core loop is proven stable over days/weeks of real operation.

- [ ] **In-memory write-failure buffer with retry** — triggered when InfluxDB goes offline for minutes; first validate that the core loop is reliable before adding buffering complexity
- [ ] **Startup InfluxDB connectivity check** — good DX addition; add after verifying write path is solid
- [ ] **Per-device consecutive error counter** — useful for dashboards once live; adds observability after basic data flow is confirmed
- [ ] **CLI `--config` argument** — default path is fine for v1; add when second deployment is needed

### Future Consideration (v2+)

Features to defer until the daemon has been in production and real needs are validated.

- [ ] **Log rotation config** — document `logrotate` approach first; build-in only if that proves insufficient
- [ ] **Configurable Modbus read timeout per-device** — PZEM-016 always responds within 200ms; only needed if devices with different timing are added
- [ ] **Multiple serial port support** — only relevant when hardware scales beyond one USB adapter
- [ ] **Cross-compilation / CI release pipeline** — useful for distribution; not needed for single-Pi deployment

---

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| TOML config (devices, serial, InfluxDB) | HIGH | LOW | P1 |
| FC04 register read (all 6 PZEM-016 fields) | HIGH | MEDIUM | P1 |
| Sequential polling loop with global interval | HIGH | LOW | P1 |
| Skip-and-log on device error | HIGH | LOW | P1 |
| InfluxDB 3 write (line protocol, per-device measurement) | HIGH | MEDIUM | P1 |
| InfluxDB write failure: log + continue | HIGH | LOW | P1 |
| Graceful SIGTERM/SIGINT shutdown | HIGH | LOW | P1 |
| Structured logging to stdout + optional file | HIGH | LOW | P1 |
| Startup config validation | HIGH | LOW | P1 |
| Systemd service unit | HIGH | LOW | P1 |
| In-memory write-failure buffer + retry | MEDIUM | MEDIUM | P2 |
| Startup InfluxDB connectivity check | MEDIUM | LOW | P2 |
| Per-device consecutive error counter | MEDIUM | LOW | P2 |
| CLI `--config` argument | LOW | LOW | P2 |
| Log rotation | LOW | LOW | P3 |
| Configurable per-device Modbus timeout | LOW | LOW | P3 |

**Priority key:**
- P1: Must have for launch
- P2: Should have, add when possible
- P3: Nice to have, future consideration

---

## Competitor Feature Analysis

*Note: This is a narrow embedded Rust daemon, not a commercial product. The relevant "competitors" are existing open-source Modbus-to-InfluxDB loggers and Python/Node.js scripts.*

| Feature | Typical Python/Node script | PyScada (full SCADA) | Our Approach |
|---------|--------------------------|----------------------|--------------|
| Config format | Hardcoded or `.env` | Django admin UI + DB | TOML file — human-editable, version-controllable |
| Error handling | Script crashes on first error | Daemon with retry | Skip-and-continue: daemon never dies due to one device |
| Resource usage on Pi | ~50MB RAM (Python runtime) | ~200MB+ (Django + DB) | ~2–5MB RAM (Rust static binary) |
| InfluxDB 3 support | Rare (most target v1/v2 API) | No v3 native support | Native v3 `/api/v3/write_lp` endpoint |
| Systemd integration | `ExecStart=python3 ...` with no restart | Complex multi-process | Single binary, `Restart=always`, `Type=simple` |
| Deployment | Copy script + pip install | Full SCADA install | Single cross-compiled binary + one config file |
| Bus safety | Often uses threads, risks collision | Worker process per device | Single async task, sequential per-device reads |

---

## Sources

- PZEM-004T v3.0 register map and multidevice usage: [github.com/mandulaj/PZEM-004T-v30](https://github.com/mandulaj/PZEM-004T-v30) — HIGH confidence
- InfluxDB 3 `/api/v3/write_lp` endpoint, auth, line protocol: [docs.influxdata.com/influxdb3/core/write-data/http-api/v3-write-lp/](https://docs.influxdata.com/influxdb3/core/write-data/http-api/v3-write-lp/) — HIGH confidence
- InfluxDB 3 write best practices (batch, precision, gzip): [docs.influxdata.com/influxdb3/core/write-data/best-practices/optimize-writes/](https://docs.influxdata.com/influxdb3/core/write-data/best-practices/optimize-writes/) — HIGH confidence
- tokio-modbus 0.17.0 RTU client API: [docs.rs/tokio-modbus](https://docs.rs/tokio-modbus/latest/tokio_modbus/) — HIGH confidence
- tracing + tracing-subscriber logging ecosystem: [docs.rs/tracing](https://docs.rs/tracing/latest/tracing/) — HIGH confidence
- PyScada SCADA (competitor feature comparison): [github.com/pyscada/PyScada](https://github.com/pyscada/PyScada) — MEDIUM confidence
- Modbus RTU RS485 logger ecosystem survey: GitHub Topics modbus-rtu (379 repos), modbus-logger (2 repos) — MEDIUM confidence
- PZEM-016 register layout and Modbus addressing: training data (matches PZEM-004T v3.0 datasheet patterns) — MEDIUM confidence (should be verified against physical hardware in Phase 1)

---

*Feature research for: RS485 Modbus RTU PZEM-016 data logger daemon (Rust, Raspberry Pi, InfluxDB 3)*
*Researched: 2026-04-02*

# Pitfalls Research

**Domain:** Rust Modbus RTU data logger — PZEM-016 + RS485 on Raspberry Pi → InfluxDB 3
**Researched:** 2026-04-02
**Confidence:** HIGH (PZEM register layout: ESPHome source + community verified; InfluxDB: official docs; Rust serial/Modbus: crate changelogs + docs; cross-compilation: Cargo official docs)

---

## Critical Pitfalls

### Pitfall 1: PZEM-016 Energy Register Is 32-bit Split Across Two 16-bit Words — Word Order Is Low-High, Not High-Low

**What goes wrong:**
You read registers 0x0003–0x0004 (energy) and naively concatenate them as `(reg[0] << 16) | reg[1]`, giving a wildly wrong kWh value. The PZEM-016 stores the 32-bit energy accumulator as `(high_word << 16) | low_word`, but the two 16-bit Modbus registers arrive as `[low_word, high_word]` — so the correct reconstruction is `(raw[1] as u32) << 16 | (raw[0] as u32)`.

**Why it happens:**
The PZEM datasheet is ambiguous and the register description just says "two registers." The Modbus spec says multi-register values are big-endian (high word first), but PZEM-016 deviates and sends low word first. ESPHome's `pzemac.cpp` source (the most battle-tested reference implementation) confirms: `pzem_get_32bit(i)` does `(pzem_get_16bit(i+2) << 16) | pzem_get_16bit(i+0)` — register `i+0` is low word, `i+2` is high word.

Similarly, current is a 32-bit value at registers 0x0001–0x0002, and active power at 0x0003–0x0005 — all using the same low-high word order.

**How to avoid:**
Use the verified register map from ESPHome `pzemac.cpp`:
```
Offset  Register  Size   Decode
0       0x0000    16-bit voltage = raw / 10.0  (max 6553.5 V)
2       0x0001    32-bit current = (reg[3]<<16|reg[2]) / 1000.0  (low word first)
6       0x0003    32-bit power   = (reg[7]<<16|reg[6]) / 10.0    (low word first)
10      0x0005    32-bit energy  = (reg[11]<<16|reg[10])          (Wh, raw integer)
14      0x0007    16-bit frequency = raw / 10.0
16      0x0008    16-bit power_factor = raw / 100.0
```
Read **10 registers** starting at address 0x0000 (function code 0x04). Request exactly `read_input_registers(addr, 0x0000, 10)`.

**Warning signs:**
- Current, power, or energy values are factor-of-65536 off
- Energy jumps discontinuously (e.g., 1 Wh → 65537 Wh on rollover)
- Integration tests against a real device show non-physical values

**Phase to address:** Serial + Modbus polling phase (Phase 1 / core Modbus integration)

---

### Pitfall 2: PZEM-016 Address Register Uses Non-Standard Function Code 0x06 — Not the Standard Modbus Address Coil

**What goes wrong:**
You try to broadcast to all devices on the bus (address 0xF8) or change/read the stored Modbus address using standard function codes and it silently fails or corrupts the bus. PZEM-016 uses a proprietary function code 0x06 (Write Single Register) to read/write its address register at 0x0002. The "broadcast address" for PZEM is 0xF8, not 0x00 (the Modbus standard).

**Why it happens:**
The PZEM "Modbus" implementation is a simplified subset. It only responds to:
- Function code `0x04` (Read Input Registers) for measurements
- Function code `0x06` (Write Single Register) for the address register at 0x0002
- Function code `0x42` (proprietary) for energy reset

Using `tokio-modbus`'s standard `read_holding_registers` (FC 0x03) will return an exception or no response.

**How to avoid:**
- Only use `read_input_registers` (FC 0x04) to poll measurement data
- Do NOT use holding register reads (FC 0x03) — PZEM won't respond
- Address assignment is a one-time hardware setup step, not a runtime daemon concern
- Use Modbus address 1–16 for each device as configured on the hardware

**Warning signs:**
- `tokio-modbus` returns `ExceptionCode::IllegalFunction` or timeout for every device
- All devices respond identically (accidentally sending to broadcast address 0xF8)

**Phase to address:** Phase 1 (Modbus RTU polling implementation)

---

### Pitfall 3: Insufficient Inter-Request Delay Causes Bus Collisions on Multi-Device Daisy Chain

**What goes wrong:**
You poll multiple PZEM-016 units back-to-back in a tight async loop. Device N is still transmitting its response when you start sending the request to device N+1. The RS485 bus enters an undefined state: garbled bytes, partial frames, CRC errors on subsequent responses, and the entire polling cycle for that interval is corrupted.

**Why it happens:**
RS485 is half-duplex. The USB-RS485 adapter must switch from RX mode to TX mode (RTS/DE signal toggle). Many cheap CH340/CP2102-based adapters have 1–5 ms hardware latency for this direction switch. The PZEM-016 response for 10 registers is ~25 bytes at 9600 baud ≈ 26 ms. If you send the next request before that window fully clears, you collide.

Additionally, `tokio-modbus` v0.8.2 added "clear rx buffer before sending" as a fix for exactly this scenario (see changelog), indicating this is a known real-world problem.

**How to avoid:**
- Add an explicit inter-request delay of **≥ 50 ms** between device polls (100 ms is safe margin)
- Configure a **per-request read timeout of ≥ 500 ms** in `tokio-modbus` to handle slow devices
- Sequence all device polls **serially** (not concurrently) — RS485 is a single shared bus
- Do NOT use `tokio::join!` or `FuturesUnordered` to poll devices in parallel

**Warning signs:**
- Increasing CRC error rate when more devices are added
- Intermittent `Err(TimedOut)` responses that correlate with bus load
- Works with 1 device, fails with 3+

**Phase to address:** Phase 1 (Modbus RTU polling) and Phase 3 (multi-device polling loop)

---

### Pitfall 4: USB-RS485 Adapter Device Path Is Not Stable Across Reboots

**What goes wrong:**
You hardcode `/dev/ttyUSB0` in the config. After a Pi reboot, or after plugging in a second USB device, the adapter appears as `/dev/ttyUSB1` and the daemon fails to open the port with `ENOENT` or `EACCES` but nothing in the logs makes this obvious.

**Why it happens:**
Linux assigns `/dev/ttyUSBn` in USB enumeration order, which is non-deterministic if other USB-serial devices are present (keyboard hubs, another adapter, etc.). CH340 and CP2102 both use `ttyUSB` — no stable naming.

**How to avoid:**
- Use a **udev rule** to create a stable symlink based on USB VID/PID and serial number:
  ```
  # /etc/udev/rules.d/99-rs485.rules
  SUBSYSTEM=="tty", ATTRS{idVendor}=="1a86", ATTRS{idProduct}=="7523", \
    SYMLINK+="ttyRS485"
  ```
- Configure the daemon to use `/dev/ttyRS485` (the symlink)
- Document the udev rule as part of deployment
- For multi-adapter setups, use `ATTRS{serial}` to distinguish

**Warning signs:**
- Daemon starts successfully in dev environment, fails on fresh Pi deployment
- `systemctl status rs485-logger` shows `Error opening serial port: No such file or directory`
- Works after `sudo systemctl restart` if adapter enumeration happens to land back on ttyUSB0

**Phase to address:** Phase 4 (systemd deployment and hardening)

---

### Pitfall 5: Serial Port Permissions — Non-root Daemon Silently Denied Access

**What goes wrong:**
The systemd service runs as a dedicated non-root user (correct for security) but the user is not in the `dialout` group. Opening `/dev/ttyUSB0` returns `EACCES`. The daemon crashes at startup and systemd keeps restarting it — burning restart budget and filling logs.

**Why it happens:**
On Raspberry Pi OS (Debian-based), `/dev/ttyUSB*` are owned by `root:dialout` with mode `0660`. Non-root users must be in the `dialout` group (or the udev rule must set permissions).

**How to avoid:**
- Add the service user to the `dialout` group: `usermod -aG dialout rs485-logger`
- OR use a udev rule to set `MODE="0660", GROUP="rs485-logger"` for the specific device
- Verify with `ls -la /dev/ttyUSB*` during setup
- Add to deployment documentation / install script

**Warning signs:**
- `Permission denied` in daemon logs at startup
- `journalctl -u rs485-logger` shows immediate crash loop
- Works when running as root but fails under systemd unit

**Phase to address:** Phase 4 (systemd deployment)

---

### Pitfall 6: InfluxDB 3 Field Type Is Locked on First Write — Mixed Integer/Float Writes Cause Silent Data Loss

**What goes wrong:**
In one polling cycle you write `power=0i` (integer, because power is 0W), then in the next cycle you write `power=45.2` (float). InfluxDB 3 rejects all subsequent writes to that field because it expects the type established at schema creation time. The write API returns HTTP 400, your error handler logs it, but you continue polling — dropping data silently.

**Why it happens:**
InfluxDB 3 (IOx-based) uses an immutable column schema. The first write to a measurement establishes the data type for each field. The PZEM-016 returns raw 16-bit integers from the hardware but they represent physical quantities (voltage in 0.1V units, power factor in 0.01 units). If you conditionally use integer vs. float formatting based on whether the decimal part is zero, you create a type instability.

**How to avoid:**
- **Always write all numeric fields as `f64` floats** in line protocol (no `i` suffix)
- Apply scaling (e.g., `raw / 10.0`) before writing — never write raw register values
- Line protocol: `voltage=231.5,current=1.234,power=285.0,energy=1024.0,frequency=50.0,power_factor=0.95`
- Keep a schema document noting each field is always float

**Warning signs:**
- HTTP 422 or 400 responses from InfluxDB write endpoint with "field type conflict" in body
- Data gaps in Grafana that correlate with power=0 (common at night for solar monitoring)
- InfluxDB logs show `ERR: column type conflict`

**Phase to address:** Phase 2 (InfluxDB write integration)

---

### Pitfall 7: InfluxDB 3 API Endpoint Differs From v1/v2 — Using Wrong Write Path Gives 404 or Silent Drops

**What goes wrong:**
You target `/write?db=...` (v1 API) or `/api/v2/write` (v2 API). InfluxDB 3 Core/Enterprise responds with 404 or returns success without actually storing data, depending on whether the v1 compatibility layer is enabled. Data appears to be written but queries return no results.

**Why it happens:**
InfluxDB 3 is a completely rewritten storage engine (Apache Arrow/DataFusion). The write API endpoint is `/api/v3/write_lp` for the native v3 API, or `/api/v2/write` if the v2 compatibility shim is enabled. The v1 `/write` endpoint exists only on explicitly configured v1-compat setups. Many examples online still show v1/v2 paths.

**How to avoid:**
- Use the **native v3 write endpoint**: `POST /api/v3/write_lp?db=<bucket>`
- Authentication header: `Authorization: Bearer <token>`
- Content-Type: `text/plain; charset=utf-8`
- Verify the InfluxDB 3 instance version before development begins
- Test with `curl` first: `curl -X POST "http://host:8086/api/v3/write_lp?db=power" -H "Authorization: Bearer TOKEN" --data-raw "test_measurement field=1.0"`

**Warning signs:**
- HTTP 404 from write endpoint
- HTTP 200 but data never appears in queries
- `influx3 query "SELECT * FROM ..."` returns empty results despite writes "succeeding"

**Phase to address:** Phase 2 (InfluxDB integration)

---

### Pitfall 8: Energy Register Rollover Creates Negative Delta or Data Spike in InfluxDB

**What goes wrong:**
The PZEM-016 energy counter is a 32-bit unsigned integer in Wh, maxing out at 4,294,967,295 Wh (≈ 4.3 GWh). In high-consumption installations this wraps to 0. Grafana's "non-negative derivative" shows a massive negative spike; alerts fire; cumulative energy charts are permanently corrupted.

More practically: **the energy register also resets to 0 when power is cut**. Any power outage resets it. This is the much more common rollover scenario in residential/commercial use.

**Why it happens:**
The PZEM-016 stores energy in EEPROM (flash) with limited write cycles, and hardware designs typically reset the register on power loss. The Modbus register just reads whatever is currently stored — it provides no "total lifetime" semantics.

**How to avoid:**
- Write the **raw energy value** to InfluxDB as a gauge, not as a monotone counter
- Track a `last_energy` value in the daemon and detect rollovers: if `new_value < last_value` → reset detected → log warning, still write new value
- In InfluxDB/Grafana, use `non_negative_difference()` or `non_negative_derivative()` to compute energy delta per interval
- Log reset events at `WARN` level with timestamps for auditability

**Warning signs:**
- Energy value drops to 0 in dashboard after a power event
- Negative energy delta in derived calculations
- Energy counter jumps backward when a PZEM is replaced with a new unit

**Phase to address:** Phase 1 (Modbus polling — add reset detection) and Phase 3 (multi-device resilience)

---

### Pitfall 9: Tokio Async Runtime + tokio-serial: Opening Serial Port Outside Runtime Context Panics

**What goes wrong:**
You call `tokio_serial::SerialStream::open(...)` or `tokio_modbus::rtu::connect_slave(...)` in the synchronous `main()` body before calling `tokio::main` or inside a `std::thread::spawn`. The runtime is not active and the call panics with "no reactor running" or produces a port that immediately returns `WouldBlock` on every operation.

**Why it happens:**
`tokio-serial` requires an active Tokio runtime to register the file descriptor with the reactor. The sync `SerialPortBuilder::open()` approach in `tokio-modbus` v0.8.0+ replaced the async `connect()` with synchronous `attach()`, but the `SerialStream` itself must still be created within a runtime context (or via `tokio::task::block_in_place` in a sync context).

**How to avoid:**
- Use `#[tokio::main]` on `main()` — create the serial port **inside** the async context
- Use `tokio_modbus::rtu::attach_slave(serial_builder, slave)` (the v0.8+ API), not the old `connect()` API
- Never store `SerialStream` in a struct that is initialized before the runtime starts

**Warning signs:**
- `thread 'main' panicked at 'there is no reactor running'`
- Port opens without error but every `read_input_registers` call returns `WouldBlock` immediately
- Works in `tokio::test` but panics in `main()`

**Phase to address:** Phase 1 (Modbus RTU async setup)

---

### Pitfall 10: Cross-Compilation Fails Due to Missing `libudev` or OpenSSL — Linker Errors for ARM Targets

**What goes wrong:**
You set up `cargo build --target aarch64-unknown-linux-gnu` and hit linker errors: `cannot find -ludev` or `cannot find -lssl`. The build fails even though the Rust code itself is fine. Alternatively, you add `serialport = "4"` which transitively depends on `libudev` on Linux, and the cross linker can't find the ARM version.

**Why it happens:**
`tokio-serial` → `serialport` → `libudev` (on Linux, for USB hotplug detection). Cross-compilation requires either:
1. ARM sysroot with `libudev-dev:arm64` installed, or
2. Disabling `libudev` with `serialport = { features = [], default-features = false }` (disables USB port listing — acceptable since we use a static `/dev/ttyRS485` path)

**How to avoid:**
- Add to `Cargo.toml`:
  ```toml
  [dependencies]
  serialport = { version = "4", default-features = false }
  # This disables libudev dependency — we use a fixed device path, not USB discovery
  ```
- For cross-compilation: use `cross` (the Rust cross-compilation tool) which handles sysroots automatically:
  ```bash
  cargo install cross
  cross build --release --target aarch64-unknown-linux-gnu
  ```
- OR compile natively on the Pi (slow but zero cross-compilation complexity)
- Add `.cargo/config.toml` with the target linker:
  ```toml
  [target.aarch64-unknown-linux-gnu]
  linker = "aarch64-linux-gnu-gcc"
  ```

**Warning signs:**
- `error: linking with 'aarch64-linux-gnu-gcc' failed: exit status: 1`
- `cannot find -ludev` in linker output
- Build succeeds on x86 host but fails when adding `--target armv7-unknown-linux-gnueabihf`

**Phase to address:** Phase 4 (deployment / build pipeline)

---

### Pitfall 11: systemd Service Hardening Breaks Serial Port Access

**What goes wrong:**
You copy a hardened systemd service template that includes `PrivateDevices=true` or `DeviceAllow=` restrictions. The service fails to start with `Permission denied` opening the serial port — even though the user is in `dialout` — because `PrivateDevices=true` creates a separate `/dev` mount that omits USB serial devices.

**Why it happens:**
`PrivateDevices=true` creates a minimal `devtmpfs` with only pseudo-devices (null, zero, urandom, etc.). USB serial devices (`/dev/ttyUSB*`, `/dev/ttyACM*`) are excluded from this private namespace.

**How to avoid:**
- Do **not** use `PrivateDevices=true` for a serial port daemon
- Use `DeviceAllow` explicitly instead:
  ```ini
  DeviceAllow=/dev/ttyUSB0 rw
  # Or for stable symlink:
  DeviceAllow=/dev/ttyRS485 rw
  ```
- Safe hardening options that work with serial access:
  ```ini
  [Service]
  User=rs485-logger
  NoNewPrivileges=true
  ProtectSystem=strict
  ProtectHome=true
  PrivateTmp=true
  # NOT PrivateDevices=true
  ReadWritePaths=/var/log/rs485-logger
  ```

**Warning signs:**
- `EACCES` or `ENOENT` on serial port open despite correct file permissions
- Service works when run manually as the same user, fails under systemd
- `systemd-analyze security rs485-logger` shows good scores but service won't start

**Phase to address:** Phase 4 (systemd service unit)

---

### Pitfall 12: InfluxDB Line Protocol Timestamp Precision Mismatch Causes Data Rejection or Silent Duplicate Collisions

**What goes wrong:**
You generate timestamps in milliseconds (e.g., `SystemTime::now()` → `.as_millis()`) but send them to InfluxDB without specifying `precision=ms` in the query parameter. InfluxDB 3 defaults to **nanoseconds**. Your 13-digit ms timestamp is interpreted as nanoseconds — placing all writes in year 1970. All data is written to the distant past and queries against the current time range return nothing.

Conversely, if you specify `precision=s` but send nanosecond timestamps, writes are rejected as out-of-range.

**How to avoid:**
- Use `SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as i64` for native nanosecond precision
- OR use `precision=ms` in write URL and `as_millis() as i64`
- Set the precision **explicitly** in the URL: `POST /api/v3/write_lp?db=power&precision=ns`
- Write a unit test that verifies the timestamp written can be queried back within ±5s of `now()`

**Warning signs:**
- Writes return HTTP 200 but queries `SELECT * FROM ... WHERE time > now() - 1h` return 0 rows
- Data appears if you query `WHERE time > '1970-01-01T00:00:01Z'` (milliseconds mistaken for nanoseconds)
- Grafana shows "No data" for the last hour despite active polling

**Phase to address:** Phase 2 (InfluxDB write integration)

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Hardcode `/dev/ttyUSB0` in config | No udev setup needed | Daemon breaks on reboot if enumeration order changes | Never — udev rule is 5 minutes of work |
| Write raw register integers to InfluxDB | Simpler code | Field type conflicts when value hits 0; misleading dashboards | Never — always scale to physical units |
| Ignore HTTP errors from InfluxDB write | Simpler error path | Silent data loss for hours; no observability | Never — log every non-2xx response |
| Use `unwrap()` on serial read | Faster prototype | Daemon crashes on first transient CRC error, defeating resilience goal | Prototype only — replace before first deployment |
| Single global `tokio-modbus` context for all devices | Simpler code | Context holds Modbus state; one device error can poison bus state for others | Acceptable for v1 if devices are polled sequentially with error recovery |
| Skip timestamp in line protocol (let InfluxDB use server time) | No clock sync needed | Poll time ≠ measurement time; drift during InfluxDB overload causes phantom gaps | Acceptable only if sub-second accuracy is irrelevant |

---

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| PZEM-016 via Modbus | Use `read_holding_registers` (FC 0x03) | Use `read_input_registers` (FC 0x04) — PZEM ignores FC 0x03 |
| PZEM-016 32-bit values | Assume Modbus big-endian word order (high word first) | PZEM uses low-word-first; reconstruct as `(reg[n+1] as u32) << 16 \| reg[n] as u32` |
| InfluxDB 3 write API | POST to `/write` (v1) or `/api/v2/write` | POST to `/api/v3/write_lp?db=<bucket>` with Bearer token |
| InfluxDB 3 field types | Mix integer `0i` and float `0.0` for same field | Always use float suffix — NEVER `i` for measurement fields |
| tokio-modbus RTU connect | Call `connect()` (removed in v0.8+) | Use `attach()` / `attach_slave()` — sync, infallible, requires active runtime |
| RS485 multi-device | Poll all devices concurrently | Poll strictly serially with ≥ 50 ms inter-device gap |
| systemd hardening | Add `PrivateDevices=true` from template | Use `DeviceAllow=/dev/ttyRS485 rw` instead — serial devices excluded from private devtmpfs |

---

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Write one line-protocol record per device per field | HTTP connection overhead for 6 fields × N devices per interval | Batch all fields for one device in a single line; batch all devices in one HTTP POST body | Even at 1 device / 10s, connection overhead accumulates on Pi's CPU |
| Create a new HTTP client per write | TLS handshake per poll cycle, high CPU on Pi | Reuse `reqwest::Client` (connection pool) across all writes | With 5+ devices at 5s intervals, CPU spikes noticeable |
| Blocking serial read inside async task without `spawn_blocking` | tokio thread pool starved; timer drift | Use `tokio-serial`'s async `SerialStream` — non-blocking by design | Under load with many devices or short intervals |
| Large `Vec<u8>` allocations per Modbus frame | Minor — 25 bytes per frame | Pre-allocate or use stack buffers; tokio-modbus handles this internally since v0.9 | Not a real concern at this scale |

---

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| InfluxDB token stored world-readable in config file | Token leakage → unauthorized writes, data deletion | `chmod 640 /etc/rs485-logger/config.toml` + `chown root:rs485-logger` |
| Running daemon as root (avoid for convenience) | Serial port exploit or panic → full Pi access | Run as dedicated user; use `dialout` group membership |
| Config file without validation on startup | Bad TOML (e.g., empty device list) causes runtime panics vs. early failure | Validate config at startup before opening serial port; fail fast with clear error |
| Logging InfluxDB token to file at DEBUG level | Token appears in log files | Never log the token value; log only "token configured: yes/no" |

---

## "Looks Done But Isn't" Checklist

- [ ] **Modbus polling:** Verify device address range — PZEM factory default is 0x01; confirm actual addresses assigned to hardware before writing config
- [ ] **Energy rollover:** Daemon must log a warning when energy register value decreases between polls — verify this with a simulated reset test
- [ ] **InfluxDB field types:** After first write, query `SHOW COLUMNS FROM <measurement>` to confirm all numeric fields are `DOUBLE` not `INT64`
- [ ] **Timestamp precision:** Verify written timestamps appear in correct time range with `SELECT * FROM ... WHERE time > now() - 5m`
- [ ] **Serial permissions:** Confirm daemon starts successfully after `reboot` (not just after manual deployment)
- [ ] **udev symlink:** Confirm `/dev/ttyRS485` exists after reboot AND after unplugging/replugging the adapter
- [ ] **systemd restart:** Confirm `Restart=on-failure` and `RestartSec=5s` are in service unit — first device timeout on startup must not kill the daemon permanently
- [ ] **Skip-and-continue:** Disconnect one PZEM device and verify the daemon continues polling the remaining devices without restarting
- [ ] **InfluxDB unavailable:** Kill InfluxDB and verify daemon logs HTTP errors but does NOT crash — then reconnect and verify data resumes

---

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| Wrong 32-bit word order in register decode | LOW | Fix decode function; historical data is permanently wrong but new data correct immediately |
| InfluxDB field type conflict (int vs float) | HIGH | Must drop the measurement and re-create; all historical data lost for that measurement |
| Energy counter reset undetected, corrupted cumulative in Grafana | MEDIUM | Add `WHERE energy > 0` filter to queries; use `non_negative_derivative()` going forward |
| Serial port path instability | LOW | Add udev rule; restart daemon |
| systemd PrivateDevices blocks serial | LOW | Edit service unit; `systemctl daemon-reload && systemctl restart` |
| Timestamp precision mismatch (writes going to 1970) | MEDIUM | Fix precision in write URL; delete incorrect data manually; no historical gap but cleanup needed |

---

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| PZEM 32-bit word order | Phase 1: Modbus polling | Integration test: read one device, assert voltage within 100–260V range |
| PZEM function code (FC 0x04 only) | Phase 1: Modbus polling | Confirm no ExceptionCode in first successful poll |
| Inter-request delay on multi-device bus | Phase 1 (single device) + Phase 3 (multi-device) | Run 3+ device poll for 1 hour, count CRC errors = 0 |
| Serial port path unstable | Phase 4: systemd deployment | Reboot Pi, confirm service starts without intervention |
| Serial port permissions | Phase 4: systemd deployment | Confirm daemon runs under non-root user with dialout group |
| InfluxDB field type conflict | Phase 2: InfluxDB integration | Inspect column schema after first write; regression test for zero-value fields |
| Wrong InfluxDB write endpoint | Phase 2: InfluxDB integration | curl smoke test against exact endpoint before integrating |
| Energy register rollover handling | Phase 1: polling + Phase 3: resilience | Unit test for decreasing energy value; daemon logs WARN |
| tokio-modbus API changes (attach vs connect) | Phase 1: Modbus RTU setup | Compile with latest tokio-modbus; check CHANGELOG for breaking changes |
| Cross-compilation linker/libudev | Phase 4: build + deployment | CI build for aarch64-unknown-linux-gnu passes cleanly |
| systemd PrivateDevices blocks serial | Phase 4: service unit | `systemd-analyze security` + verify `DeviceAllow` works on Pi |
| Timestamp precision mismatch | Phase 2: InfluxDB integration | Assert written records are queryable within 60s of write |

---

## Sources

- **ESPHome PZEM-AC source** (`pzemac.cpp`): Verified register layout, word order, function codes, scaling factors — https://github.com/esphome/esphome/blob/dev/esphome/components/pzemac/pzemac.cpp (HIGH confidence)
- **tokio-modbus CHANGELOG**: `v0.8.2` — "Clear rx buffer before sending to help with error recovery on unreliable physical connections" — https://github.com/slowtec/tokio-modbus/blob/main/CHANGELOG.md (HIGH confidence)
- **tokio-modbus CHANGELOG**: `v0.8.0` — replaced `connect()` with synchronous `attach()` API (HIGH confidence)
- **InfluxDB 3 line protocol reference**: Field type immutability, timestamp precision default (nanoseconds), special characters — https://docs.influxdata.com/influxdb3/cloud-serverless/reference/syntax/line-protocol/ (HIGH confidence)
- **InfluxDB 3 schema design guide**: Field type conflicts, column schema enforcement — https://docs.influxdata.com/influxdb3/cloud-serverless/write-data/best-practices/schema-design/ (HIGH confidence)
- **tokio-serial docs.rs**: Struct/trait layout confirming serialport dependency chain — https://docs.rs/tokio-serial/latest/tokio_serial/ (HIGH confidence)
- **Cargo configuration reference**: Cross-compilation target linker configuration — https://doc.rust-lang.org/cargo/reference/config.html#target (HIGH confidence)
- **influxdata/line-protocol Go README**: Notes on uint64 truncation and type enforcement at encode time (MEDIUM confidence — Go library, but same protocol rules apply)
- **Domain knowledge (HIGH confidence)**: RS485 half-duplex timing, CH340/CP2102 direction-switch latency, Linux udev stable device naming, systemd `PrivateDevices` behavior with `/dev/ttyUSB*`, PZEM-016 power-loss energy reset behavior

---
*Pitfalls research for: Rust Modbus RTU + PZEM-016 + RS485 + InfluxDB 3 on Raspberry Pi*
*Researched: 2026-04-02*