---
phase: 03-modbus-poll-loop
verified: 2026-04-02T00:00:00Z
status: passed
score: 4/4 must-haves verified
re_verification: false
gaps: []
human_verification:
  - test: "Single-device poll with physical PZEM-016"
    expected: "ModbusPoller.poll_device() returns a PowerReading with plausible values (voltage ~230V, frequency ~50Hz)"
    why_human: "Requires physical RS485 hardware — test is #[ignore]-gated; cannot verify register decode correctness against real hardware programmatically"
  - test: "Multi-device sequential poll produces separate InfluxDB measurements"
    expected: "Two device entries (e.g. solar_panel, grid_meter) each appear as a distinct measurement in InfluxDB after one poll cycle"
    why_human: "Requires running hardware + InfluxDB instance; line protocol logic is unit-tested but end-to-end path needs real stack"
  - test: "Skip-and-continue: disconnect one PZEM-016 mid-run"
    expected: "Daemon logs WARN with device name and error, continues polling remaining devices; no crash or restart"
    why_human: "Requires hardware manipulation; code path (match Err(e) → tracing::warn!) is verified in source but behavior needs live confirmation"
  - test: "SIGTERM graceful exit timing"
    expected: "kill -SIGTERM <pid> causes clean exit within 5 seconds, after current poll cycle completes"
    why_human: "Timing constraint (< 5 seconds) requires running daemon against hardware"
---

# Phase 03: Modbus Poll Loop — Verification Report

**Phase Goal:** Integrate `tokio-modbus` RTU client with real PZEM-016 hardware; wire config → poller → writer into the full sequential poll loop with skip-and-continue error handling, structured logging, and graceful shutdown.

**Verified:** 2026-04-02T00:00:00Z
**Status:** ✅ PASSED (automated) — 4 human verification items remain
**Re-verification:** No — initial verification
**Build:** `cargo build` → ✅ zero errors, zero warnings  
**Test suite:** `cargo test` → ✅ 17 passed; 0 failed; 3 ignored (hardware/InfluxDB gated)  
**Release build:** `cargo build --release` → ✅ clean

---

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | `ModbusPoller.poll_device()` reads PZEM-016 and returns a `PowerReading` with physically plausible values | ✓ VERIFIED (code) / ? HUMAN (hardware) | `src/poller.rs`: `rtu::attach(port)`, `set_slave`, `read_input_registers(0x0000, 10)` with 500ms timeout; `decode_registers` wired; hardware test is `#[ignore]`-gated |
| 2 | Daemon polls all configured devices sequentially; each device's data appears as a separate InfluxDB measurement | ✓ VERIFIED (code) / ? HUMAN (e2e) | `src/main.rs:112`: `for device in &cfg.devices` (sequential); `influx.rs:16-25`: measurement = `device_name`; `writer.write(&reading).await` called per device |
| 3 | Disconnect one PZEM-016 — daemon logs WARN, continues polling other devices, does not crash | ✓ VERIFIED (code) / ? HUMAN (live) | `src/main.rs:127-134`: `Err(e) => tracing::warn!(device = %device.name, error = %e, "Device poll failed, skipping")` — `continue` is implicit in `for` loop body |
| 4 | SIGTERM causes daemon to complete the current poll cycle and exit cleanly within 5 seconds | ✓ VERIFIED (code) / ? HUMAN (timing) | `src/main.rs:13-35`: `shutdown_signal()` handles SIGTERM + SIGINT; `tokio::pin!(shutdown)` outside loop; `tokio::select!` on `ticker.tick()` vs `&mut shutdown` — signal only breaks after cycle |

**Score:** 4/4 truths verified at code level. 4 human verification items for live hardware behavior (see section below).

---

## Required Artifacts

