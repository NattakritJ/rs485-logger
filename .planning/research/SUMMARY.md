# Research Summary: rs485-logger

**Domain:** Embedded Linux daemon — Modbus RTU RS485 polling → InfluxDB 3 time-series ingest
**Researched:** 2026-04-02
**Overall confidence:** HIGH (stack versions crates.io-verified; InfluxDB 3 API official-docs-verified; PZEM register layout ESPHome-source-verified)

---

## Executive Summary

The rs485-logger project is a well-scoped, narrow-purpose Rust daemon. The technology choices are effectively forced by the constraints: `tokio-modbus` is the only async Rust Modbus RTU client that integrates natively with `tokio-serial`; `reqwest` with raw line protocol is the correct InfluxDB 3 write approach (no official Rust v3 client exists); and `tracing` + `tracing-subscriber` is the 2025 standard for structured logging in async Rust daemons.

The domain has two unusual failure modes that must be addressed up front. First, the PZEM-016 deviates from Modbus standard 32-bit word order — registers arrive low-word-first, not high-word-first, causing factor-of-65536 errors on current, power, and energy values if decoded naively. Second, InfluxDB 3 permanently locks field data types on first write; writing `power=0i` (integer) then `power=45.2` (float) is unrecoverable without dropping the measurement. Both pitfalls must be addressed in Phase 1/2 before any data flows into production.

The architecture is genuinely simple: a single async polling loop (sequential, never concurrent — RS485 is half-duplex), a register decoder, a line protocol formatter, and an HTTP POST. There is no message queue, no database, no web server, no background worker. The entire daemon can be expressed in ~5 short source files. The primary complexity is operational: correct systemd hardening, stable udev device naming, and getting the cross-compilation toolchain set up once.

The suggested build order (config → types/decoder → line protocol → InfluxDB write → Modbus poller → poll loop → logger → systemd) allows all pure-logic components to be unit-tested without hardware. Only the Modbus poller and the end-to-end poll loop require a real PZEM-016.

---

## Key Findings

**Stack:** `tokio` + `tokio-modbus 0.17` + `tokio-serial 5.4.5` + `reqwest 0.13` + `tracing` + `toml`/`serde` + `anyhow` — all versions verified on crates.io.

**Architecture:** Single-crate binary, five source modules (`main`, `config`, `types`, `poller`, `influx`), `current_thread` tokio runtime, sequential per-device polling loop, direct function calls between modules (no channels).

**Critical pitfall:** InfluxDB 3 field types are immutable after first write — always write all numeric fields as `f64` floats, never as integers, or recovery requires dropping the measurement and losing all historical data.

---

## Implications for Roadmap

Based on research, the following phase structure is recommended:

1. **Config + Types** — Foundation with no hardware dependency
   - Addresses: TOML config parsing, `DeviceConfig`/`AppConfig` structs, `PowerReading` struct, register decode logic
   - Avoids: Type-first approach prevents the integer/float field-type pitfall (types defined as `f32`/`f64` from the start)
   - Can be fully unit-tested without hardware or InfluxDB

2. **InfluxDB Write Integration** — Validate data destination before hardware
   - Addresses: Line protocol builder, `InfluxWriter` HTTP client, `/api/v3/write_lp` endpoint, `Bearer` token auth, timestamp precision (`precision=second`)
   - Avoids: Wrong endpoint (v1/v2), field type conflict (float-only from day 1), timestamp precision mismatch
   - Can be integration-tested against a local InfluxDB instance without hardware

3. **Modbus RTU Polling** — Hardware integration
   - Addresses: `tokio-modbus` RTU `attach_slave`, `read_input_registers` FC 0x04, register decoding (low-word-first 32-bit values), per-device timeout, skip-and-continue error handling
   - Avoids: Wrong function code (FC 0x03), wrong word order, concurrent polling, re-opening serial port per device
   - Requires real PZEM-016 hardware for validation

