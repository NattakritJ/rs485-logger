---
phase: 05-readme-manual
plan: "01"
subsystem: documentation
tags: [readme, docs, manual, hardware, wiring, configuration, deployment]
dependency_graph:
  requires: [03-modbus-poll-loop, 04-systemd-deployment]
  provides: [README.md — complete E2E user manual]
  affects: []
tech_stack:
  added: []
  patterns: [markdown documentation, annotated config examples]
key_files:
  created:
    - README.md
  modified:
    - src/main.rs
decisions:
  - README documents --config CLI flag which required fixing main.rs to actually parse it (was hardcoded to "config.toml")
  - Troubleshooting table documents both serial permission errors and InfluxDB auth failures
  - Manual startup section added for debugging outside systemd
metrics:
  duration: "3 min"
  completed: "2026-04-02T08:21:24Z"
  tasks: 1
  files: 2
---

# Phase 05 Plan 01: README.md E2E Manual Summary

**One-liner:** Standalone README.md covering PZEM-016 wiring, daisy-chain topology, config reference, InfluxDB 3 setup, native/cross-compile builds, install.sh deploy, systemd management, data verification, udev rule, and troubleshooting table — plus a bug fix for the `--config` CLI flag.

---

## What Was Built

A complete, standalone `README.md` (421 lines) structured as a user manual with 13 sections:

1. **Header** — project title and one-line description
2. **Overview** — design rationale, sequential polling, per-device measurements
3. **Prerequisites** — hardware list (Pi, PZEM-016, USB-RS485 adapter) and software (InfluxDB 3)
4. **Hardware Wiring** — PZEM-016 terminal overview, A/B RS485 adapter pinout, daisy-chain topology, termination resistor guidance, Pi connection
5. **Configuration (`config.toml`)** — complete annotated example with all fields, plus a reference table of types/defaults/constraints
6. **InfluxDB 3 Setup** — self-hosted Docker one-liner, Cloud URL, API token creation
7. **Installation** — native Pi build (rustup + cargo), cross-compile via `./deploy/build-release.sh`, deploy commands referencing `deploy/install.sh`
8. **Running the Daemon** — full systemctl lifecycle commands, expected healthy log output
9. **Verifying Data in InfluxDB** — curl SQL query example, field reference table
10. **udev Rule** — driver identification, per-chip table (cp210x/ch341/ftdi_sio), reload without reboot
11. **Troubleshooting** — 10-row table covering serial port errors, Modbus timeouts, InfluxDB auth/connection, config errors, and systemd enable
12. **PZEM-016 Register Map** — register addresses, scales, units, low-word-first note
13. **License** — MIT reference

---

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed missing `--config` CLI argument parsing in `main.rs`**
- **Found during:** Task 1 (while cross-referencing deploy files for README accuracy)
- **Issue:** `src/main.rs` hardcoded `load_config("config.toml")` with no CLI argument parsing. However, `deploy/rs485-logger.service` passes `ExecStart=/usr/local/bin/rs485-logger --config /etc/rs485-logger/config.toml` — this argument was silently ignored, causing the daemon to look for `config.toml` in the systemd working directory (which doesn't exist), resulting in a startup failure on any installed system.
- **Fix:** Added minimal `std::env::args()` parsing to detect `--config <path>` and use it as the config path; falls back to `"config.toml"` for local development. No new dependencies — uses only `std::env`.
- **Files modified:** `src/main.rs`
- **Commit:** `bb34653` (same commit as README)

---

## Decisions Made

- `--config` CLI argument parsing added to `src/main.rs` using `std::env::args()` (no clap/argparse dependency added — keeps binary minimal and avoids changing `Cargo.toml`)
- README uses `<PI_IP>` and `YOUR_TOKEN` as the only placeholder variables — all other commands are runnable as-is per plan directive
- Troubleshooting table added "Config not found" row (not in plan template) since the `--config` bug fix makes the config path important to document

---

## Known Stubs

None — README references only concrete, implemented functionality. All commands reference actual deploy scripts and config fields that exist in the codebase.

---

## Self-Check: PASSED

Files checked:
- `README.md` — FOUND (421 lines ≥ 200 ✓)
- `src/main.rs` — FOUND with --config parsing ✓

Commits:
- `bb34653` — feat(05-readme-manual-01): add README.md E2E manual and fix --config CLI arg ✓

Verification:
- `grep "poll_interval_secs" README.md` — 3 matches ✓
- `grep "install.sh" README.md` — 4 matches ✓
- `grep "cp210x" README.md` — 6 matches ✓
- `grep "ttyRS485" README.md` — 9 matches ✓
- `grep "Troubleshoot" README.md` — 1 match ✓