### Plan 03-01 Artifacts (POLL-01)

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/poller.rs` | `ModbusPoller` struct with `new()` and `poll_device()` | ✓ VERIFIED | 92 lines; `pub struct ModbusPoller`; `pub fn new`; `pub async fn poll_device`; no dead code |
| `src/main.rs` | `mod poller` declared | ✓ VERIFIED | Line 3: `mod poller;`; Line 8: `use poller::ModbusPoller;` |

### Plan 03-02 Artifacts (POLL-02, POLL-03)

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/main.rs` | Full poll loop: config → poller → influx writer | ✓ VERIFIED | `tokio::time::interval` at line 99; sequential `for device in &cfg.devices` at line 112; `writer.write(&reading).await` at line 119 |
| `src/main.rs` | Per-device error handling with `tracing::warn!` | ✓ VERIFIED | Lines 120-125 (InfluxDB write failure WARN); Lines 128-133 (device poll failure WARN); structured `device = %device.name, error = %e` fields present |

### Plan 03-03 Artifacts (OPS-01, OPS-02, OPS-03)

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/main.rs` | Signal handling via `tokio::signal` + graceful shutdown | ✓ VERIFIED | `shutdown_signal()` fn at lines 13-35; `tokio::pin!(shutdown)` at line 107; `tokio::select!` at line 110; `break` on signal at line 139 |
| `src/main.rs` | `tracing_subscriber` init with `EnvFilter` | ✓ VERIFIED | Lines 50-88: `EnvFilter::try_from_default_env()` → `log_level` config → "info" fallback; dual stderr+file branch; `_file_guard` lifetime fix |
| `src/config.rs` | Optional `log_file` and `log_level` in `AppConfig` | ✓ VERIFIED | Lines 12-13: `pub log_file: Option<String>` and `pub log_level: Option<String>` |

---

## Key Link Verification

| From | To | Via | Pattern Status | Manual Verdict |
|------|----|-----|----------------|----------------|
| `src/poller.rs` | `src/types.rs` | `decode_registers()` call | ✓ gsd-tools VERIFIED | `poller.rs:11` imports; `poller.rs:65` calls |
| `src/poller.rs` | `tokio-modbus` | `rtu::attach` + `read_input_registers` | ✗ gsd-tools (pattern mismatch: `rtu::attach_slave`) | ✓ VERIFIED — `rtu::attach(port)` at `poller.rs:33`; plan pattern outdated; decision documented in SUMMARY |
| `src/main.rs` | `src/poller.rs` | `ModbusPoller::new(&cfg.serial)` | ✓ gsd-tools VERIFIED | `main.rs:96` confirmed |
| `src/main.rs` | `src/influx.rs` | `writer.write(&reading).await` | ✓ gsd-tools VERIFIED | `main.rs:119` confirmed |
| `src/main.rs` | `tokio::time::interval` | `ticker.tick().await` in loop | ✗ gsd-tools (pattern: `interval.tick`) | ✓ VERIFIED — `ticker.tick()` at `main.rs:111`; naming mismatch in plan pattern only |
| `src/main.rs` | `tokio::signal::unix` | `tokio::select!` on signal future | ✓ gsd-tools VERIFIED | `main.rs:31` (shutdown_signal), `main.rs:110` (select!) |
| `src/main.rs` | `tracing_subscriber` | `tracing_subscriber::fmt()` + `EnvFilter` | ✓ gsd-tools VERIFIED | `main.rs:50-88` confirmed |

**Note:** Two gsd-tools UNVERIFIED results are false negatives due to plan pattern strings not matching actual (correct) implementation. Both manually confirmed as WIRED.

---

## Data-Flow Trace (Level 4)

`src/main.rs` is the primary runtime artifact. It does not render UI but does process and forward real sensor data.

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|--------------|--------|--------------------|--------|
| `src/main.rs` poll loop | `reading: PowerReading` | `poller.poll_device(device).await` | Yes — `ModbusPoller` reads live Modbus registers via `read_input_registers`; no static fallback | ✓ FLOWING |
| `src/influx.rs:write()` | `body: String` (line protocol) | `to_line_protocol(reading)` with all 6 fields from PowerReading | Yes — formats real `f64` values from reading; no hardcoded data | ✓ FLOWING |
| `src/types.rs:decode_registers()` | 6 `f64` fields | 10 raw `u16` Modbus registers | Yes — arithmetic decode (÷10, ÷1000, etc.) from raw register data | ✓ FLOWING |

---

## Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Binary compiles (dev) | `cargo build` | `Finished dev profile` — 0 errors | ✓ PASS |
| Binary compiles (release) | `cargo build --release` | `Finished release profile` — 0 errors | ✓ PASS |
| Unit tests pass | `cargo test` | `17 passed; 0 failed; 3 ignored` | ✓ PASS |
| `decode_registers` correctly decodes 10 registers | `test_basic_decode` | voltage=230.1V, current=1.234A, power=285.2W, freq=50.0Hz, pf=0.95 | ✓ PASS |
| Line protocol uses device name as measurement | `test_device_name_verbatim` | `"grid_meter ..."` | ✓ PASS |
| Float formatting prevents InfluxDB type conflicts | `test_zero_power_is_float` | `"power=0."` not `"power=0"` | ✓ PASS |
| Timestamp in nanoseconds | `test_timestamp_is_nanoseconds` | ends with `1700000000000000000` | ✓ PASS |
| Config validation rejects empty device list | `test_empty_device_list_rejected` | returns Err mentioning "device"/"empty" | ✓ PASS |
| Poll on hardware (RS485) | Requires physical device | Skipped — `#[ignore]` gated | ? SKIP |