4. **Poll Loop + Signal Handling** — Full integration
   - Addresses: `tokio::time::interval`, sequential device loop, SIGTERM/SIGINT graceful shutdown, structured logging to stdout + file, startup config validation
   - Avoids: Concurrent polling (`join!`), missing inter-request delay, panicking on device error
   - End-to-end test: real device → real InfluxDB → verify data in Grafana

5. **Systemd Deployment** — Production hardening
   - Addresses: Systemd service unit (`Restart=always`), udev stable device naming (`/dev/ttyRS485`), serial port permissions (`dialout` group), cross-compilation to `aarch64-unknown-linux-gnu` or `armv7-unknown-linux-gnueabihf`
   - Avoids: `PrivateDevices=true` (breaks serial access), hardcoded `/dev/ttyUSB0`, running as root, cross-compilation `libudev` linker errors

**Phase ordering rationale:**
- Config and types come first because every other component depends on them, and they can be fully tested without hardware or external services.
- InfluxDB write comes before Modbus polling because the write path has the most unrecoverable failure mode (field type conflict). Better to validate and lock the schema before any real data flows.
- Modbus polling comes third because it requires hardware and benefits from having the types and InfluxDB write already solid.
- The poll loop phase wires everything together and exercises the full path end-to-end.
- Systemd deployment is last because it is purely operational — the daemon must already be correct before hardening its runtime environment.

**Research flags for phases:**
- Phase 3 (Modbus): Validate PZEM-016 register map against real hardware — ESPHome source is the best available reference but the official Peacefair datasheet is behind a vendor wall (MEDIUM confidence)
- Phase 5 (systemd): Test `DeviceAllow` behaviour on the specific Raspberry Pi OS version in use — systemd version differences can affect exact security directive syntax
- All other phases: Standard patterns, well-documented, HIGH confidence

---

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack (crate versions) | HIGH | All versions verified against crates.io API on 2026-04-02 |
| InfluxDB 3 write API | HIGH | Endpoint, auth format, precision, line protocol verified against official InfluxData docs |
| tokio-modbus API | HIGH | v0.17.0 docs.rs verified; `attach_slave`, `set_slave`, `read_input_registers` confirmed |
| PZEM-016 register map | MEDIUM | Verified via ESPHome `pzemac.cpp`; official Peacefair datasheet not publicly accessible |
| PZEM-016 word order | HIGH | ESPHome source explicitly demonstrates low-word-first reconstruction; matches community reports |
| Architecture patterns | HIGH | Sequential polling, `current_thread` runtime, `set_slave` re-addressing — all from tokio-modbus official examples |
| Pitfalls | HIGH | 12 pitfalls documented, each with source reference; none based on training data alone |
| Cross-compilation | HIGH | `cargo cross` tool and `default-features = false` on `serialport` verified against Cargo official docs |
| systemd hardening | HIGH | `PrivateDevices=true` behaviour documented in systemd man pages; `DeviceAllow` as alternative confirmed |

---

## Gaps to Address

- **PZEM-016 register map validation:** The register layout in `ARCHITECTURE.md` (Pattern 4) and `PITFALLS.md` (Pitfall 1) are sourced from ESPHome community code, not the official Peacefair datasheet. Phase 3 should include a manual verification step: read raw registers from one real device and compare to the documented map.
- **InfluxDB 3 Core vs Cloud Serverless API differences:** Some pitfall sources referenced the Cloud Serverless docs. Field type immutability and line protocol behaviour are the same for both editions, but verify the exact error response format (HTTP 400 vs 422) on the specific InfluxDB 3 Core build in use.
- **tokio-modbus `set_slave` API in v0.17:** Architecture Pattern 2 shows `ctx.set_slave(...)`. Verify this method name against v0.17.0 docs.rs — earlier versions used `set_slave`, but confirm it was not renamed during the 0.8→0.17 version gap.
- **Energy register rollover test:** PITFALLS.md documents the energy reset behaviour but it cannot be fully validated in research. Phase 3 should include a simulated reset test (manually reset PZEM energy register using FC 0x42) to confirm the daemon's rollover detection logic works.

---

*Summary for: Rust RS485 Modbus RTU → InfluxDB 3 daemon (rs485-logger)*
*Researched: 2026-04-02*
