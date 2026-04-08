---
status: done
migrated: true
---

# M001: rs485-logger v1.0 MVP — Milestone Summary

**Shipped:** 2026-04-03
**Phases completed:** 7 (S01–S07)
**LOC:** ~1,737 Rust across 6 source files

## What Was Built

A production-ready Rust daemon that polls multiple PZEM-016 power meters connected in a Modbus RS485 daisy chain via USB-to-RS485 adapter on a Raspberry Pi. The daemon reads all available measurements (voltage, current, power, energy, frequency, power factor) at a configurable interval and writes them into InfluxDB 3 with each device landing in its own named measurement.

## Slices Completed

| Slice | Title | Completed |
|-------|-------|-----------|
| S01 | Foundation (config, types, register decoder) | 2026-04-02 |
| S02 | InfluxDB Integration (line protocol, HTTP write) | 2026-04-02 |
| S03 | Modbus + Poll Loop (RTU client, error isolation, shutdown) | 2026-04-02 |
| S04 | Systemd Deployment (service unit, udev, cross-compile) | 2026-04-02 |
| S05 | README / Manual (E2E hardware → InfluxDB guide) | 2026-04-02 |
| S06 | Daily Energy Reset (FC 0x42, chrono-tz, select! arm) | 2026-04-03 |
| S07 | Daemon Reliability Hardening (14 findings resolved) | 2026-04-03 |

## Key Files

- `src/main.rs` — tokio::main, poll loop, signal handling, daily reset select! arm, reliability counters
- `src/config.rs` — AppConfig / DeviceConfig / InfluxConfig / EnergyResetConfig structs, validation
- `src/types.rs` — PowerReading struct, decode_registers() with correct PZEM-016 word order
- `src/poller.rs` — ModbusPoller (rtu::attach + set_slave), poll_device(), reset_energy() FC 0x42
- `src/influx.rs` — to_line_protocol(), InfluxWriter with reqwest (rustls-tls, HTTP timeouts)
- `src/scheduler.rs` — next_reset_instant(), far_future()
- `rs485-logger.service` — systemd unit with Restart=always
- `deploy/install.sh` — deployment script (udev, service install)
- `Cross.toml` — cargo cross config for aarch64/armv7
- `README.md` — comprehensive E2E manual

## Key Decisions

See DECISIONS.md for the full decision register (D001–D015).

## Tech Debt Deferred

- CFG-02: `parity: Option<String>` missing from `SerialConfig` — PZEM-016 8N1 default works implicitly; add in v1.1
- RISK-1: udev rule driver name (`ch341` vs `cp210x`) mismatch with README documentation
- reqwest feature `"rustls"` should be `"rustls-tls"` per canonical crates.io name (works today as alias)

## Final Test Results

- 39 unit tests pass (4 ignored — hardware/integration gated)
- `cargo clippy -- -D warnings`: clean, no warnings
- Cross-compiled aarch64 binary confirmed on Raspberry Pi

---
*Milestone completed: 2026-04-03*
