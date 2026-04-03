---
phase: 07-daemon-reliability-hardening
verified: 2026-04-03T00:00:00Z
status: passed
score: 13/13 must-haves verified
re_verification: false
gaps: []
human_verification:
  - test: "CRIT-02 exit on 10 consecutive all-device failures — systemd restart"
    expected: "Process exits with code 0 after 10 polls where all devices fail; systemd Restart=always restarts it within RestartSec=5 seconds"
    why_human: "Cannot test without physical RS485 hardware to cause consecutive failures; would require mocking the ModbusPoller or running on a Pi with a disconnected serial port"
  - test: "HIGH-04 inter-device drain delay clears stale frames in practice"
    expected: "After a device times out, the 100ms sleep prevents the next device's response from being corrupted by leftover bytes in the serial buffer"
    why_human: "Behavioral correctness requires physical RS485 bus with timing measurement; unit tests cannot replicate serial buffer state"
---

# Phase 7: Daemon Reliability Hardening — Verification Report

**Phase Goal:** Fix all 14 findings from the daemon reliability verification report — eliminate daemon-hang modes, unrecoverable serial failures, config validation gaps, and operational hygiene issues so the daemon runs reliably on a Raspberry Pi indefinitely.
**Verified:** 2026-04-03T00:00:00Z
**Status:** PASSED
**Re-verification:** No — initial verification

---

## Goal Achievement

The phase goal was to fix 14 findings. Per `07-CONTEXT.md`, 12 were implemented (MED-03 and LOW-01 were explicitly deferred as "safe as-is per report" — not part of the fix scope). All 12 addressed findings have been verified against actual source code.

### Observable Truths (from CONTEXT.md Success Criteria)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | `cargo test` passes — all existing tests plus new validation tests | ✓ VERIFIED | 39 tests pass, 4 ignored (hardware); `cargo test` output confirmed |
| 2 | `cargo clippy -- -D warnings` clean | ✓ VERIFIED | `clippy` exits with `Finished` and no warnings |
| 3 | InfluxDB client has connect + request timeouts (CRIT-01, HIGH-01) | ✓ VERIFIED | `src/influx.rs:37-38`: `.connect_timeout(Duration::from_secs(5))` and `.timeout(Duration::from_secs(10))` |
| 4 | `config.toml` in `.gitignore`, `config.toml.example` exists (CRIT-03) | ✓ VERIFIED | `.gitignore:4` has `config.toml`; `config.toml.example` exists with `YOUR_INFLUXDB_TOKEN_HERE` placeholder |
| 5 | Device names validated at config load — reject spaces/commas/newlines (HIGH-02) | ✓ VERIFIED | `src/config.rs:82-91`: `chars().all(|c| c.is_alphanumeric() \|\| c == '_')` with empty-name check |
| 6 | Database name validated at config load (HIGH-03) | ✓ VERIFIED | `src/config.rs:66-70`: `chars().all(|c| c.is_alphanumeric() \|\| c == '_' \|\| c == '-')` |
| 7 | All-device-failure counter causes process exit after N consecutive failures (CRIT-02) | ✓ VERIFIED | `src/main.rs:157-335`: `consecutive_all_fail` counter + `MAX_CONSECUTIVE_ALL_FAIL=10` const + `break` on threshold |
| 8 | Post-timeout 100ms delay between device polls prevents stale Modbus frames (HIGH-04) | ✓ VERIFIED | `src/main.rs:307`: `tokio::time::sleep(Duration::from_millis(100)).await` in `Err(e)` arm |
| 9 | Log rotation enabled via `rolling::daily` (MED-01) | ✓ VERIFIED | `src/main.rs:102`: `tracing_appender::rolling::daily(dir, filename)` |
| 10 | InfluxDB failure state tracking — log first occurrence, suppress repeats (MED-04) | ✓ VERIFIED | `src/main.rs:164,284-294`: `influx_healthy: bool` flag with WARN-on-first-fail, INFO-on-restore |
| 11 | Energy reset timezone/time validated eagerly at config load (MED-05) | ✓ VERIFIED | `src/config.rs:93-99`: `er.timezone.parse::<chrono_tz::Tz>()` and `NaiveTime::parse_from_str` when `enabled=true` |
| 12 | System clock warning when timestamp < 2024-01-01 (LOW-02) | ✓ VERIFIED | `src/types.rs:7,54-61`: `static CLOCK_WARNED: AtomicBool` + `if timestamp_secs < 1_704_067_200` with once-guard |
| 13 | `far_future()` reduced to 10 years (MED-02) | ✓ VERIFIED | `src/main.rs:43`: `365 * 24 * 3600 * 10` (was `* 100`) |

**Score: 13/13 truths verified**

---

## Required Artifacts

