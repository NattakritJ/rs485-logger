# Phase 4: Per-Device Poll Interval Override - Research

**Researched:** 2026-04-28
**Domain:** Rust config schema + async scheduler loop modification
**Confidence:** HIGH

## Summary

This phase extends the rs485-logger daemon to allow individual `[[devices]]` entries to override the global poll interval. The change is scoped to two areas: (1) config schema + validation in `src/config.rs`, and (2) the poll loop in `src/main.rs` — transforming it from "poll all → sleep" to "tick at minimum interval → poll only due devices."

The codebase already has well-established patterns for `Option<u64>` config fields with `unwrap_or` defaults (see `read_timeout_ms`, `poll_interval_ms`). The poll loop at lines 243–412 of `main.rs` uses `tokio::time::interval` with a `select!` for shutdown/reset/tick — the tick arm needs restructuring but the surrounding infrastructure remains unchanged.

**Primary recommendation:** Follow the existing `Option<u64>` pattern exactly. Add `poll_interval_ms: Option<u64>` to `DeviceConfig`, compute `tick_interval_ms` once at startup, and add a `Vec<std::time::Instant>` parallel to `cfg.devices` for `last_polled_at` tracking. The transformation is mechanical and low-risk.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- **D-01:** Add `poll_interval_ms: Option<u64>` to `DeviceConfig` in `src/config.rs`. Optional — devices without it inherit global interval.
- **D-02:** Field name is `poll_interval_ms`, matching the global field name.
- **D-03:** Validate per-device `poll_interval_ms` in `validate_config`: if present, must be > 0.
- **D-04:** Replace "poll all → sleep" loop with tick-at-minimum + per-device due check. Track `last_polled_at: Vec<Instant>`. Each tick: iterate devices, compute effective interval, poll only if due.
- **D-05:** `tick_interval_ms` computed once at startup. If no overrides, behaviour identical to Phase 3.
- **D-06:** Sub-500ms warning applies only to global interval, not per-device.
- **D-07:** Global interval remains the default. No change to global interval resolution logic.
- **D-08:** Global interval still participates in tick cadence calculation as baseline.
- **D-09:** Adapt cycle-too-long warning to compare per-tick duration against `tick_interval_ms`.

### Agent's Discretion
- `Vec<Instant>` vs `HashMap<u8, Instant>` for `last_polled_at` — Vec recommended (simpler, devices iterated by index)
- Startup log format for per-device intervals — summary line recommended
- CRIT-02 counter semantics (tick-based vs per-device poll-based) — pass-level counter acceptable

### Deferred Ideas (OUT OF SCOPE)
- Dynamic interval adjustment based on cycle duration
- Per-device consecutive-fail threshold
- `poll_interval_secs` at device level
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| D-01 | Add `poll_interval_ms: Option<u64>` to `DeviceConfig` | Config.rs analysis shows exact insertion point (line 46-49), established `Option<u64>` pattern |
| D-02 | Field name `poll_interval_ms` | Matches global field; serde deserializes automatically from TOML |
| D-03 | Validate per-device interval > 0 | validate_config device loop at lines 81-98 — add check alongside existing address/name checks |
| D-04 | Tick-at-minimum + per-device due check | Poll loop at lines 312-411 — restructure tick arm with `last_polled_at` Vec |
| D-05 | Compute `tick_interval_ms` at startup | Insert after line 140 (global interval resolution) |
| D-06 | Sub-500ms warning only for global | Warning at lines 143-149 — no change needed, already scoped to global |
| D-07 | Global interval semantics unchanged | Resolution at lines 139-140 — untouched |
| D-08 | Global interval in tick cadence | tick_interval_ms = min(global, min(device overrides)) |
| D-09 | Cycle-too-long warning adapted | Warning at lines 401-410 — change `poll_interval_ms` to `tick_interval_ms` |
</phase_requirements>

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Per-device interval config | Config parsing (`config.rs`) | — | Serde deserialization + validation |
| Tick interval computation | Main loop setup (`main.rs`) | — | Computed once after config load |
| Per-device due check | Poll loop (`main.rs`) | — | Runtime scheduling decision per tick |
| CRIT-02 adaptation | Poll loop (`main.rs`) | — | Failure tracking per cycle |

## Current Implementation Analysis

### 1. DeviceConfig Struct (`src/config.rs`, lines 45-49)

```rust
#[derive(Debug, serde::Deserialize, Clone)]
pub struct DeviceConfig {
    pub address: u8,
    pub name: String,
}
```

