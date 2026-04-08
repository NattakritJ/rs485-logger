---
phase: 02-revisit-polling-slowlyness
plan: "02"
subsystem: config, serial, deploy
tags: [parity, udev, config, serial, modbus, rs485]
dependency_graph:
  requires: []
  provides: [CFG-02, RISK-1-fix]
  affects: [src/config.rs, src/poller.rs, deploy/99-rs485.rules, README.md, config.toml.example]
tech_stack:
  added: []
  patterns: [Option<String> for optional config fields, fail-fast validation at startup, tokio_serial Parity enum mapping]
key_files:
  created: []
  modified:
    - src/config.rs
    - src/poller.rs
    - deploy/99-rs485.rules
    - README.md
    - config.toml.example
decisions:
  - "parity: Option<String> with default None (= 8N1 via unwrap_or) — backwards-compatible with existing configs"
  - "Parity validation at validate_config startup — fail-fast before serial port open"
  - "udev rule made driver-agnostic by removing DRIVERS== filter — works with cp210x, ch341, ftdi_sio"
metrics:
  duration: "3m"
  completed_date: "2026-04-09"
  tasks_completed: 2
  files_changed: 5
  commits: 2
requirements: [D-09, D-10, D-11, D-12, D-13, D-14]
---

# Phase 02 Plan 02: Parity Config + Udev Fix Summary

**One-liner:** Parity field added to SerialConfig (8N1 configurable, validated at startup) and udev rule made driver-agnostic (removes ch341-only DRIVERS== filter).

## What Was Built

### Task 1: Parity Config Field, Validation, and Poller Integration

- **`src/config.rs`** — Added `pub parity: Option<String>` to `SerialConfig` with doc comment explaining accepted values. Added validation in `validate_config` that rejects values other than "N", "E", "O" (case-insensitive). All 4 `SerialConfig` struct literals in test helpers updated with `parity: None`.
- **`src/poller.rs`** — `ModbusPoller::new` now maps `serial.parity` to `tokio_serial::Parity` enum and applies it via `.parity(parity)` on the `SerialPortBuilder`. Invalid values already rejected by validation before reaching this point; `_` arm safely defaults to `Parity::None`.
- **`config.toml.example`** — Added commented-out `parity = "N"` documentation block after `read_timeout_ms`.
- **6 new tests** covering: `"N"` value, `"E"`, `"O"`, lowercase `"n"`, invalid `"X"` rejection, absent field is `None`.

### Task 2: Driver-Agnostic Udev Rule and README Alignment

- **`deploy/99-rs485.rules`** — Replaced `DRIVERS=="ch341"` with no driver filter. Rule now matches any USB serial adapter on the `tty` subsystem. Added comment block with VID/PID pinning instructions for multi-device setups and reference list of common chip drivers.
- **`README.md`** — Replaced the driver-specific udev section with driver-agnostic documentation. Removed "you must edit the rule before running" instruction. Added VID/PID pinning instructions. Added `ftdi_sio` to driver table (was missing). Section title changed from "Finding Your Adapter's Driver" to "Pinning to a Specific Adapter."

## Commits

| Task | Commit | Description |
|------|--------|-------------|
| 1    | cb15129 | feat(02-02): add parity config field, validation, and apply in poller |
| 2    | 1a9c44b | fix(02-02): make udev rule driver-agnostic and align README docs |

## Verification Results

- `cargo test` — 47 passed, 0 failed, 4 ignored (hardware tests) ✓
- `cargo clippy -- -D warnings` — 0 warnings ✓
- `grep -c 'DRIVERS==' deploy/99-rs485.rules` → 0 ✓
- `grep 'parity' src/config.rs` shows field and validation ✓
- `grep 'parity' src/poller.rs` shows mapping and builder call ✓
- `grep 'driver-agnostic' README.md` confirms updated docs ✓

## Deviations from Plan

### Auto-fixed Issues (from 02-01 agent)

**[Rule 3 - Blocking] parity field and poller changes pre-applied in 02-01**
- **Found during:** Pre-execution review
- **Issue:** The 02-01 agent had already applied the `parity: Option<String>` field to `SerialConfig` and the `ModbusPoller::new` parity mapping as a Rule 3 auto-fix when it encountered compile failures
- **Impact on this plan:** `src/poller.rs` had no new changes in Task 1's commit (already done); `src/config.rs` needed validation logic and tests added as planned
- **All acceptance criteria still met:** Config field ✓, Validation ✓, Poller application ✓, Tests ✓

## Known Stubs

None — all functionality is fully implemented and wired.

## Self-Check: PASSED

Files confirmed:
- [FOUND] src/config.rs — parity field, validation, 6 new tests
- [FOUND] src/poller.rs — parity mapping in ModbusPoller::new
- [FOUND] deploy/99-rs485.rules — driver-agnostic, no DRIVERS== filter
- [FOUND] README.md — driver-agnostic docs, VID/PID instructions
- [FOUND] config.toml.example — parity commented-out option

Commits confirmed:
- [FOUND] cb15129 — feat(02-02): add parity config field
- [FOUND] 1a9c44b — fix(02-02): make udev rule driver-agnostic
