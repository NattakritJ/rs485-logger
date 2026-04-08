---
phase: 02-revisit-polling-slowlyness
verified: 2026-04-09T14:00:00Z
status: passed
score: 9/9 must-haves verified
re_verification: false
---

# Phase 02: Revisit Polling Slowlyness — Verification Report

**Phase Goal:** Eliminate ~3.15s InfluxDB write bottleneck from poll loop via fire-and-forget tokio::spawn writes; fold in CFG-02 parity config and RISK-1 udev rule fix.
**Verified:** 2026-04-09
**Status:** ✅ PASSED
**Re-verification:** No — initial verification

---

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | InfluxDB writes no longer block the Modbus poll loop — each write is spawned as a fire-and-forget task | ✓ VERIFIED | `tokio::spawn(async move {` at main.rs:320; `writer_clone.write(&reading).await` only inside spawned block (line 321); no direct `writer.write(&reading).await` in poll loop body |
| 2 | InfluxDB health state transitions (healthy→unhealthy, unhealthy→healthy) are logged correctly from spawned tasks | ✓ VERIFIED | `health.swap(false, Ordering::Relaxed)` at main.rs:332 triggers WARN on first failure; `health.store(true, Ordering::Relaxed)` at main.rs:325 triggers INFO on restore; both inside spawned async block |
| 3 | Existing skip-and-log behavior for device poll errors is preserved — no regression | ✓ VERIFIED | `Err(e)` arm (main.rs:345–357) unchanged: `tracing::warn!` + 100ms drain sleep; `bus_delay_read()` still called after every device |
| 4 | tokio runtime is multi_thread (already the case) | ✓ VERIFIED | `#[tokio::main(flavor = "multi_thread")]` at main.rs:59 |
| 5 | Parity is configurable in TOML config with `Option<String>` defaulting to 'N' (8N1) | ✓ VERIFIED | `pub parity: Option<String>` in config.rs:34; `unwrap_or("N")` in poller.rs:40; `# parity = "N"` commented in config.toml.example:30 |
| 6 | Invalid parity value causes config validation error at startup (fail-fast) | ✓ VERIFIED | `validate_config` checks upper == "N" \|\| "E" \|\| "O" at config.rs:106–113; `test_parity_invalid_rejected` test confirms "X" is rejected |
| 7 | Existing configs without parity field continue to work without changes | ✓ VERIFIED | `Option<String>` field with `unwrap_or("N")` — absent field deserialises to `None`; `test_parity_absent_is_none` confirms `VALID_CONFIG` (no parity key) gives `None` |
| 8 | Udev rule is driver-agnostic — works with ch341, cp210x, and ftdi_sio adapters | ✓ VERIFIED | `grep -c 'DRIVERS==' deploy/99-rs485.rules` returns `0`; rule is `SUBSYSTEM=="tty", SUBSYSTEMS=="usb", SYMLINK+="ttyRS485", MODE="0660", GROUP="dialout"` |
| 9 | README udev documentation matches the actual deploy/99-rs485.rules file | ✓ VERIFIED | README.md:413 says "driver-agnostic"; no `DRIVERS==` in README; `ATTRS{idVendor}` VID/PID instruction at README.md:430; `ftdi_sio` in driver table at README.md:437 |

**Score:** 9/9 truths verified

---

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/influx.rs` | `#[derive(Clone)]` on `InfluxWriter` | ✓ VERIFIED | Line 27: `#[derive(Clone)]`; struct at line 28 |
| `src/main.rs` | `tokio::spawn` fire-and-forget, `Arc<AtomicBool>` health, `use std::sync::atomic`, `use std::sync::Arc` | ✓ VERIFIED | Lines 12–13: imports; line 178: `Arc::new(AtomicBool::new(true))`; line 317: `writer.clone()`; line 318: `Arc::clone(&influx_healthy)`; line 320: `tokio::spawn(async move {` |
| `src/config.rs` | `parity: Option<String>` in `SerialConfig`, validation in `validate_config`, 6 parity tests | ✓ VERIFIED | Line 34: field; lines 106–113: validation; 6 parity tests confirmed by `grep -c 'test_parity'` → 6 |
| `src/poller.rs` | Parity mapped to `tokio_serial::Parity`, applied via `.parity(parity)` | ✓ VERIFIED | Lines 40–46: match arm maps "E"→Even, "O"→Odd, _→None; `.parity(parity)` chained on builder |
| `deploy/99-rs485.rules` | Driver-agnostic rule, no `DRIVERS==` clause, VID/PID comment | ✓ VERIFIED | `grep -c 'DRIVERS=='` = 0; rule line 29 has no driver filter; lines 9–12 document VID/PID usage; line 26 says "Driver-agnostic rule" |
| `README.md` | Driver-agnostic udev docs, VID/PID instruction, `ftdi_sio` in table | ✓ VERIFIED | Line 413: "driver-agnostic"; line 430: `ATTRS{idVendor}` example; line 437: ftdi_sio row |
| `config.toml.example` | Commented-out `parity = "N"` with doc block | ✓ VERIFIED | Lines 27–30: comment block + `# parity = "N"` |

