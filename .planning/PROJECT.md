# rs485-logger

## What This Is

A Rust daemon that polls multiple PZEM-016 power meters connected in a Modbus RS485 daisy chain via USB-to-RS485 adapter on a Raspberry Pi. It reads all available measurements (voltage, current, power, energy, frequency, power factor) at a configurable interval and writes them into InfluxDB 3, with each device landing in its own named measurement.

## Core Value

Reliable, continuous power data from every PZEM-016 flowing into InfluxDB without data gaps — even when individual devices go offline.

## Requirements

### Validated

(None yet — ship to validate)

### Active

- [ ] Read all PZEM-016 data fields (voltage, current, power, energy, frequency, power factor) via Modbus RTU over RS485
- [ ] Support dynamic number of devices defined in TOML config (not hardcoded)
- [ ] Assign a human-readable name to each device in config; use name as InfluxDB measurement name
- [ ] Global polling interval configured in TOML
- [ ] Write data to InfluxDB 3 (local or remote, URL + token + org + bucket in config)
- [ ] Skip failed device reads, log the error, continue polling other devices
- [ ] Run as a long-running daemon suitable for systemd
- [ ] Write operational logs to both console and file (configurable paths/levels)

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
| Rust for implementation | Low memory footprint on Pi, reliability, strong serial/async ecosystem | — Pending |
| TOML config format | Idiomatic in Rust ecosystem (serde + toml crate), human-friendly | — Pending |
| Per-device InfluxDB measurement (named) | Allows per-device dashboards and queries without tag filtering | — Pending |
| Skip-and-log on device error | Partial data is better than no data; daemon must stay alive | — Pending |
| Global polling interval | Simplifies scheduling; PZEM-016 response time makes per-device intervals unnecessary at typical intervals | — Pending |

---
*Last updated: 2026-04-02 after initialization*

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
