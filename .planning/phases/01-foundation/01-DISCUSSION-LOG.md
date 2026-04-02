# Phase 1: Foundation - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-02
**Phase:** 01-foundation
**Areas discussed:** Config schema design, Module structure, Register decode error handling, Test structure

---

## Config Schema Design

| Option | Description | Selected |
|--------|-------------|----------|
| Nested sections (`[serial]`, `[influxdb]`, `[[devices]]`) | Idiomatic TOML, clean serde derives, clear separation | ✓ |
| Flat top-level keys | Simpler for small configs, but no namespacing | |
| Mixed nested + optional defaults | More flexible but adds `Option<>` complexity | |

**User's choice:** "You decide all for me what's the best" — agent selected nested sections
**Notes:** All fields required (no Option<> defaults) for v1 to force operator awareness. `poll_interval_secs` stays at top level.

---

## Module Structure

| Option | Description | Selected |
|--------|-------------|----------|
| Flat (`main.rs`, `config.rs`, `types.rs`) | Simple, idiomatic for small binary, easy to grow | ✓ |
| `lib.rs` + submodules | Enables external testing via integration tests, more structure | |
| Single `main.rs` with inline modules | Minimal files but harder to navigate | |

**User's choice:** Agent-selected — flat layout
**Notes:** No premature organization. Phase 2–3 adds `influx.rs`, `poller.rs` peer files naturally.

---

## Register Decode Error Handling

| Option | Description | Selected |
|--------|-------------|----------|
| `Result<PowerReading, anyhow::Error>` | Consistent with anyhow strategy, enables skip-and-log | ✓ |
| `Option<PowerReading>` | Simpler but loses error context for logging | |
| `panic!()` on invalid data | Never appropriate for device data on a daemon | |

**User's choice:** Agent-selected — `Result<PowerReading, anyhow::Error>`
**Notes:** Phase 3 skip-and-continue requires a Result; using anyhow keeps it consistent with the rest of the binary.

---

## Test Structure

| Option | Description | Selected |
|--------|-------------|----------|
| Inline `#[cfg(test)]` in each module | Idiomatic Rust, co-located with code, no extra files | ✓ |
| Separate `tests/` directory | Better for integration tests; not needed at this phase | |
| Both inline + tests/ | Comprehensive but overengineered for Phase 1 | |

**User's choice:** Agent-selected — inline `#[cfg(test)]` modules
**Notes:** Hardcoded TOML string and const register array as test fixtures — no external files needed.

---

## Agent's Discretion

- Field scaling factors (voltage ÷ 10, current ÷ 1000, etc.) — defer to PZEM-016 datasheet
- Error message exact wording — clear, operator-friendly; implementation decides
- `device_name: String` on `PowerReading` — include proactively to avoid Phase 2 refactor

## Deferred Ideas

None — discussion stayed within phase scope.