Only two fields. Adding `poll_interval_ms: Option<u64>` is trivial — serde handles `Option` as "present or absent in TOML" automatically. The `Clone` derive is already present (needed for the poll loop). [VERIFIED: codebase]

### 2. validate_config Device Loop (`src/config.rs`, lines 81-98)

```rust
for device in &cfg.devices {
    anyhow::ensure!(
        device.address >= 1 && device.address <= 247,
        "device '{}' has invalid Modbus address {} (must be 1–247)",
        device.name, device.address
    );
    anyhow::ensure!(!device.name.is_empty(), ...);
    anyhow::ensure!(device.name.chars().all(|c| c.is_alphanumeric() || c == '_'), ...);
}
```

Add `if let Some(ms) = device.poll_interval_ms { ensure!(ms > 0, ...) }` inside this loop. Follows the exact pattern used for global `poll_interval_ms` at line 64-66. [VERIFIED: codebase]

### 3. Global Interval Resolution (`src/main.rs`, lines 139-140)

```rust
let poll_interval_ms: u64 = cfg.poll_interval_ms
    .unwrap_or_else(|| cfg.poll_interval_secs.unwrap_or(10) * 1000);
```

This stays unchanged. After this line, add `tick_interval_ms` computation. [VERIFIED: codebase]

### 4. Ticker Setup (`src/main.rs`, lines 194-201)

```rust
let mut ticker = tokio::time::interval(
    std::time::Duration::from_millis(poll_interval_ms),
);
ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
```

Change `poll_interval_ms` → `tick_interval_ms` in the Duration. Keep `MissedTickBehavior::Skip`. [VERIFIED: codebase]

### 5. Poll Loop Tick Arm (`src/main.rs`, lines 312-411)

Current structure:
```rust
_ = ticker.tick() => {
    let cycle_start = std::time::Instant::now();
    let mut any_ok = false;
    for device in &cfg.devices {           // <-- polls ALL devices every tick
        match poller.poll_device(device).await {
            Ok(reading) => { any_ok = true; /* write to influx */ }
            Err(e) => { /* warn + 100ms drain delay */ }
        }
        poller.bus_delay_read().await;
    }
    // CRIT-02 check
    // cycle-too-long warning
}
```

This needs to become:
```rust
_ = ticker.tick() => {
    let cycle_start = std::time::Instant::now();
    let now = std::time::Instant::now();
    let mut any_ok = false;
    let mut any_polled = false;
    for (i, device) in cfg.devices.iter().enumerate() {
        let effective_interval = device.poll_interval_ms.unwrap_or(poll_interval_ms);
        if now.duration_since(last_polled_at[i]) < Duration::from_millis(effective_interval) {
            continue;  // not due yet
        }
        any_polled = true;
        match poller.poll_device(device).await {
            Ok(reading) => { any_ok = true; last_polled_at[i] = now; /* write to influx */ }
            Err(e) => { last_polled_at[i] = now; /* warn + drain delay */ }
        }
        poller.bus_delay_read().await;
    }
    // CRIT-02: only count if at least one device was polled and all polled devices failed
    // cycle-too-long: compare against tick_interval_ms
}
```

### 6. CRIT-02 Counter (`src/main.rs`, lines 379-398)

```rust
if any_ok {
    consecutive_all_fail = 0;
} else {
    consecutive_all_fail += 1;
    // ... warn and break at MAX_CONSECUTIVE_ALL_FAIL
}
```

**Key consideration:** With per-device intervals, not all devices are polled every tick. A tick where no devices are due should NOT increment the failure counter. Only ticks where at least one device was polled and all polled devices failed should count. [VERIFIED: codebase analysis]

### 7. Cycle-Too-Long Warning (`src/main.rs`, lines 400-410)

```rust
let cycle_ms = cycle_start.elapsed().as_millis();
if cycle_ms > poll_interval_ms as u128 {
    tracing::warn!(...);
}
```

Change comparison to `tick_interval_ms`. Update the message text accordingly. [VERIFIED: codebase]

## Implementation Approach Per Decision

### D-01 / D-02: Add `poll_interval_ms` to DeviceConfig

```rust
#[derive(Debug, serde::Deserialize, Clone)]
pub struct DeviceConfig {
    pub address: u8,
    pub name: String,
    pub poll_interval_ms: Option<u64>,  // NEW: per-device override
}
```

