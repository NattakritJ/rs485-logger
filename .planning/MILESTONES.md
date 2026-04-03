# Milestones: rs485-logger

## v1.0 MVP — Shipped 2026-04-03

**Phases:** 7 | **Plans:** 16 | **Commits:** 69
**Timeline:** 2026-04-02 → 2026-04-03 (2 days)
**LOC:** ~1,737 Rust

**Delivered:** Full Rust daemon that polls PZEM-016 power meters over Modbus RTU RS485, writes measurements to InfluxDB 3, runs as a hardened systemd service on Raspberry Pi with daily energy reset scheduling.

### Key Accomplishments

1. Rust project skeleton with full tech stack (tokio, tokio-modbus, reqwest, tracing, serde) — compiles clean for aarch64
2. TOML config parsing + startup validation — device list, serial, InfluxDB, polling interval, energy reset config
3. InfluxDB 3 HTTP write client — float-typed line protocol, Bearer auth, error handling (STOR-01–STOR-04)
4. Modbus RTU poll loop — sequential device polling, skip-and-continue on errors, graceful SIGTERM/SIGINT shutdown
5. Systemd deployment artifacts — service unit, udev `/dev/ttyRS485` symlink, cross-compilation (aarch64/armv7), install script
6. Daily energy reset via Modbus FC 0x42 at 00:00 Asia/Bangkok with IANA timezone support + daemon reliability hardening (HTTP timeouts, serial recovery, config validation, log rotation)

### Requirements Coverage

19/19 requirements satisfied (16 v1 + 3 added during development: DOC-01, SCHED-01, REL-01)

### Archive

- [.planning/milestones/v1.0-ROADMAP.md](milestones/v1.0-ROADMAP.md) — full phase details
- [.planning/milestones/v1.0-REQUIREMENTS.md](milestones/v1.0-REQUIREMENTS.md) — requirements with outcomes
- [.planning/milestones/v1.0-MILESTONE-AUDIT.md](milestones/v1.0-MILESTONE-AUDIT.md) — audit report

### Known Tech Debt

| ID | Severity | Description |
|----|----------|-------------|
| CFG-02 | MEDIUM | Parity field missing from SerialConfig — PZEM-016 8N1 default works implicitly |
| RISK-1 | LOW | udev rule uses `ch341` driver vs `cp210x` in README documentation |
| RISK-4 | LOW | reqwest feature `"rustls"` should be `"rustls-tls"` (works today as alias) |
| REL-01 | LOW | Orphaned REQ-ID in ROADMAP.md — not formally defined in REQUIREMENTS.md |
| — | LOW | 5 of 7 phases have no VERIFICATION.md (phases 1, 2, 4, 5, 6) |

---

*Tag: v1.0*
