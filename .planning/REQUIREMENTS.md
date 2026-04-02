# Requirements: rs485-logger

**Defined:** 2026-04-02
**Core Value:** Reliable, continuous power data from every PZEM-016 flowing into InfluxDB without data gaps — even when individual devices go offline.

## v1 Requirements

### Config

- [x] **CFG-01**: Operator can define device list (Modbus address + human-readable name) in a TOML config file
- [x] **CFG-02**: Operator can configure serial port path, baud rate, and parity in TOML
- [x] **CFG-03**: Operator can configure InfluxDB 3 endpoint URL, bearer token, and database name in TOML
- [x] **CFG-04**: Operator can set a global polling interval (seconds) in TOML
- [x] **CFG-05**: Daemon validates config at startup and emits clear error messages for invalid/missing values before entering the poll loop

### Polling

- [ ] **POLL-01**: Daemon reads all six PZEM-016 fields (voltage, current, power, energy, frequency, power factor) per device via Modbus RTU FC 0x04
- [ ] **POLL-02**: Daemon polls all configured devices sequentially (never concurrently) on every interval tick
- [ ] **POLL-03**: Daemon skips a failed device read (timeout, CRC error, etc.), logs the error, and continues polling remaining devices without restarting

### Storage

- [ ] **STOR-01**: Daemon writes each device's readings to InfluxDB 3 via HTTP POST to `/api/v3/write_lp` using line protocol, with the device name as the measurement name
- [ ] **STOR-02**: Daemon uses `Authorization: Bearer <token>` header for InfluxDB writes
- [ ] **STOR-03**: All numeric fields are written as `f64` floats (never integers) to prevent InfluxDB 3 field type conflicts
- [ ] **STOR-04**: Daemon logs InfluxDB write failures and continues polling (write errors do not crash or stall the daemon)

### Operations

- [ ] **OPS-01**: Daemon handles SIGTERM and SIGINT gracefully — completes the current poll cycle and exits cleanly
- [ ] **OPS-02**: Daemon emits structured logs to stdout/stderr (compatible with systemd journald)
- [ ] **OPS-03**: Daemon optionally writes logs to a file at a configurable path and log level
- [ ] **OPS-04**: A systemd `.service` unit file is included in the repository with `Restart=always` and `RestartSec=5`

## v2 Requirements

### Resilience

- **RES-01**: Daemon buffers failed InfluxDB writes in memory (bounded ring buffer) and retries on next successful connection
- **RES-02**: Daemon performs an InfluxDB connectivity check at startup and emits a clear error if unreachable

### Observability

- **OBS-01**: Daemon tracks consecutive error count per device and escalates log severity (warn at 3, error at 10 consecutive failures)
- **OBS-02**: Daemon emits a startup summary log line showing: serial port, baud rate, device count, InfluxDB URL, poll interval

### Developer Experience

- **DX-01**: Config file path is configurable via `--config <path>` CLI argument (default: `/etc/rs485-logger/config.toml`)

## Out of Scope

| Feature | Reason |
|---------|--------|
| Per-device polling intervals | Turns a simple sequential loop into a scheduler; all PZEM-016 respond in <200ms; global interval is sufficient |
| Disk-persistent write buffer (SQLite/CSV) | SD card write amplification on Raspberry Pi; adds complexity; in-memory buffer handles brief outages |
| Hot-reload of config (SIGHUP) | Config affects serial port setup; `systemctl restart` is fast and safe |
| Modbus TCP support | Different framing, different code path; project is RTU-only by constraint |
| Web UI / REST API | InfluxDB + Grafana already provides visualization |
| Alerting / threshold notifications | InfluxDB 3 or Grafana alerting handles this |
| OAuth / env-var credential sourcing | Token in TOML with `chmod 600` is sufficient for single-Pi deployment |
| One-shot / cron mode | Daemon-only; use `mbpoll` for ad-hoc reads |
| Auto-discovery of PZEM devices | Bus scan takes minutes, fragile on production systems; explicit TOML config is correct |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| CFG-01 | Phase 1 | Complete |
| CFG-02 | Phase 1 | Complete |
| CFG-03 | Phase 1 | Complete |
| CFG-04 | Phase 1 | Complete |
| CFG-05 | Phase 1 | Complete |
| POLL-01 | Phase 2 | Pending |
| POLL-02 | Phase 3 | Pending |
| POLL-03 | Phase 3 | Pending |
| STOR-01 | Phase 2 | Pending |
| STOR-02 | Phase 2 | Pending |
| STOR-03 | Phase 2 | Pending |
| STOR-04 | Phase 3 | Pending |
| OPS-01 | Phase 3 | Pending |
| OPS-02 | Phase 3 | Pending |
| OPS-03 | Phase 3 | Pending |
| OPS-04 | Phase 4 | Pending |

**Coverage:**
- v1 requirements: 16 total
- Mapped to phases: 16
- Unmapped: 0 ✓

---
*Requirements defined: 2026-04-02*
*Last updated: 2026-04-02 after initial definition*
