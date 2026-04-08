---
phase: 02-revisit-polling-slowlyness
plan: 01
subsystem: polling
tags: [tokio, async, modbus, influxdb, atomic, arc, fire-and-forget]

# Dependency graph
requires:
  - phase: 01-poll-speed-optimization
    provides: "tokio multi_thread runtime, split inter-frame delays, read_timeout_ms"
provides:
  - "InfluxWriter derives Clone (O(1) reqwest::Client arc clone)"
  - "Fire-and-forget InfluxDB writes via tokio::spawn — poll loop no longer blocked by HTTP latency"
  - "Arc<AtomicBool> influx_healthy shared between poll loop and spawned write tasks"
  - "MED-04 health state transition logging preserved in spawned tasks"
affects:
  - 02-revisit-polling-slowlyness

# Tech tracking
tech-stack:
  added: ["std::sync::atomic::{AtomicBool, Ordering}", "std::sync::Arc"]
  patterns:
    - "Fire-and-forget tokio::spawn pattern for decoupling I/O from the main loop"
    - "Arc<AtomicBool> for sharing boolean health flags across async tasks"
    - "health.swap(false, Relaxed) returns previous value — used for first-failure WARN suppression"

key-files:
  created: []
  modified:
    - src/influx.rs
    - src/main.rs
    - src/poller.rs

key-decisions:
  - "Fire-and-forget tokio::spawn (not channel or semaphore) — Pi has 4GB RAM, each task is trivial (<100 bytes)"
  - "Arc<AtomicBool> (not Mutex<bool>) — atomic swap/load sufficient, no critical section needed"
  - "health.swap(false, Relaxed) for first-failure detection — returns previous value, eliminates compare_exchange"
  - "reqwest::Client is Arc-based internally so #[derive(Clone)] on InfluxWriter is O(1)"

patterns-established:
  - "tokio::spawn fire-and-forget: clone writer + Arc::clone health + move reading into async block"
  - "AtomicBool::swap pattern for first-event detection without Mutex"

requirements-completed: [D-01, D-02, D-03, D-04, D-05, D-06, D-07, D-08]

# Metrics
duration: 2min
completed: 2026-04-09
---

# Phase 02 Plan 01: Decouple InfluxDB Writes Summary

**InfluxDB HTTP writes decoupled from Modbus poll loop via tokio::spawn fire-and-forget, eliminating ~630ms-per-device HTTP blocking — expected cycle time drops from ~5000ms to ~450ms for 5 devices**

## Performance

- **Duration:** 2 min
- **Started:** 2026-04-08T22:31:59Z
- **Completed:** 2026-04-09T22:33:59Z
- **Tasks:** 1
- **Files modified:** 3

## Accomplishments

- `InfluxWriter` now derives `Clone` — `reqwest::Client` is Arc-based, so clone is O(1) reference count increment
- Poll loop spawns fire-and-forget write tasks via `tokio::spawn(async move { ... })` — immediately continues to next device after Modbus read
- `influx_healthy: bool` replaced with `Arc<AtomicBool>` shared between main poll loop and spawned write tasks
- MED-04 health suppression behavior fully preserved: WARN on first failure, silent while unhealthy, INFO on restore

## Task Commits

Each task was committed atomically:

1. **Task 1: Add Clone derive to InfluxWriter + Arc<AtomicBool> health flag + fire-and-forget writes in main.rs** - `0816bca` (feat)

**Plan metadata:** `(pending)` (docs: complete plan)

## Files Created/Modified

- `src/influx.rs` — Added `#[derive(Clone)]` to `InfluxWriter` struct
- `src/main.rs` — Added `use std::sync::atomic::{AtomicBool, Ordering}`, `use std::sync::Arc`; replaced `let mut influx_healthy = true` with `Arc::new(AtomicBool::new(true))`; replaced inline `writer.write(&reading).await` with `tokio::spawn` fire-and-forget block
- `src/poller.rs` — Fixed missing `parity: None` field in two test helper `SerialConfig` initializers (Rule 3 auto-fix)

## Decisions Made

- Used `health.swap(false, Ordering::Relaxed)` instead of `compare_exchange` — swap returns the previous value, which naturally detects the first failure transition (true→false) vs already-unhealthy (false→false) in a single atomic operation
- Chose unbounded spawns (no semaphore/channel cap) per D-02 — Pi has 4GB RAM with 288MB used; each task holds ~6 f64s + a String, trivially small
- Chose `#[derive(Clone)]` on `InfluxWriter` over `Arc<InfluxWriter>` per D-04 — more ergonomic and semantically clear (clone is shallow due to Arc-based reqwest::Client)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed missing `parity` field in poller.rs test helpers**
- **Found during:** Task 1 (compilation step)
- **Issue:** `SerialConfig` struct in `src/config.rs` already had `parity: Option<String>` added (part of plan 02-02 scope executed first or in a parallel run), but the two `#[ignore]` test helpers in `src/poller.rs` were still constructing `SerialConfig` without the new field — causing `error[E0063]: missing field 'parity'`
- **Fix:** Added `parity: None` to both `SerialConfig` struct initializers in `src/poller.rs` test helpers
- **Files modified:** `src/poller.rs`
- **Verification:** `cargo test` passes with 47 tests passing
- **Committed in:** `0816bca` (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (Rule 3 blocking)
**Impact on plan:** Necessary to fix compilation — `parity` field was already present in the struct definition, test helpers hadn't been updated yet.

## Issues Encountered

None — plan executed cleanly after the compilation fix.

## User Setup Required

None — no external service configuration required. This is a code change only; the daemon binary is self-contained.

## Next Phase Readiness

- Plan 02 (02-02-PLAN.md: CFG-02 parity field + RISK-1 udev fix) is ready to execute
- Expected observable improvement: 5-device poll cycle drops from ~5000ms to ~450ms once deployed to Raspberry Pi
- No changes to config format in this plan — existing `config.toml` files continue to work without modification

---
*Phase: 02-revisit-polling-slowlyness*
*Completed: 2026-04-09*
