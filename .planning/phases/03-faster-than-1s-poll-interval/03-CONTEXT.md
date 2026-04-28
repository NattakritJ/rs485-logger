# Phase 3: Faster than 1s poll interval - Context

**Gathered:** 2026-04-28
**Status:** Ready for planning

<domain>
## Phase Boundary

Enable poll intervals below 1 second by extending the TOML config to accept millisecond-level values. The current `poll_interval_secs: u64` field accepts only whole seconds (minimum 1). After Phase 2, a 5-device cycle completes in ~450ms — sub-second polling is physically achievable and this phase makes it configurable.

This phase is config-schema and validation only — no Modbus protocol changes, no new data fields, no architectural rewrites. Sequential polling is unchanged.

</domain>

<decisions>
## Implementation Decisions

### Config Field Approach
- **D-01:** Add a new optional field `poll_interval_ms: Option<u64>` to `AppConfig` in `src/config.rs`.
- **D-02:** `poll_interval_ms` takes precedence over `poll_interval_secs` when both are present in config.
- **D-03:** Both fields are optional. If neither is set, the effective interval defaults to **10 seconds** (10,000ms). Rationale: matches the current `config.toml` default and avoids requiring every config to specify an interval.

### Backward Compatibility
- **D-04:** Change `poll_interval_secs: u64` → `poll_interval_secs: Option<u64>` in `AppConfig`. Existing configs that set `poll_interval_secs = 10` continue to work without modification.
- **D-05:** Derive the **effective interval in ms** once at startup (after loading config):
  ```rust
  let poll_interval_ms: u64 = cfg.poll_interval_ms
      .unwrap_or_else(|| cfg.poll_interval_secs.unwrap_or(10) * 1000);
  ```
  Pass this single `u64` (milliseconds) through the rest of the program so callers are insulated from the two-field schema.
- **D-06:** Update `config.toml` comments to document both fields and note that `poll_interval_ms` overrides `poll_interval_secs`.

### Minimum Interval Guard
- **D-07:** No hard minimum floor — the daemon will not fail startup for small intervals.
- **D-08:** Log a `tracing::warn!` at startup if the effective interval is **< 500ms**:
  ```
  poll_interval_ms = {N}, "Poll interval is very short ({N}ms) — a 5-device cycle takes ~450ms, polling may not complete within the interval"
  ```
  The threshold 500ms is chosen because a 5-device cycle takes ~450ms — anything below gives less than 50ms headroom.

### Cycle Duration Warning (update existing)
- **D-09:** Update the existing cycle-too-long warning in `src/main.rs` to always display the interval in milliseconds, regardless of which config field was used. Example:
  ```
  "Poll cycle took {X}ms, exceeds configured interval of {Y}ms — consider reducing read_timeout_ms or device count"
  ```
- **D-10:** The comparison logic (`cycle_ms > interval_ms`) is already in ms internally (line 388). After D-05, this comparison now uses the resolved `poll_interval_ms` field directly — no cast from `poll_interval_secs` needed.

### Config Validation Updates
- **D-11:** Update `validate_config` in `src/config.rs`:
  - If `poll_interval_secs` is present, ensure it is > 0.
  - If `poll_interval_ms` is present, ensure it is > 0.
  - If both are absent, no error — default of 10s applies (D-03).
- **D-12:** Remove the existing `anyhow::ensure!(cfg.poll_interval_secs > 0, ...)` guard and replace with the updated logic above.

### Startup Logging
- **D-13:** Update the startup log line that currently logs `interval_secs = cfg.poll_interval_secs` to log the **resolved effective interval** in ms:
  ```rust
  tracing::info!(poll_interval_ms = poll_interval_ms, "Starting poll loop");
  ```

### the agent's Discretion
- Whether to store the resolved `poll_interval_ms: u64` on `AppConfig` (computed field) or derive it inline at the call site in `main.rs`
- Whether existing tests that hardcode `poll_interval_secs: 10` need all to be updated, or whether a targeted subset is sufficient
- Exact placement of the D-08 WARN log (config load time vs. poll loop start)

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

No external specs for this phase — requirements are fully captured in decisions above.

### Existing Implementation
- `src/config.rs` — `AppConfig` struct (`poll_interval_secs: u64` → make `Option<u64>`); `validate_config` (update zero-check); all test fixtures using `poll_interval_secs = 10`
- `src/main.rs` — `Duration::from_secs(cfg.poll_interval_secs)` ticker setup (line 181); cycle warning comparison (line 388); startup log (line 139)
- `config.toml` — comment says "Minimum: 1. Typical: 10." — update to document both fields
- `config.toml.example` — same comment update needed

### Prior Phase Context
- Phase 1 CONTEXT.md (`01-CONTEXT.md`) — established `read_timeout_ms: Option<u64>` pattern for config overrides
- Phase 2 CONTEXT.md (`02-CONTEXT.md`) — established `parity: Option<String>` pattern; confirms existing configs must not break

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `Option<u64>` with `unwrap_or(default)` config pattern — used for `read_timeout_ms` (src/config.rs:30) and `parity` (src/config.rs:34); follow exact same pattern for `poll_interval_ms` and the optionalised `poll_interval_secs`
- Cycle duration warning infrastructure already in place at `src/main.rs:386–395` — only the comparison value and message format need updating

### Established Patterns
- Config optional fields: `Option<T>` + `unwrap_or(sensible_default)` — never `#[serde(default)]` magic
- Fail-fast validation at startup in `validate_config` — add new checks here, not in `main()`
- Structured log fields at startup: `tracing::info!(field = value, "message")` pattern
- All existing tests in `src/config.rs` use inline TOML strings — updating them follows the same pattern

### Integration Points
- `tokio::time::interval(Duration::from_secs(cfg.poll_interval_secs))` at `src/main.rs:180–182` — change to `Duration::from_millis(poll_interval_ms)` after resolving the effective interval
- `cycle_ms > interval_ms` comparison at `src/main.rs:388` — `interval_ms` currently derived as `cfg.poll_interval_secs as u128 * 1000`; replace with resolved `poll_interval_ms as u128`

</code_context>

<specifics>
## Specific Ideas

- The two-field approach (keep `poll_interval_secs`, add `poll_interval_ms`) avoids breaking any deployed config on any Raspberry Pi. Both optional with a 10s default means someone who wipes their config and restarts gets a safe default automatically.
- Resolving to a single `poll_interval_ms: u64` in main() and threading that through is cleaner than reading both fields at every call site.
- The 500ms WARN threshold is well-chosen: it matches the realistic floor for a 5-device chain (5 × ~90ms per device = ~450ms cycle), leaving only ~50ms headroom at exactly 500ms.

</specifics>

<deferred>
## Deferred Ideas

- Per-device polling intervals — consistent with prior deferral in Phase 1; single global interval remains sufficient
- Dynamic interval adjustment based on measured cycle duration — interesting but adds complexity; not needed now
- Fractional-second display in log messages (e.g., "0.5s") — ms display is sufficient and simpler

</deferred>

---

*Phase: 03-faster-than-1s-poll-interval*
*Context gathered: 2026-04-28*
