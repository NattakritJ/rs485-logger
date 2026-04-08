# S07: Daemon Reliability Hardening

**Status:** ✅ Completed 2026-04-03
**Goal:** Fix all 14 findings from the daemon reliability verification report — eliminate daemon-hang modes, unrecoverable serial failures, config validation gaps, and operational hygiene issues so the daemon runs reliably on a Raspberry Pi indefinitely.
**Demo:** 39 unit tests pass, clippy clean; `consecutive_all_fail` counter exits after 10 cycles (systemd restarts); InfluxDB health state machine suppresses log spam on sustained outage; log rotation prevents unbounded SD card growth.

## Must-Haves

- InfluxDB HTTP timeouts: 5s connect, 10s request (CRIT-01/HIGH-01)
- `config.toml` removed from git + `config.toml.example` added (CRIT-03)
- Config validation: device names, energy reset timezone/time, clock warning (HIGH-02/MED-05/LOW-02)
- Serial recovery: exit + systemd restart after 10 consecutive all-fail cycles (CRIT-02)
- 100ms Modbus drain delay after per-device error (HIGH-04)
- InfluxDB health state machine — one WARN on first failure, silent until recovery (MED-04)
- Daily rolling log rotation via `tracing-appender::rolling::daily` (MED-01)
- Database name validation at config time (HIGH-03)

## Tasks

- [x] T01: InfluxDB client timeouts + git hygiene + log rotation (CRIT-01, CRIT-03, HIGH-01, HIGH-03, MED-01, MED-02)
- [x] T02: Config validation hardening — device names, energy reset timezone/time, clock warning (HIGH-02, MED-05, LOW-02)
- [x] T03: Runtime resilience — serial recovery exit, post-timeout delay, InfluxDB health backoff (CRIT-02, HIGH-04, MED-04)

## Files Likely Touched

- `src/main.rs`
- `src/influx.rs`
- `src/config.rs`
- `.gitignore`
- `config.toml.example`

## Key Decisions (from execution)

- CRIT-02 uses exit+systemd-restart (not in-process serial reconnect) — simpler, more reliable; no hardware to test reconnect with
- `MAX_CONSECUTIVE_ALL_FAIL=10` as a const in `main.rs` — no config knob added; can be exposed later if needed
- `influx_healthy` flag is per-daemon (not per-device) — all devices write to same InfluxDB instance; one health flag is correct