| Artifact | Purpose | Exists | Substantive | Wired | Status |
|----------|---------|--------|-------------|-------|--------|
| `src/influx.rs` | HTTP client with timeouts; `InfluxWriter::new` returns `Result` | ✓ | ✓ (73 lines, real impl) | ✓ used in `main.rs:152` | ✓ VERIFIED |
| `src/config.rs` | Database + device name validation; energy reset eager parse | ✓ | ✓ (608 lines, full test suite) | ✓ called via `load_config()` in `main.rs:72` | ✓ VERIFIED |
| `src/main.rs` | Poll loop with `consecutive_all_fail`, `influx_healthy`, 100ms drain delay | ✓ | ✓ (360 lines, all three mechanisms present) | ✓ is the entry point | ✓ VERIFIED |
| `src/types.rs` | `CLOCK_WARNED` AtomicBool + timestamp < 2024 check | ✓ | ✓ (144 lines, static + conditional check) | ✓ called from `poller.rs` via `decode_registers` | ✓ VERIFIED |
| `.gitignore` | Contains `config.toml` to prevent token commits | ✓ | ✓ (5 lines; `config.toml` on line 4) | ✓ consumed by git | ✓ VERIFIED |
| `config.toml.example` | Template with `YOUR_INFLUXDB_TOKEN_HERE` placeholder | ✓ | ✓ (48 lines, full documented template) | ✓ standalone reference file | ✓ VERIFIED |

---

## Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `main.rs` | `InfluxWriter::new` | `src/influx.rs:35` returning `Result` | ✓ WIRED | `main.rs:152`: `let writer = InfluxWriter::new(&cfg.influxdb)?` |
| `main.rs` | `writer.write(&reading)` | InfluxDB state machine | ✓ WIRED | `main.rs:283-294`: `influx_healthy` gate + `Err`/`Ok` arms both handled |
| `main.rs` | `consecutive_all_fail` counter | `any_ok` flag per tick | ✓ WIRED | `main.rs:272-335`: `any_ok` set in `Ok(reading)` arm, checked after device loop |
| `main.rs` | Drain delay | 100ms sleep in `Err(e)` arm | ✓ WIRED | `main.rs:307`: inside `Err(e) =>` block, before `poller.bus_delay()` |
| `config.rs` | Device name validation | `validate_config()` | ✓ WIRED | `config.rs:82-91`: called by `load_config()` which is called by `main.rs:72` |
| `config.rs` | Database name validation | `validate_config()` | ✓ WIRED | `config.rs:66-70`: same call chain |
| `config.rs` | Energy reset eager parse | `validate_config()` when `enabled=true` | ✓ WIRED | `config.rs:93-99`: guarded by `if er.enabled` |
| `types.rs` | Clock warning | `CLOCK_WARNED.swap` | ✓ WIRED | `types.rs:56`: `AtomicBool::swap` prevents repeat; called by `decode_registers` |
| `main.rs` | `rolling::daily` file appender | `tracing_appender::rolling::daily` | ✓ WIRED | `main.rs:102`: inside `if let Some(ref log_path) = cfg.log_file` branch |

---

## Data-Flow Trace (Level 4)

Not applicable for this phase — no components rendering dynamic data. All changes are to daemon logic, validation, and operational infrastructure (not UI/API consumers of a data source).

---

## Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| All 39 unit tests pass | `cargo test` | 39 passed, 0 failed, 4 ignored | ✓ PASS |
| Clippy clean (no warnings) | `cargo clippy -- -D warnings` | `Finished` with no diagnostic output | ✓ PASS |
| DB name `power/test` rejected | `cargo test test_database_name_with_slash_rejected` | ok | ✓ PASS |
| DB name `my database` (space) rejected | `cargo test test_database_name_with_space_rejected` | ok | ✓ PASS |
| DB name `power?db=x` (injection) rejected | `cargo test test_database_name_with_question_mark_rejected` | ok | ✓ PASS |
| Valid DB names pass | `cargo test test_database_name_alphanumeric_underscore_dash_passes` | ok | ✓ PASS |
| Device name `solar panel` rejected | `cargo test test_device_name_with_space_rejected` | ok | ✓ PASS |
| Device name with comma rejected | `cargo test test_device_name_with_comma_rejected` | ok | ✓ PASS |
| Device name with newline rejected | `cargo test test_device_name_with_newline_rejected` | ok | ✓ PASS |
| Empty device name rejected | `cargo test test_device_name_empty_rejected` | ok | ✓ PASS |
| Bad timezone rejected when `enabled=true` | `cargo test test_invalid_timezone_rejected_when_enabled` | ok | ✓ PASS |
| Bad time format rejected when `enabled=true` | `cargo test test_invalid_time_format_rejected_when_enabled` | ok | ✓ PASS |
| Bad config silently ignored when `enabled=false` | `cargo test test_invalid_timezone_not_checked_when_disabled` | ok | ✓ PASS |
| `InfluxWriter::new` succeeds (returns `Result`) | `cargo test test_influx_writer_constructs` | ok | ✓ PASS |