---

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `src/main.rs` | `src/influx.rs` | `writer.clone()` into spawned task | ✓ WIRED | `writer_clone = writer.clone()` at main.rs:317; owned by spawned async block |
| `src/main.rs` | `Arc<AtomicBool>` | `Arc::clone(&influx_healthy)` shared between main and spawned tasks | ✓ WIRED | `let health = Arc::clone(&influx_healthy)` at main.rs:318; `health.swap`/`health.load`/`health.store` used inside spawned task |
| `src/config.rs` | `src/poller.rs` | `serial.parity` read in `ModbusPoller::new` | ✓ WIRED | `serial.parity.as_deref().unwrap_or("N")` at poller.rs:40; result applied to builder at line 46 |
| `deploy/99-rs485.rules` | `README.md` | README documents actual rule content | ✓ WIRED | README correctly reflects driver-agnostic rule; both contain VID/PID pattern; no mismatch |

---

### Data-Flow Trace (Level 4)

Not applicable — this phase modifies async task spawning patterns and config parsing, not data rendering pipelines. No components render dynamic data to end-users; all outputs are InfluxDB line-protocol writes and tracing logs.

---

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| All tests pass (47 unit tests) | `cargo test 2>&1 \| tail -5` | `47 passed; 0 failed; 4 ignored` | ✓ PASS |
| No clippy warnings | `cargo clippy -- -D warnings 2>&1 \| tail -3` | `Finished dev profile — 0 warnings` | ✓ PASS |
| `DRIVERS==` absent from udev rule | `grep -c 'DRIVERS==' deploy/99-rs485.rules` | `0` | ✓ PASS |
| `#[derive(Clone)]` on InfluxWriter | `grep 'derive(Clone)' src/influx.rs` | `#[derive(Clone)]` present | ✓ PASS |
| `tokio::spawn` in poll loop | `grep 'tokio::spawn' src/main.rs` | `tokio::spawn(async move {` present | ✓ PASS |
| `parity` field in SerialConfig | `grep 'parity' src/config.rs` | field + validation + tests all present | ✓ PASS |
| `writer.write` NOT directly awaited in poll loop | `grep -n 'writer\.write.*await' src/main.rs \| grep -v 'spawn\|async move'` | no output (0 matches) | ✓ PASS |
| `writer_clone.write` awaited inside spawned block | `grep -n 'writer_clone\.write.*await' src/main.rs` | line 321 — inside spawned task | ✓ PASS |

---

### Requirements Coverage

Requirements from phase plans: **D-01 through D-14** (02-01-PLAN covers D-01–D-08; 02-02-PLAN covers D-09–D-14).

Note: There is no standalone `REQUIREMENTS.md` at `.planning/REQUIREMENTS.md`. Requirements for this phase are defined inline in the `02-CONTEXT.md` decisions block (D-01 through D-14) and the ROADMAP. All 14 decision items are cross-referenced below.

| Requirement | Source Plan | Description (from 02-CONTEXT.md) | Status | Evidence |
|-------------|-------------|-----------------------------------|--------|----------|
| D-01 | 02-01 | Decouple InfluxDB writes via `tokio::spawn` fire-and-forget | ✓ SATISFIED | `tokio::spawn(async move {` at main.rs:320 |
| D-02 | 02-01 | Unbounded write tasks — no semaphore/channel cap | ✓ SATISFIED | No `Semaphore` or bounded channel in spawned write path |
| D-03 | 02-01 | Out-of-order HTTP delivery acceptable; timestamps set at Modbus read time | ✓ SATISFIED | `PowerReading.timestamp_secs` set in `poll_device`, moved into spawn block |
| D-04 | 02-01 | `#[derive(Clone)]` on `InfluxWriter` (reqwest::Client is Arc-based) | ✓ SATISFIED | `#[derive(Clone)]` at influx.rs:27 |
| D-05 | 02-01 | Spawned task owns `writer_clone`, `reading`, `Arc<AtomicBool>` | ✓ SATISFIED | `async move { ... }` at main.rs:320 captures all three by value |
| D-06 | 02-01 | Replace `influx_healthy: bool` with `Arc<AtomicBool>` | ✓ SATISFIED | `Arc::new(AtomicBool::new(true))` at main.rs:178 |
| D-07 | 02-01 | Health transitions: WARN on first fail, silent while unhealthy, INFO on restore | ✓ SATISFIED | `health.swap(false, Relaxed)` → WARN at main.rs:332; `health.store(true)` → INFO at main.rs:325 |
| D-08 | 02-01 | Use `Arc::clone` to pass health flag into spawned tasks | ✓ SATISFIED | `Arc::clone(&influx_healthy)` at main.rs:318 |
| D-09 | 02-02 | Add `parity: Option<String>` to `SerialConfig`, default "N" | ✓ SATISFIED | `pub parity: Option<String>` at config.rs:34; `unwrap_or("N")` at poller.rs:40 |
| D-10 | 02-02 | Apply parity in `ModbusPoller::new`; invalid string → validation error at startup | ✓ SATISFIED | Match arm at poller.rs:40–43; validation at config.rs:106–113 |
| D-11 | 02-02 | Backwards compatible: absent parity field defaults to "N" | ✓ SATISFIED | `Option<String>` + `unwrap_or("N")`; `test_parity_absent_is_none` passes |
| D-12 | 02-02 | Fix `deploy/99-rs485.rules` — remove mismatched `DRIVERS=="ch341"` clause | ✓ SATISFIED | `DRIVERS==` count = 0 in rules file |
| D-13 | 02-02 | Driver-agnostic rule: `SUBSYSTEM=="tty", SUBSYSTEMS=="usb", SYMLINK+="ttyRS485"`; VID/PID comment | ✓ SATISFIED | Rule line 29 confirmed; VID/PID in comments at lines 12 and 26 |
| D-14 | 02-02 | Update README.md to align with actual `deploy/99-rs485.rules` | ✓ SATISFIED | README says "driver-agnostic"; no old `DRIVERS==` text; `ftdi_sio` table row added |

