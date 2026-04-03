# Phase 1: Foundation - Context

**Gathered:** 2026-04-02
**Status:** Ready for planning

<domain>
## Phase Boundary

Parse and validate a TOML config file; define the `PowerReading` struct with correct PZEM-016
register decode logic. All logic is unit-testable with no external dependencies (no hardware,
no network, no InfluxDB connection required).

Phase 1 does NOT include: Modbus communication, HTTP writes, logging setup, or signal handling.
Those belong to Phases 2–3.

</domain>

<decisions>
## Implementation Decisions

### Config Schema Design

- **D-01:** Use TOML nested sections — `[serial]`, `[influxdb]`, and `[[devices]]` array-of-tables.
  This is idiomatic in the Rust/toml ecosystem and produces clean `serde` struct derives.
- **D-02:** `poll_interval_secs` lives at the top level (flat key), not nested — it's a single global
  value and nesting it would be awkward.
- **D-03:** All config fields are **required** (no `Option<>` defaults) for v1. Operator must be
  explicit. Config validation (CFG-05) should emit clear errors for every missing field.
- **D-04:** Device list uses `[[devices]]` with two required fields per entry: `address` (u8, 1–247)
  and `name` (String, used as InfluxDB measurement name).

Example shape:
```toml
poll_interval_secs = 10

[serial]
port = "/dev/ttyUSB0"
baud_rate = 9600

[influxdb]
url = "http://localhost:8086"
token = "my-token"
database = "power"

[[devices]]
address = 1
name = "solar_panel"

[[devices]]
address = 2
name = "grid_meter"
```

### Module Structure

- **D-05:** Flat binary layout — `main.rs`, `config.rs`, `types.rs` as peer files under `src/`.
  No `lib.rs` crate split in Phase 1. Phase 2–3 can refactor if needed; premature org adds no value
  for a small daemon binary.

### Register Decode Error Handling

- **D-06:** `decode_registers()` returns `Result<PowerReading, anyhow::Error>` — never panics on
  device data. This is consistent with the `anyhow` error strategy used across the binary and
  enables the skip-and-log pattern Phase 3 requires without caller changes.
- **D-07:** `PowerReading` holds all six fields as `f64` (voltage, current, power, energy,
  frequency, power_factor). All are f64 to prevent InfluxDB 3 field type conflicts (STOR-03).
- **D-08:** 32-bit register reconstruction is **low-word-first**: `(high_register as u32) << 16 | low_register as u32`.
  This matches PZEM-016 datasheet layout (low word comes first in the response frame).
  Note: MEDIUM confidence — verify against physical hardware in Phase 3 (STATE.md blocker).

### Test Structure

- **D-09:** Unit tests use inline `#[cfg(test)]` modules in each source file — idiomatic Rust,
  co-located with the code under test. No separate `tests/` integration test directory in Phase 1.
- **D-10:** Register decode test uses a hardcoded `const` array of raw u16 register values derived
  from PZEM-016 datasheet example readings. Expected output values are asserted with float
  tolerance (not exact equality) due to scaling divisions.
- **D-11:** Config test uses a hardcoded TOML string (`const CONFIG: &str = "..."`) inlined in
  the test module — no external fixture files needed.

### Agent's Discretion

- Specific field scaling factors (e.g., voltage ÷ 10, current ÷ 1000) — use PZEM-016 datasheet
  values; researcher/planner should confirm register map from official docs.
- Error message wording in config validation — use clear, operator-friendly phrasing; exact
  wording left to implementation.
- `PowerReading` should also carry a `device_name: String` field for use by the InfluxDB writer
  in Phase 2 — include it now to avoid a breaking refactor.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project Foundation
- `.planning/PROJECT.md` — Project vision, constraints, key decisions, hardware context
- `.planning/REQUIREMENTS.md` — v1 requirements; Phase 1 maps to CFG-01 through CFG-05
- `.planning/ROADMAP.md` — Phase 1 plan breakdown (plans 01-01, 01-02, 01-03) and success criteria

### Stack
- `.planning/research/STACK.md` — Full stack rationale; confirms `toml 1.1.1 + serde`, `anyhow 1.0.102`; all dependency versions locked here

### PZEM-016 Protocol
- No local spec file — PZEM-016 register map must be sourced from the official PZEM-016 datasheet
  during research (registers 0x0000–0x0007, FC 0x04, low-word-first 32-bit encoding for power/energy).
  Researcher should locate and inline the register table into RESEARCH.md.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- None — greenfield project. No existing source files under `src/`.

### Established Patterns
- None yet — this phase establishes the patterns all subsequent phases follow.
- Decision: flat module layout (`config.rs`, `types.rs`, `main.rs`) sets the pattern for Phase 2–3 additions (`influx.rs`, `poller.rs`).

### Integration Points
- `PowerReading` struct (defined here in `types.rs`) is the **central data type** used by:
  - Phase 2: `to_line_protocol()` consumes `&PowerReading`
  - Phase 3: `decode_registers()` produces `PowerReading`; poll loop passes it to writer
- `AppConfig` struct (defined here in `config.rs`) is read once at startup in `main.rs` and passed by reference/Arc to Phase 3 components.

</code_context>

<specifics>
## Specific Ideas

- `device_name` on `PowerReading` — include now so Phase 2 line protocol formatter doesn't need a separate argument. Saves a refactor.
- Config validation should use `anyhow::bail!()` or `anyhow::ensure!()` patterns — consistent with the rest of the codebase error strategy, not custom error types.
- The `[[devices]]` entry `address` field: valid Modbus addresses are 1–247. Validation should reject 0 and 248–255.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---

*Phase: 01-foundation*
*Context gathered: 2026-04-02*