**Impact on existing tests:** The `poller.rs` tests (lines 176-205) construct `DeviceConfig` directly — these must add `poll_interval_ms: None` to struct literals. Same for any test in `config.rs` that builds `DeviceConfig` manually (the `test_empty_device_list_rejected` test at line 171). TOML-based tests are unaffected (serde handles missing optional fields). [VERIFIED: codebase grep]

### D-03: Validation

Add inside the existing `for device in &cfg.devices` loop:
```rust
if let Some(ms) = device.poll_interval_ms {
    anyhow::ensure!(
        ms > 0,
        "device '{}' has invalid poll_interval_ms {} (must be > 0)",
        device.name, ms
    );
}
```

### D-04 / D-05: Tick Interval + Due Check

Compute after line 140:
```rust
let tick_interval_ms: u64 = cfg.devices.iter()
    .filter_map(|d| d.poll_interval_ms)
    .chain(std::iter::once(poll_interval_ms))  // include global
    .min()
    .unwrap();  // safe: chain always has at least one element

let mut last_polled_at: Vec<std::time::Instant> = cfg.devices.iter().map(|d| {
    let effective = d.poll_interval_ms.unwrap_or(poll_interval_ms);
    std::time::Instant::now() - std::time::Duration::from_millis(effective)
}).collect();
```

The `last_polled_at` initialization subtracts the effective interval from `now` so all devices are immediately due on the first tick. This is simpler and safer than the CONTEXT suggestion of `u64::MAX / 2` (which could overflow on some platforms). [VERIFIED: std::time::Instant arithmetic is safe for reasonable durations]

### D-06: Sub-500ms Warning

No code change needed — the warning at lines 143-149 already only checks the global `poll_interval_ms`. [VERIFIED: codebase]

### D-09: Cycle-Too-Long Warning

Change:
```rust
if cycle_ms > poll_interval_ms as u128 {
```
to:
```rust
if cycle_ms > tick_interval_ms as u128 {
```
And update the message to reference `tick_interval_ms`.

## Agent's Discretion Recommendations

### last_polled_at: Vec<Instant> (recommended)

Use `Vec<std::time::Instant>` indexed parallel to `cfg.devices`. Reasons:
- Devices are already iterated by index (`cfg.devices.iter().enumerate()`)
- No allocation overhead vs HashMap
- Simpler code — no key lookup
- `cfg.devices` never changes at runtime

### Startup Logging

Emit per-device effective intervals when overrides are present:
```rust
let override_count = cfg.devices.iter().filter(|d| d.poll_interval_ms.is_some()).count();
if override_count > 0 {
    tracing::info!(
        tick_interval_ms = tick_interval_ms,
        overrides = override_count,
        "Per-device poll intervals active"
    );
    for device in &cfg.devices {
        let effective = device.poll_interval_ms.unwrap_or(poll_interval_ms);
        tracing::info!(device = %device.name, interval_ms = effective, "Device poll interval");
    }
}
```

This gives operators visibility without cluttering logs when no overrides exist.

### CRIT-02 Counter: Pass-Level (recommended)

Keep the existing pass-level counter but guard it with `any_polled`:
```rust
if !any_polled {
    // No devices were due this tick — don't change the counter
} else if any_ok {
    consecutive_all_fail = 0;
} else {
    consecutive_all_fail += 1;
    // ... existing warn + exit logic
}
```

This is simpler than per-device tracking and achieves the safety intent: if the Modbus bus is dead, all devices will fail on their respective due ticks, and the counter will accumulate to the threshold. The guard prevents false-positive increments on ticks where no devices were due.

## Common Pitfalls

### Pitfall 1: last_polled_at Must Update on Failure Too
**What goes wrong:** If `last_polled_at[i]` is only updated on success, a failing device will be retried every tick (every `tick_interval_ms`) instead of waiting its full effective interval.
**Why it happens:** Natural inclination to only update timestamp on success.
**How to avoid:** Update `last_polled_at[i] = now` after both success AND failure paths.
**Warning signs:** A failing device appearing in every tick's poll list.

### Pitfall 2: Struct Literal Breakage in Tests
**What goes wrong:** Adding a field to `DeviceConfig` causes compilation errors in all tests that construct `DeviceConfig` directly.
**Why it happens:** Rust requires all struct fields in literal construction.
**How to avoid:** Grep for `DeviceConfig {` across the codebase and update all occurrences. Known locations:
- `src/poller.rs` lines 182-185, 199-202 (2 test functions)
- `src/config.rs` line 171-189 (`test_empty_device_list_rejected`)
**Warning signs:** `cargo build` failure after changing the struct.