---

## Requirements Coverage

| Requirement | Phase Assignment | Source Plan | Description | Code Evidence | Status |
|-------------|-----------------|-------------|-------------|---------------|--------|
| **POLL-01** | Phase 3 (plan says Phase 2 in traceability; plan 03-01 claims it) | 03-01-PLAN | Reads all 6 PZEM-016 fields via FC 0x04 | `poller.rs:58` FC 0x04, 10 regs; `types.rs:38-43` decodes voltage/current/power/energy/frequency/power_factor | ✓ SATISFIED |
| **POLL-02** | Phase 3 | 03-02-PLAN | Sequential device polling | `main.rs:112` `for device in &cfg.devices` — sequential (single `mut poller`, no `tokio::spawn`) | ✓ SATISFIED |
| **POLL-03** | Phase 3 | 03-02-PLAN | Skip failed device, log, continue | `main.rs:127-133` `Err(e) => tracing::warn!(...); /* loop continues */` | ✓ SATISFIED |
| **OPS-01** | Phase 3 | 03-03-PLAN | SIGTERM/SIGINT graceful exit | `main.rs:13-35` + `main.rs:137-140` `tokio::select!` + `break` | ✓ SATISFIED |
| **OPS-02** | Phase 3 | 03-03-PLAN | Structured logs to stderr (journald) | `main.rs:84-88` stderr writer; structured fields `device = %device.name, error = %e` | ✓ SATISFIED |
| **OPS-03** | Phase 3 | 03-03-PLAN | Optional file log at configurable path/level | `config.rs:12-13` `log_file`/`log_level` options; `main.rs:60-81` file appender branch with `_file_guard` | ✓ SATISFIED |

### Requirement Traceability Note

**POLL-01** is listed as "Phase 2 / Complete" in REQUIREMENTS.md traceability table but claimed by 03-01-PLAN. Investigation: REQUIREMENTS.md line 75 assigns POLL-01 to Phase 2 (likely from prior planning), while 03-01-PLAN.md frontmatter claims it. The actual implementation — `ModbusPoller` and `decode_registers` — exists and is correct. The traceability table discrepancy is a documentation inconsistency, not a code gap.

**STOR-04** is mapped to Phase 3 in REQUIREMENTS.md (line 81) but is NOT claimed in any Phase 3 plan's `requirements` field. STOR-04 ("Daemon logs InfluxDB write failures and continues polling") is actually implemented: `main.rs:119-125` handles `writer.write()` errors with `tracing::warn!` and continues. The implementation is present and correct; the gap is only that STOR-04 was implemented by Phase 2 (`02-02-PLAN.md` claims it at `influx.rs` level) and no Phase 3 plan re-claimed it in its frontmatter. **The behavior is satisfied by the wiring in main.rs.**

