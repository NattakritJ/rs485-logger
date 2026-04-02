# rs485-logger

## What This Is

A Rust daemon that polls multiple PZEM-016 power meters connected in a Modbus RS485 daisy chain via USB-to-RS485 adapter on a Raspberry Pi. It reads all available measurements (voltage, current, power, energy, frequency, power factor) at a configurable interval and writes them into InfluxDB 3, with each device landing in its own named measurement.

## Core Value

Reliable, continuous power data from every PZEM-016 flowing into InfluxDB without data gaps — even when individual devices go offline.

## Requirements

### Validated

- [x] TOML config format — idiomatic in Rust ecosystem (serde + toml crate) — Validated in Phase 1: foundation
- [x] Per-device InfluxDB measurement (named) — measurement = device name, no tags needed — Validated in Phase 2: influxdb-integration
- [x] Read all PZEM-016 data fields (voltage, current, power, energy, frequency, power factor) via Modbus RTU over RS485 — Validated in Phase 3: modbus-poll-loop
- [x] Skip failed device reads, log the error, continue polling other devices — Validated in Phase 3: modbus-poll-loop
- [x] Graceful SIGTERM/SIGINT shutdown (completes current poll cycle, exits cleanly) — Validated in Phase 3: modbus-poll-loop
- [x] Structured logging to stderr (journald-compatible) with optional file appender — Validated in Phase 3: modbus-poll-loop

### Active

- [ ] Support dynamic number of devices defined in TOML config (not hardcoded)
- [ ] Global polling interval configured in TOML
- [ ] Write data to InfluxDB 3 (local or remote, URL + token + org + bucket in config)
- [ ] Run as a long-running daemon suitable for systemd

### Out of Scope

- Per-device polling intervals — single global interval is sufficient
- OAuth / env-var credential sourcing — token in config file is enough for v1
- One-shot / cron mode — daemon-only for v1
- Web UI or dashboard — InfluxDB handles visualization

## Context

- **Hardware:** Raspberry Pi (arm/arm64) with USB-to-RS485 adapter; PZEM-016 units daisy-chained on Modbus RTU bus (default 9600 baud, 8N1)
- **Protocol:** Modbus RTU; PZEM-016 uses addresses 1–16; all registers readable via standard function code 0x04
- **Target OS:** Linux (Raspberry Pi OS); deployment via systemd service unit
- **Language:** Rust — chosen for low resource usage on Pi and reliability
- **InfluxDB:** Version 3 (HTTP line protocol write API); supports both local and remote endpoints

## Constraints

- **Tech Stack**: Rust — no other languages
- **Hardware**: Must run on Raspberry Pi (arm/armv7/aarch64); binary should cross-compile or compile natively on Pi
- **Protocol**: Modbus RTU only — no Modbus TCP
- **InfluxDB**: Version 3 API (not v1/v2 compatible write endpoints)
- **Config**: TOML only
- **Deployment**: systemd daemon; no Docker requirement

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Rust for implementation | Low memory footprint on Pi, reliability, strong serial/async ecosystem | Confirmed — binary compiles for aarch64 natively |
| TOML config format | Idiomatic in Rust ecosystem (serde + toml crate), human-friendly | Confirmed — Phase 1 foundation |
| Per-device InfluxDB measurement (named) | Allows per-device dashboards and queries without tag filtering | Confirmed — Phase 2 |
| Skip-and-log on device error | Partial data is better than no data; daemon must stay alive | Confirmed — Phase 3; tracing::warn! + loop continues |
| Global polling interval | Simplifies scheduling; PZEM-016 response time makes per-device intervals unnecessary at typical intervals | Confirmed — Phase 3 poll loop |
| tokio-modbus 0.17 + SerialStream | Only async RTU crate integrating with tokio-serial; double-Result pattern confirmed | Confirmed — Phase 3; `rtu::attach(port)` + triple `?` |
| tracing-subscriber with EnvFilter | journald-compatible structured logging; RUST_LOG + log_level config + file appender | Confirmed — Phase 3 |

---
*Last updated: 2026-04-02 — Phase 3 complete: full Modbus poll loop wired with signal handling and structured logging*

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `/gsd-transition`):
1. Requirements invalidated? → Move to Out of Scope with reason
2. Requirements validated? → Move to Validated with phase reference
3. New requirements emerged? → Add to Active
4. Decisions to log? → Add to Key Decisions
5. "What This Is" still accurate? → Update if drifted

**After each milestone** (via `/gsd-complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state