### Pitfall 3: Instant::duration_since Panic
**What goes wrong:** `a.duration_since(b)` panics if `b > a` (on some platforms) or returns Duration::ZERO.
**Why it happens:** Clock monotonicity edge cases.
**How to avoid:** Use `now.checked_duration_since(last_polled_at[i]).unwrap_or(Duration::MAX)` or simply `now >= last_polled_at[i] + effective_duration` which is the pattern from D-04.
**Warning signs:** Panic in production on unusual platforms.

### Pitfall 4: Ticker Drift with MissedTickBehavior::Skip
**What goes wrong:** If the tick interval is very short and polling takes longer than one tick, ticks are skipped. This is actually the desired behavior (D-04 says "Skip" missed ticks).
**Why it happens:** `MissedTickBehavior::Skip` is already set.
**How to avoid:** No action needed — already correct. The per-device due check handles scheduling correctly regardless of missed ticks.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Tick scheduling | Custom timer/sleep loop | `tokio::time::interval` with `MissedTickBehavior::Skip` | Already in use; handles drift, cancellation, and async correctly |
| Duration arithmetic | Manual ms comparison with casts | `std::time::Instant` + `Duration` | Overflow-safe, monotonic clock, standard library |

## Code Examples

### Full tick_interval_ms Computation
```rust
// Compute tick cadence: minimum of global interval and all device overrides
let tick_interval_ms: u64 = cfg.devices.iter()
    .filter_map(|d| d.poll_interval_ms)
    .chain(std::iter::once(poll_interval_ms))
    .min()
    .unwrap(); // safe: chain guarantees at least one element
```

### Per-Device Due Check Pattern
```rust
let now = std::time::Instant::now();
let mut any_polled = false;
let mut any_ok = false;

for (i, device) in cfg.devices.iter().enumerate() {
    let effective = std::time::Duration::from_millis(
        device.poll_interval_ms.unwrap_or(poll_interval_ms)
    );
    if now.duration_since(last_polled_at[i]) < effective {
        continue;
    }
    any_polled = true;
    last_polled_at[i] = now;
    // ... poll_device + influx write (existing code)
}
```

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Struct literal breakage in tests | Certain | Low (compile error, easy fix) | Grep and fix all `DeviceConfig {` occurrences |
| Regression when no per-device overrides | Low | High | tick_interval_ms == poll_interval_ms when no overrides → identical behavior |
| CRIT-02 false positives on empty ticks | Medium | Medium | Guard with `any_polled` flag |
| Instant arithmetic edge cases | Very Low | Low | Use comparison pattern from D-04, not subtraction |

## Files to Modify

| File | Changes |
|------|---------|
| `src/config.rs` | Add `poll_interval_ms: Option<u64>` to `DeviceConfig`; add validation in device loop; update test struct literals |
| `src/main.rs` | Compute `tick_interval_ms`; init `last_polled_at`; change ticker duration; add due-check in poll loop; adapt CRIT-02 guard; adapt cycle-too-long warning; add startup logging for overrides |
| `src/poller.rs` | Update test `DeviceConfig` struct literals (add `poll_interval_ms: None`) |
| `config.toml.example` | Add commented `poll_interval_ms` example in a `[[devices]]` block |
| `config.toml` | Add commented `poll_interval_ms` example |

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `std::time::Instant::now() - Duration::from_millis(effective)` won't underflow for reasonable interval values | Implementation Approach D-04/D-05 | Panic at startup — but Instant is monotonic and underflow only possible if effective > time since boot, which is unrealistic for ms-scale intervals |

**All other claims verified from codebase.**

## Open Questions

None — all decisions are locked in CONTEXT.md and the codebase analysis confirms feasibility for each.

## Sources

### Primary (HIGH confidence)
- `src/config.rs` — DeviceConfig struct, validate_config function, existing `Option<u64>` patterns
- `src/main.rs` — Poll loop structure, ticker setup, CRIT-02 logic, cycle-too-long warning
- `src/poller.rs` — DeviceConfig usage in tests
- `04-CONTEXT.md` — All 9 locked decisions (D-01 through D-09)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — no new dependencies, all changes within existing codebase
- Architecture: HIGH — straightforward transformation of existing poll loop
- Pitfalls: HIGH — identified from direct codebase analysis

**Research date:** 2026-04-28
**Valid until:** indefinite (codebase-specific, no external dependencies)
