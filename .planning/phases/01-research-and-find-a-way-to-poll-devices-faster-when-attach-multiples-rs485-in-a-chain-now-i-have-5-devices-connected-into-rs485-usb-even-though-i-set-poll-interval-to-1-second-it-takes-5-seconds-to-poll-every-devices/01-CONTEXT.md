# Phase 1: Poll Speed Optimization - Context

**Gathered:** 2026-04-09
**Status:** Ready for planning

<domain>
## Phase Boundary

Reduce the effective time to complete one full poll cycle across all PZEM-016 devices on a single RS485 bus. The goal is to make the cycle duration fit within the configured `poll_interval_secs` — currently 5 devices × (500ms timeout + 100ms IFD) ≈ 3–5s per cycle even when `poll_interval_secs = 1`.

This phase is software-only. No hardware changes. No new capabilities (no multiple serial ports, no new data fields, no architectural rewrites). The RS485 half-duplex constraint is physical and not addressed here — devices remain polled sequentially.

</domain>

<decisions>
## Implementation Decisions

### Modbus Read Timeout
- **D-01:** Reduce per-device read timeout from 500ms to **150ms** for FC 0x04 (read input registers). PZEM-016 responds in <50ms at 9600 baud; 150ms gives 3x headroom.
- **D-02:** Energy reset timeout (FC 0x42) stays at **500ms** (hardcoded). The PZEM-016 performs an internal EEPROM write on reset — it needs the full margin.
- **D-03:** Read timeout is **configurable** via `read_timeout_ms` field in the `[serial]` config section. Default: 150ms. This allows tuning on hardware without a recompile.

### Inter-Frame Delay (IFD)
- **D-04:** Split the currently flat `INTER_FRAME_DELAY = 100ms` into two named constants:
  - `INTER_FRAME_DELAY_READ = 30ms` — applied after successful or failed read transactions (FC 0x04)
  - `INTER_FRAME_DELAY_RESET = 100ms` — applied after energy reset transactions (FC 0x42)
- **D-05:** These are **code constants** in `src/poller.rs`, not config values. The split is well-justified by the PZEM-016 datasheet difference between reads and EEPROM writes.
- **D-06:** The `bus_delay()` method on `ModbusPoller` should accept a parameter (or be replaced by two methods) so callers explicitly choose the correct delay.

### Interval Semantics
- **D-07:** Keep the existing `tokio::time::interval` with `MissedTickBehavior::Skip`. Tick-based scheduling is correct — with the timeout + IFD tuning, a 5-device cycle should be ~900ms, fitting within a 1s tick.
- **D-08:** Add a **cycle duration warning**: measure wall-clock time for each full poll cycle (all devices). If cycle duration exceeds `poll_interval_secs`, log at WARN level:
  `"Poll cycle took Xms, exceeds configured interval of Ys — consider reducing read_timeout_ms or device count"`
- **D-09:** The cycle timer starts at the top of the tick arm and stops after all devices are polled and InfluxDB writes are attempted.

### Multiple USB Adapters
- **D-10:** Out of scope for this phase. Software-only optimizations (D-01 through D-09) are expected to bring the 5-device cycle well under 1.5s — no hardware change needed.

### the agent's Discretion
- Exact location of cycle timer (before or after `any_ok` check)
- Whether to use `std::time::Instant` or `tokio::time::Instant` for cycle measurement
- How to expose `read_timeout_ms` with a sensible default if absent from config (Option<u64> with fallback)

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

No external specs for this phase — requirements are fully captured in decisions above.

### Existing Implementation
- `src/poller.rs` — `ModbusPoller` struct, `poll_device`, `reset_energy`, `bus_delay`, `INTER_FRAME_DELAY` constant
- `src/main.rs` — poll loop (ticker arm), energy reset arm, `MissedTickBehavior::Skip` setup
- `src/config.rs` — `SerialConfig` struct (add `read_timeout_ms: Option<u64>` here)

### Hardware Reference
- PZEM-016 Modbus RTU: 9600 baud, 8N1, FC 0x04 for 10 registers at 0x0000, FC 0x42 for energy reset
- Typical response latency: 30–80ms for reads; EEPROM write for resets requires more time

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `ModbusPoller` (`src/poller.rs:33`) — add `INTER_FRAME_DELAY_READ` and `INTER_FRAME_DELAY_RESET` constants; update `bus_delay()` to accept an enum or bool, or split into two methods
- `SerialConfig` (`src/config.rs:26`) — add `read_timeout_ms: Option<u64>` field with a default of 150
- Poll loop tick arm (`src/main.rs:295`) — add `std::time::Instant::now()` before the device loop and WARN log after

### Established Patterns
- Skip-and-log on device error: `tracing::warn!` + continue loop (do NOT change this)
- `INTER_FRAME_DELAY` as a `const Duration` in `poller.rs` — same pattern for new constants
- Config defaults via `Option<T>` with `unwrap_or(default)` — follow this for `read_timeout_ms`
- `tokio::time::timeout(Duration, future)` wrapping Modbus calls — change the `Duration::from_millis(500)` literal to use `cfg.serial.read_timeout_ms`

### Integration Points
- `poll_device` in `src/poller.rs:68` — change `Duration::from_millis(500)` to a parameter or read from config passed into `ModbusPoller::new`
- `reset_energy` in `src/poller.rs:103` — keep `Duration::from_millis(500)` hardcoded (D-02)
- `bus_delay` in `src/poller.rs:57` — needs to differentiate read vs reset delay (D-06)
- Callers of `bus_delay()` in `src/main.rs:156, 277, 339` — must pass the correct delay type

</code_context>

<specifics>
## Specific Ideas

- The timeout change from 500ms → 150ms is the highest-impact single change: saves 350ms × 5 devices = 1.75s per cycle in the timeout-limited case.
- The IFD split saves 70ms × 5 devices = 350ms per cycle in the normal (non-timeout) case.
- Combined: a 5-device cycle currently ~3–5s should drop to ~900ms–1.1s, fitting within a 1s poll interval.
- `read_timeout_ms` default should be 150ms. If a user's devices need more time they can raise it in config — avoids a recompile.

</specifics>

<deferred>
## Deferred Ideas

- Multiple USB-RS485 adapters for parallel polling — hardware change, belongs in a future phase if software tuning is insufficient
- Per-device timeout override in `DeviceConfig` — not needed now; global `read_timeout_ms` in `[serial]` is sufficient
- Configurable inter-frame delays — two code constants are sufficient; making them config adds complexity without clear user benefit

</deferred>

---

*Phase: 01-poll-speed-optimization*
*Context gathered: 2026-04-09*
