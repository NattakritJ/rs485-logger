# rs485-logger

## What This Is

A Rust daemon that polls multiple PZEM-016 power meters connected in a Modbus RS485 daisy chain via USB-to-RS485 adapter on a Raspberry Pi. It reads all available measurements (voltage, current, power, energy, frequency, power factor) at a configurable interval, writes them into InfluxDB 3 with each device landing in its own named measurement, performs a daily energy counter reset via Modbus FC 0x42, and runs as a hardened systemd service designed for indefinite unattended operation.

## Core Value

Reliable, continuous power data from every PZEM-016 flowing into InfluxDB without data gaps — even when individual devices go offline.

## Current State (v1.0 — Shipped 2026-04-03)

- **Status:** Production-ready daemon shipped
- **LOC:** ~1,737 Rust across 6 source files (main.rs, config.rs, types.rs, influx.rs, poller.rs, scheduler.rs)
- **Tech stack:** tokio 1.50 (current_thread), tokio-modbus 0.17, reqwest 0.13 (rustls), tracing 0.1, chrono-tz 0.10
- **Target:** aarch64-unknown-linux-gnu / armv7-unknown-linux-gnueabihf (Raspberry Pi); cross-compilation via `cargo cross`
- **Known tech debt:** CFG-02 parity field missing; RISK-1 udev driver doc mismatch; 5 phases lack VERIFICATION.md

## Requirements

### Validated

- ✓ TOML config format (device list, serial, InfluxDB, polling interval, energy reset) — v1.0
- ✓ Per-device InfluxDB measurement (named) — v1.0
- ✓ Read all PZEM-016 data fields (voltage, current, power, energy, frequency, power factor) via Modbus RTU FC 0x04 — v1.0
- ✓ Skip failed device reads, log the error, continue polling other devices — v1.0
- ✓ Graceful SIGTERM/SIGINT shutdown (completes current poll cycle, exits cleanly) — v1.0
- ✓ Structured logging to stderr (journald-compatible) with optional file appender — v1.0
- ✓ Write data to InfluxDB 3 via HTTP POST (Bearer auth, float-typed line protocol) — v1.0
- ✓ Run as systemd daemon (Restart=always, stable /dev/ttyRS485 udev symlink, install.sh) — v1.0
- ✓ Dynamic device list from TOML config (not hardcoded) — v1.0
- ✓ Global polling interval configured in TOML — v1.0
- ✓ Comprehensive E2E README.md manual (hardware wiring → deployment → InfluxDB verification) — v1.0
- ✓ Daily energy reset via Modbus FC 0x42 at configurable timezone/time — v1.0
- ✓ Daemon reliability hardening (HTTP timeouts, serial recovery, config validation, log rotation, git hygiene) — v1.0

### Active (Next Milestone)

- [ ] **CFG-02 gap:** Add `parity: Option<String>` to `SerialConfig` with default `"N"` — makes PZEM-016 8N1 explicit and user-configurable
- [ ] **RISK-1 fix:** Align udev rule driver name (`ch341` vs `cp210x`) with README documentation
- [ ] **reqwest feature:** Rename feature `"rustls"` to `"rustls-tls"` in Cargo.toml for canonical correctness

### Out of Scope

- Per-device polling intervals — single global interval is sufficient
- OAuth / env-var credential sourcing — token in config file is enough
- One-shot / cron mode — daemon-only
- Web UI or dashboard — InfluxDB handles visualization
- Disk-persistent write buffer — SD card write amplification; in-memory handling is sufficient
- Hot-reload of config (SIGHUP) — `systemctl restart` is fast and safe
- Modbus TCP support — RTU-only by constraint
- Auto-discovery of PZEM devices — explicit TOML config is safer on production systems

## Context

- **Hardware:** Raspberry Pi (arm/arm64) with USB-to-RS485 adapter; PZEM-016 units daisy-chained on Modbus RTU bus (default 9600 baud, 8N1)
- **Protocol:** Modbus RTU; PZEM-016 uses addresses 1–16; all registers readable via standard function code 0x04; energy reset via FC 0x42
- **Target OS:** Linux (Raspberry Pi OS); deployment via systemd service unit
- **Language:** Rust — chosen for low resource usage on Pi and reliability
- **InfluxDB:** Version 3 (HTTP line protocol write API at `/api/v3/write_lp`, Bearer auth); supports both local and remote endpoints
- **Energy reset:** Daily at configurable time in any IANA timezone (e.g. Asia/Bangkok) using chrono-tz; far_future() pattern parks the select! arm when disabled
- **Reliability:** 10 consecutive all-device-fail cycles → process exit + systemd restart; 100ms Modbus drain delay after per-device error; InfluxDB health tracking with WARN on first failure

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
| Rust for implementation | Low memory footprint on Pi, reliability, strong serial/async ecosystem | ✓ Confirmed — binary compiles for aarch64; ~1,737 LOC |
| TOML config format | Idiomatic in Rust ecosystem (serde + toml crate), human-friendly | ✓ Confirmed — Phase 1 |
| Per-device InfluxDB measurement (named) | Allows per-device dashboards and queries without tag filtering | ✓ Confirmed — Phase 2 |
| Skip-and-log on device error | Partial data is better than no data; daemon must stay alive | ✓ Confirmed — Phase 3; tracing::warn! + loop continues |
| Global polling interval | Simplifies scheduling; PZEM-016 response time makes per-device intervals unnecessary | ✓ Confirmed — Phase 3 poll loop |
| tokio-modbus 0.17 + rtu::attach(port) | Only async RTU crate integrating with tokio-serial; slave switched via set_slave() | ✓ Confirmed — Phase 3 |
| tracing-subscriber with EnvFilter | journald-compatible structured logging; RUST_LOG + log_level config + file appender | ✓ Confirmed — Phase 3 |
| reqwest rustls feature | Avoids OpenSSL dependency during cross-compilation to ARM | ✓ Confirmed — Phase 1/4 |
| chrono-tz for energy reset timezone | Bundles IANA tz database at compile time; avoids runtime system tz dependency | ✓ Confirmed — Phase 6 |
| far_future() parks disabled select! arm | No conditional select! needed; clean biased select! structure | ✓ Confirmed — Phase 6 |
| CRIT-02: exit + systemd restart (not in-process reconnect) | Simpler, more reliable; systemd handles restart correctly without hardware to test reconnect | ✓ Confirmed — Phase 7 |
| InfluxDB health tracking per-daemon (not per-device) | All devices write to same InfluxDB instance — one health flag is correct | ✓ Confirmed — Phase 7 |

---
*Last updated: 2026-04-03 after v1.0 milestone — full daemon shipped: Modbus RTU polling, InfluxDB 3 writes, daily energy reset, systemd deployment, reliability hardening*

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