**All 14 requirements satisfied.**

---

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| — | — | None found | — | — |

Scanned: `src/influx.rs`, `src/main.rs`, `src/config.rs`, `src/poller.rs`, `deploy/99-rs485.rules`, `README.md`, `config.toml.example`.

- No TODO/FIXME/PLACEHOLDER comments in modified files.
- No `return null` / `return {}` stubs in any artifact.
- No hardcoded empty data that flows to output.
- The `parity: None` values in test helpers are correct initial state, not stubs — the parity field is `Option<String>` and tests validate both `None` (absent) and `Some(...)` (set) cases.
- The `# parity = "N"` commented-out line in `config.toml.example` is a documentation pattern, not a stub.

---

### Human Verification Required

#### 1. Live Poll Cycle Duration Reduction

**Test:** On a Raspberry Pi with 5 PZEM-016 devices connected, run `rs485-logger` before and after this phase and compare logged cycle times.
**Expected:** Cycle time drops from ~5000ms (sequential: Modbus + InfluxDB per device) to ~450ms (Modbus-only: 5 × (60ms + 30ms IFD)).
**Why human:** Requires physical RS485 hardware and a running InfluxDB 3 instance; cannot be verified in CI.

#### 2. InfluxDB Health Suppression Behavior

**Test:** With InfluxDB unreachable, observe log output over multiple poll cycles. Then restore InfluxDB access and observe the restore log.
**Expected:**
  - First write failure: `WARN` log with device name and error
  - Subsequent failures (while down): **no** additional WARN logs — silent suppression
  - First success after restore: `INFO "InfluxDB connection restored"` appears once
**Why human:** Requires a live InfluxDB instance that can be toggled offline; involves timing-sensitive async behavior.

#### 3. Parity Field End-to-End on Hardware

**Test:** Set `parity = "E"` in config.toml, start the daemon against a PZEM-016 device.
**Expected:** Device times out (since PZEM-016 uses 8N1); confirms the parity setting is actually applied to the serial port (not ignored silently).
**Why human:** Requires physical hardware; the parity _application_ is hardware-observable but not software-testable without a real serial device.

#### 4. Udev Rule Symlink Creation

**Test:** Copy `deploy/99-rs485.rules` to `/etc/udev/rules.d/` on a Raspberry Pi with a USB-RS485 adapter, run `sudo udevadm control --reload-rules && sudo udevadm trigger`, then `ls -la /dev/ttyRS485`.
**Expected:** `/dev/ttyRS485` symlink appears regardless of adapter chip (cp210x, ch341, or ftdi_sio).
**Why human:** Requires physical hardware and Linux udev subsystem.

---

### Commit Verification

| Commit | Message | Contents |
|--------|---------|----------|
| `0816bca` | feat(02-01): decouple InfluxDB writes from Modbus poll loop via tokio::spawn | src/influx.rs Clone derive, src/main.rs fire-and-forget spawn, src/poller.rs parity test fix |
| `cb15129` | feat(02-02): add parity config field, validation, and apply in poller | src/config.rs parity field + validation + 6 tests, src/poller.rs parity mapping, config.toml.example |
| `1a9c44b` | fix(02-02): make udev rule driver-agnostic and align README docs | deploy/99-rs485.rules, README.md |
| `4b36ef3` | docs(02-01): complete decouple-influxdb-writes plan | 02-01-SUMMARY.md |
| `616c843` | docs(02-02): complete parity config + udev fix plan | 02-02-SUMMARY.md |

All commits verified present in `git log`.

---

### Gaps Summary

No gaps. All 9 observable truths are verified, all 7 required artifacts are substantive and wired, all 4 key links are connected, all 14 decision requirements (D-01–D-14) are satisfied, tests pass (47/47), and clippy is clean.

The phase goal — eliminate the ~3.15s InfluxDB write bottleneck and fold in CFG-02 + RISK-1 — is fully achieved in the codebase.

---

_Verified: 2026-04-09_
_Verifier: the agent (gsd-verifier)_
