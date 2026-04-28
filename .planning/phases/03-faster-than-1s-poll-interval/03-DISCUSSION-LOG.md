# Phase 3: Faster than 1s poll interval - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-28
**Phase:** 03-faster-than-1s-poll-interval
**Areas discussed:** Config Field Approach, Backward Compatibility, Minimum Interval Guard, Cycle Duration Warning

---

## Config Field Approach

| Option | Description | Selected |
|--------|-------------|----------|
| Add `poll_interval_ms: u64` alongside `poll_interval_secs` | ms wins if both present; precise, no floats, common in embedded/timing tools | ✓ |
| Change `poll_interval_secs` to `f64` | Allows 0.5, 0.2, etc. in-place; most intuitive for humans | |
| Keep `poll_interval_secs: u64` + add `poll_interval_ms: Option<u64>` override | Two separate optional fields | |

**User's choice:** Add `poll_interval_ms: u64` alongside — ms wins if both present.
**Notes:** None.

---

## Backward Compatibility

| Option | Description | Selected |
|--------|-------------|----------|
| Keep `poll_interval_secs` as required, `poll_interval_ms` as optional override | Existing configs need no changes | |
| Both optional with defaults (`poll_interval_secs` defaults 10s, `poll_interval_ms` overrides) | Daemon works with empty poll config | ✓ |
| `poll_interval_ms` only — deprecate `poll_interval_secs` | Breaking change, old configs fail | |

**User's choice:** Both optional — `poll_interval_secs` defaults to 10s; `poll_interval_ms` overrides when present.
**Notes:** Same `Option<T>` + `unwrap_or(default)` pattern as `read_timeout_ms` and `parity`.

---

## Minimum Interval Guard

| Option | Description | Selected |
|--------|-------------|----------|
| Hard minimum floor (e.g., 200ms) | Fail-fast at startup with clear error | |
| Warn at startup if < 500ms, proceed anyway | Log WARN, don't reject | ✓ |
| No guard | Let cycle-too-long warning handle it | |

**User's choice:** Warn at startup if interval < 500ms — no hard rejection.
**Notes:** 500ms threshold chosen because 5-device cycle is ~450ms.

---

## Cycle Duration Warning

| Option | Description | Selected |
|--------|-------------|----------|
| Always show interval in ms | e.g., `"interval of 500ms"` regardless of config field used | ✓ |
| Show in unit matching config field | secs if `poll_interval_secs`, ms if `poll_interval_ms` | |
| Keep existing format | Just fix comparison math to use ms internally | |

**User's choice:** Always show interval in ms in the warning message.
**Notes:** Consistent, avoids seconds vs ms confusion in log output.

---

## Agent's Discretion

- Whether to store the resolved `poll_interval_ms: u64` as a computed field on `AppConfig` or derive it inline in `main.rs`
- Whether to update all test fixtures or a targeted subset
- Exact placement of the < 500ms WARN log (config load time vs. poll loop start)

## Deferred Ideas

- Per-device polling intervals
- Dynamic interval adjustment based on measured cycle duration
- Fractional-second display in log messages
