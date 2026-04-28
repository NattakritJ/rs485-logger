# Phase 4: Per-Device Poll Interval Override - Context

**Gathered:** 2026-04-28
**Status:** Ready for planning

<domain>
## Phase Boundary

Allow individual `[[devices]]` entries in TOML to override the global poll interval. Devices without a per-device interval continue to use the global interval unchanged. The RS485 bus remains serial (one device at a time); polling is still sequential but the scheduler is extended to check which devices are "due" on each tick.

This phase is config-schema + scheduler loop changes only — no new Modbus commands, no new data fields, no changes to InfluxDB writes or energy reset logic.

</domain>

<decisions>
## Implementation Decisions

### Config Schema
- **D-01:** Add `poll_interval_ms: Option<u64>` to `DeviceConfig` in `src/config.rs`. This field is optional — devices without it inherit the resolved global interval.
- **D-02:** Field name is `poll_interval_ms`, matching the global field name for consistency. No `poll_interval_secs` variant at the device level.
- **D-03:** Validate per-device `poll_interval_ms` in `validate_config`: if present, must be > 0 (same rule as global).

### Scheduler Design — Tick-at-Minimum + Per-Device Due Check
- **D-04:** Replace the current "poll all → sleep global_interval → repeat" loop with a **tick-at-minimum + per-device due check** approach:
  - Compute `tick_interval_ms = min(global_interval_ms, min(device.poll_interval_ms for all devices that have an override))`. This is the main loop's tick cadence.
  - Track `last_polled_at: Vec<Instant>` (indexed parallel to `cfg.devices`) — initialised so all devices are "due immediately" on the first tick.
  - Each tick: iterate all devices; for each device, compute its effective interval (`device.poll_interval_ms.unwrap_or(global_interval_ms)`); if `now >= last_polled_at[i] + effective_interval`, poll it and update `last_polled_at[i]`.
  - Only devices that are due get polled on a given tick — Modbus traffic is proportional to need.
- **D-05:** `tick_interval_ms` is computed once at startup after config is resolved. If all devices inherit the global interval, `tick_interval_ms == global_interval_ms` and behaviour is identical to Phase 3 (no regression).

### Sub-500ms Warning
- **D-06:** The existing `tracing::warn!` for intervals < 500ms (added in Phase 3, D-08) applies **only to the global interval**, not to per-device `poll_interval_ms`. Operators who set a short per-device interval are assumed to know what they're doing.

### Global Interval Semantics
- **D-07:** The global interval (`poll_interval_ms` / `poll_interval_secs` at the root config level) remains the **default** — devices without a per-device override inherit it. Global interval is still required or defaults to 10,000ms (10s). No change to existing global interval resolution logic from Phase 3.
- **D-08:** When all devices have `poll_interval_ms` set, the global interval still controls the tick cadence as the baseline minimum unless device intervals are all shorter.

### Cycle-Too-Long Warning (Agent's Discretion)
- **D-09:** Adapt the existing cycle-too-long warning to compare the per-tick poll duration against `tick_interval_ms` (the minimum across all device intervals). The warning message should note the tick interval, not the global interval. If the tick interval is the same as the global interval (no per-device overrides), behaviour is identical to Phase 3.
  - Concretely: after the per-tick polling pass completes, compare elapsed time against `tick_interval_ms` and warn if exceeded.

### Agent's Discretion
- Whether to use `Vec<Instant>` (parallel to `cfg.devices`) or `HashMap<u8, Instant>` (keyed by Modbus address) for `last_polled_at` tracking — Vec is simpler given devices are iterated by index anyway.
- Whether the startup log emits per-device effective intervals or just a summary line noting how many overrides are active.
- Whether the "all devices failed N consecutive cycles" counter (CRIT-02) counts ticks or actual polls (since not all devices are polled every tick when intervals differ) — the intent is to exit when all devices consistently fail to respond, so counting actual poll failures per device and triggering exit when every device has failed its own consecutive-poll threshold is more accurate but may be overkill; a simpler pass-level counter is acceptable.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

No external specs for this phase — requirements fully captured in decisions above.

### Existing Implementation
- `src/config.rs` — `DeviceConfig` struct (add `poll_interval_ms: Option<u64>`); `validate_config` (add per-device interval > 0 check)
- `src/main.rs` — poll loop (lines ~278–410); `poll_interval_ms` resolution (line ~138); cycle-too-long warning (line ~402); startup log (line ~152)
- `config.toml` — `[[devices]]` blocks (add commented `poll_interval_ms` example)
- `config.toml.example` — same update

### Prior Phase Context
- Phase 3 CONTEXT.md (`03-CONTEXT.md`) — established `poll_interval_ms` naming and `Option<u64>` + `unwrap_or` pattern; sub-500ms WARN threshold at global level; resolved `poll_interval_ms` threaded through main
- Phase 1 CONTEXT.md (`01-CONTEXT.md`) — established `read_timeout_ms: Option<u64>` optional-override config pattern

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `Option<u64>` + `unwrap_or(sensible_default)` config pattern — already used for `read_timeout_ms`, `poll_interval_ms`, `parity`; follow exact same pattern for `DeviceConfig.poll_interval_ms`
- `validate_config` per-field checks — add `device.poll_interval_ms > 0` check alongside the existing per-device `address` and `name` checks (lines 81–98 of `src/config.rs`)
- Cycle timer infrastructure (`cycle_start = Instant::now()` + `cycle_ms = cycle_start.elapsed().as_millis()`) already in place in main loop — reuse for per-tick comparison

### Established Patterns
- Config optional fields: `Option<T>` never `#[serde(default)]` magic
- All validation in `validate_config`, not in `main()`
- Structured log fields at startup: `tracing::info!(field = value, "message")`
- `DeviceConfig` is `Clone` — this must be preserved for the per-device due-check iteration

### Integration Points
- `for device in &cfg.devices` loop at ~line 319 in `src/main.rs` — this becomes the per-tick filtered loop
- `tokio::time::interval(Duration::from_millis(poll_interval_ms))` ticker at ~line 195 — change to use `tick_interval_ms`
- CRIT-02 consecutive-fail counter at ~line 388 — may need adjustment if not all devices are polled every tick

</code_context>

<specifics>
## Specific Ideas

- The tick-at-minimum approach means zero regression for configs without per-device intervals: `tick_interval_ms` equals the global interval, the due-check always fires for every device, and behaviour is identical to Phase 3.
- `last_polled_at` initialised to `Instant::now() - Duration::from_millis(u64::MAX / 2)` (or simply `Instant::now() - device_interval`) so all devices are due immediately on the first tick rather than waiting one full interval before the first read.
- Per-device interval example in config.toml.example (commented out):
  ```toml
  [[devices]]
  address = 1
  name = "floor_1"
  # poll_interval_ms = 500   # Optional: override global interval for this device
  ```

</specifics>

<deferred>
## Deferred Ideas

- Dynamic interval adjustment based on measured cycle duration — interesting but adds complexity
- Per-device consecutive-fail threshold (separate from global CRIT-02) — could allow one slow device to not drag down the exit timer; worth revisiting in a future phase
- `poll_interval_secs` at device level — not needed; `poll_interval_ms` covers all cases

</deferred>

---

*Phase: 04-per-device-poll-interval-override*
*Context gathered: 2026-04-28*