### Orphaned Requirement Check

| Requirement | REQUIREMENTS.md Phase | Claimed by Phase 3 Plan | Assessment |
|-------------|----------------------|-------------------------|------------|
| STOR-04 | Phase 3 | None (implemented in Phase 2 + Phase 3 wiring) | ⚠️ ORPHANED in Phase 3 plans (not claimed), but SATISFIED in code |

---

## Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| `src/main.rs` | 17, 23 | `expect("failed to install ...")` in `shutdown_signal()` | ℹ️ Info | These `expect` calls are correct — signal handler installation failure is a fatal programming error, not a runtime error. No stub concern. |
| `src/poller.rs` | 89-90 | `unwrap()` inside `#[ignore]` test | ℹ️ Info | In test-only code, behind `#[ignore]` gate. Not in production path. |

No TODO/FIXME/PLACEHOLDER/unimplemented! found in any production source file. No hardcoded empty arrays or empty returns. No module-level `#![allow(dead_code)]` remaining. All stubs from TDD RED phase replaced with real implementation.

---

## Human Verification Required

### 1. Single-Device Poll Against Real Hardware

**Test:** Connect a PZEM-016 to `/dev/ttyUSB0`, configure `config.toml` with its Modbus address, run `cargo run`, observe first log line after startup.  
**Expected:** `INFO poll success device="<name>"` appears within `poll_interval_secs` seconds; InfluxDB receives a data point with voltage ≈220-240V, frequency ≈50Hz, power_factor 0.0–1.0.  
**Why human:** Hardware required. The `test_poll_device_signature_compiles` test is `#[ignore]`-gated.

### 2. Multi-Device Sequential Poll

**Test:** Configure two `[[devices]]` entries with distinct Modbus addresses and names (e.g., `solar_panel` addr=1, `grid_meter` addr=2). Run daemon for one full interval.  
**Expected:** Two separate INFO poll-success log lines (one per device); InfluxDB has two measurements `solar_panel` and `grid_meter` each with a data point.  
**Why human:** Requires two physical PZEM-016 devices on the same RS485 bus.

### 3. Skip-and-Continue Resilience (POLL-03)

**Test:** With two devices configured, unplug or power-off device 2 mid-run.  
**Expected:** Every poll cycle: device 1 logs `INFO poll success`, device 2 logs `WARN device poll failed, skipping` with error context; daemon never exits or restarts; device 1 data continues flowing to InfluxDB.  
**Why human:** Requires deliberate hardware disruption; simulating a Modbus timeout in unit test is possible but verifying actual skip behavior with real serial framing requires live hardware.

### 4. SIGTERM Clean Exit Within 5 Seconds (OPS-01)

**Test:** Start daemon, let it enter the poll loop, send `kill -SIGTERM <pid>` mid-wait (between poll cycles).  
**Expected:** `INFO Shutdown signal received, exiting cleanly` logged, followed by `INFO rs485-logger stopped`; process exits with code 0 within 5 seconds.  
**Why human:** The `tokio::select!` + `tokio::pin!(shutdown)` code path is correct in source, but the <5s timing constraint and clean exit code require live process testing.

---

## Gaps Summary

No gaps. All 6 requirement IDs (POLL-01, POLL-02, POLL-03, OPS-01, OPS-02, OPS-03) are satisfied by the codebase.

**Code is complete and correct.** The phase goal is achieved: `tokio-modbus` RTU client is integrated, config → poller → writer is fully wired, skip-and-continue error handling is implemented, structured logging with `EnvFilter` and optional file appender is live, and graceful SIGTERM/SIGINT shutdown is wired via `tokio::select!` with a pinned signal future.

The 4 human verification items are behavioral confirmations against real hardware — they cannot be automated without a physical RS485 test rig.

---

_Verified: 2026-04-02_  
_Verifier: the agent (gsd-verifier)_