---

## Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| REL-01 | ROADMAP.md (Phase 7 `**Requirements**:`) | Daemon reliability (not defined in REQUIREMENTS.md) | ⚠️ ORPHANED | `REL-01` is referenced in `ROADMAP.md:82` but has no definition in `REQUIREMENTS.md`. It does not appear in any plan's `requirements:` frontmatter field. The work itself clearly satisfies the implied reliability intent, but the requirement ID is formally undefined. |

**Note on REL-01:** The absence of a formal definition in REQUIREMENTS.md is a documentation gap, not an implementation gap. All 12 concrete findings mapped in `07-CONTEXT.md` have been fixed. The REQUIREMENTS.md should be updated to add a `REL-01` resilience requirement and mark it complete for Phase 7.

---

## Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| None | — | — | — | — |

No TODOs, FIXME markers, empty implementations, placeholder returns, or hardcoded stubs found in any phase-07 modified files.

---

## Human Verification Required

### 1. CRIT-02 — Exit-on-consecutive-failures + systemd restart

**Test:** On a Raspberry Pi with the daemon running, disconnect or power off all PZEM-016 devices (or physically unplug the RS485 USB adapter). Observe daemon behavior over 10 polling intervals.
**Expected:** After 10 consecutive all-fail cycles, the process logs `error ... "All devices failed 10 consecutive polls — exiting for systemd restart"` and exits. systemd `Restart=always` restarts it within `RestartSec=5` seconds. After restart, the daemon re-opens the serial port successfully if hardware is restored.
**Why human:** Cannot test without physical RS485 hardware. The code path that causes `break` from the poll loop is verified in source but the full lifecycle (exit → systemd restart → serial re-open) requires live hardware.

### 2. HIGH-04 — Drain delay clears stale Modbus frames in practice

**Test:** Configure two PZEM-016 devices. Cause device 1 to time out by powering it off mid-poll. Observe whether device 2 responds correctly on the same poll cycle.
**Expected:** Device 2 responds correctly to its query despite device 1 having just timed out. No "unexpected byte sequence" or CRC errors on device 2's response.
**Why human:** The 100ms delay is present in code (`main.rs:307`) but whether it is sufficient to drain stale bytes from the specific USB-RS485 adapter depends on the adapter's FIFO size and firmware behavior. Must be verified on actual hardware.

---

## Gaps Summary

No gaps. All 13 success criteria from `07-CONTEXT.md` are fully implemented and verified against the actual codebase:

- **CRIT-01 / HIGH-01:** `connect_timeout(5s)` + `timeout(10s)` in `InfluxWriter::new()` — confirmed in `src/influx.rs:37-38`
- **CRIT-02:** `consecutive_all_fail` counter with `MAX_CONSECUTIVE_ALL_FAIL=10` and `break` exit — confirmed in `src/main.rs:157-335`
- **CRIT-03:** `config.toml` gitignored, `config.toml.example` with placeholder token — confirmed in `.gitignore:4` and `config.toml.example:28`
- **HIGH-02:** Device name whitelist validation (alphanumeric + underscore) + empty-name check — confirmed in `src/config.rs:82-91`
- **HIGH-03:** Database name whitelist validation (alphanumeric + underscore + dash) — confirmed in `src/config.rs:66-70`
- **HIGH-04:** 100ms drain delay in `Err(e)` arm before `bus_delay()` — confirmed in `src/main.rs:307`
- **MED-01:** `rolling::daily` log appender — confirmed in `src/main.rs:102`
- **MED-02:** `far_future()` reduced to `* 10` (10 years) — confirmed in `src/main.rs:43`
- **MED-04:** `influx_healthy` state machine suppressing repeated write-failure logs — confirmed in `src/main.rs:164,284-294`
- **MED-05:** Energy reset timezone + time eagerly parsed when `enabled=true` — confirmed in `src/config.rs:93-99`
- **LOW-02:** `CLOCK_WARNED: AtomicBool` + `timestamp_secs < 1_704_067_200` once-guard — confirmed in `src/types.rs:7,54-61`
- **MED-03 / LOW-01:** Explicitly deferred in `07-CONTEXT.md` as "not needed" — not implementation gaps.

**Documentation gap (non-blocking):** `REL-01` is referenced in `ROADMAP.md:82` but is not defined in `REQUIREMENTS.md` and not claimed in any plan frontmatter. The requirement ID is orphaned. The phase goal is fully achieved in code; only the traceability document needs updating.

---

_Verified: 2026-04-03T00:00:00Z_
_Verifier: the agent (gsd-verifier)_
