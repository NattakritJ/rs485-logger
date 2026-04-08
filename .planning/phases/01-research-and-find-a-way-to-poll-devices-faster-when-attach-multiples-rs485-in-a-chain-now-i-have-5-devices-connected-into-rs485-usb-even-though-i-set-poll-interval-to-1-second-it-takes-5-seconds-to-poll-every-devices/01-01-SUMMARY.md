---
phase: 01-poll-speed-optimization
plan: 01
subsystem: infra
tags: [modbus, rs485, tokio, serial, polling, timeout, pzem-016]

# Dependency graph
requires: []
provides:
  - "Configurable Modbus read timeout (default 150ms) via read_timeout_ms in [serial] TOML config"
  - "Split inter-frame delay: 30ms after FC 0x04 reads, 100ms after FC 0x42 resets"
  - "Cycle duration timer with WARN log when poll exceeds configured interval"
  - "5-device poll cycle theoretical time reduced from ~3000ms to ~900ms"
affects: [poller, config, main-loop, deployment]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Split IFD constants: INTER_FRAME_DELAY_READ (30ms) vs INTER_FRAME_DELAY_RESET (100ms)"
    - "Configurable timeout stored in ModbusPoller struct, set from config at construction"
    - "Cycle timer pattern: Instant::now() at arm entry, elapsed().as_millis() at exit"

key-files:
  created: []
  modified:
    - src/config.rs
    - src/poller.rs
    - src/main.rs
    - config.toml

key-decisions:
  - "D-01: Reduce read timeout 500ms → 150ms default (configurable via read_timeout_ms)"
  - "D-02: Energy reset (FC 0x42) timeout stays hardcoded at 500ms — EEPROM write needs margin"
  - "D-03: read_timeout_ms is Option<u64> in SerialConfig; None → 150ms default via unwrap_or"
  - "D-04/D-05: Split INTER_FRAME_DELAY into READ (30ms) and RESET (100ms) constants"
  - "D-06: bus_delay() split into bus_delay_read() and bus_delay_reset() methods"
  - "D-08/D-09: Add cycle_start timer; WARN when cycle_ms > interval_ms * 1000"

patterns-established:
  - "bus_delay_read() after FC 0x04 polls; bus_delay_reset() after FC 0x42 energy resets"
  - "read_timeout_ms = None in all SerialConfig struct literals in tests"

requirements-completed: [D-01, D-02, D-03, D-04, D-05, D-06, D-07, D-08, D-09]

# Metrics
duration: 4min
completed: 2026-04-08
---

# Phase 01 Plan 01: Poll Speed Optimization Summary

**RS485 poll cycle reduced from ~3000ms to ~900ms: configurable 150ms read timeout, split 30ms/100ms inter-frame delays, and cycle duration WARN logging**

## Performance

- **Duration:** 4 min
- **Started:** 2026-04-08T20:53:32Z
- **Completed:** 2026-04-08T20:57:44Z
- **Tasks:** 2
- **Files modified:** 4 (src/config.rs, src/poller.rs, src/main.rs, config.toml)

## Accomplishments

- Configurable Modbus read timeout: `read_timeout_ms` option in `[serial]` TOML config (default 150ms via `unwrap_or(150)`); `reset_energy` FC 0x42 stays hardcoded at 500ms
- Split `INTER_FRAME_DELAY` (100ms) into `INTER_FRAME_DELAY_READ` (30ms for FC 0x04) and `INTER_FRAME_DELAY_RESET` (100ms for FC 0x42); split `bus_delay()` into `bus_delay_read()` and `bus_delay_reset()`
- Cycle timer: `cycle_start = Instant::now()` at ticker arm entry; WARN log fires when `cycle_ms > interval_secs * 1000`
- All 3 `bus_delay()` call sites in `main.rs` replaced with semantically correct variants
- 41/41 tests pass (2 new tests for `read_timeout_ms` parsing)
- Theoretical 5-device cycle: 5 × (150ms + 30ms) = **900ms** vs 5 × (500ms + 100ms) = 3000ms

## Task Commits

Each task was committed atomically:

1. **Task 1: Config + Poller — add read_timeout_ms, split IFD, update signatures** - `f382bcd` (feat)
2. **Task 2: Main loop — wire bus_delay split, add cycle timer, update config.toml** - `4531cd3` (feat)

## Files Created/Modified

- `src/config.rs` - Added `read_timeout_ms: Option<u64>` to `SerialConfig`; 2 new tests; 4 struct literal fixes
- `src/poller.rs` - New `INTER_FRAME_DELAY_READ`/`RESET` constants; `read_timeout` field in `ModbusPoller`; `bus_delay_read()`/`bus_delay_reset()` methods; `poll_device` uses `self.read_timeout`
- `src/main.rs` - 3 `bus_delay()` → `bus_delay_read()`/`bus_delay_reset()` replacements; `cycle_start` timer + WARN log added
- `config.toml` - Documented `read_timeout_ms = 150` (commented out) in `[serial]` section (gitignored, updated on disk)

## Decisions Made

- `read_timeout_ms` is `Option<u64>` not `u64` — allows backwards-compatible config without breaking existing `config.toml` files that lack the field
- `reset_energy` timeout hardcoded at 500ms per D-02: PZEM-016 EEPROM write requires consistent generous timeout regardless of user config
- Split bus_delay into two named methods rather than a parameter — call site semantics are explicit (`bus_delay_reset` is self-documenting at energy reset sites)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Extra closing braces from edit collisions in config.rs**
- **Found during:** Task 1 (config.rs edits)
- **Issue:** Multiple `SerialConfig` struct literal replacements each accidentally inserted an extra `}` after the function body, causing compiler errors
- **Fix:** Removed the extra closing braces from `make_cfg_with_database`, `make_cfg_with_device`, and `make_cfg_with_energy_reset` helper functions
- **Files modified:** src/config.rs
- **Verification:** `cargo test` passed with 41 tests after fix
- **Committed in:** f382bcd (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (Rule 1 — edit collision bug)
**Impact on plan:** Minor formatting issue from edit tooling; fixed immediately. No scope creep.

## Issues Encountered

- `cargo test --lib` fails for binary-only crates (no library target); used `cargo test` instead — same results, all inline `#[cfg(test)]` modules run correctly
- `config.toml` is gitignored (correct — contains real credentials); `read_timeout_ms` documentation was added to the file on disk but not committed

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Poll speed optimization is complete; 5-device cycle now fits within 1s interval (900ms theoretical)
- `read_timeout_ms` can be tuned in `config.toml` without recompiling if a user's cable runs cause slower device responses
- HIGH-04 drain delay (100ms on error path) preserved — no regression
- All existing tests pass; 2 new config tests added for `read_timeout_ms`

---
*Phase: 01-poll-speed-optimization*
*Completed: 2026-04-08*

## Self-Check: PASSED

- FOUND: src/config.rs
- FOUND: src/poller.rs
- FOUND: src/main.rs
- FOUND: 01-01-SUMMARY.md
- FOUND: f382bcd (Task 1 commit)
- FOUND: 4531cd3 (Task 2 commit)
