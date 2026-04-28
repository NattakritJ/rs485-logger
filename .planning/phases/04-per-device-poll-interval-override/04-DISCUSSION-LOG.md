# Phase 4: Per-Device Poll Interval Override - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Session:** 2026-04-28

---

## Area 1: Config Schema

**Q: What field name should the per-device interval use in a [[devices]] block?**
Options presented: `poll_interval_ms` (mirrors global), `interval_ms` (shorter), both `poll_interval_ms` + `poll_interval_secs`
→ Selected: `poll_interval_ms` — mirrors the global field name; operators see the same key in both places

**Q: Should poll_interval_ms on a device be optional (falling back to the global interval) or required for every device?**
Options presented: Optional per device, Required per device
→ Selected: Optional per device — device without poll_interval_ms inherits the global interval

---

## Area 2: Scheduler Design

**Q: How should the daemon schedule devices with different poll intervals on the shared RS485 bus?**
Options presented:
- Tick-at-minimum + per-device due check
- Priority queue scheduler (BinaryHeap)
- Poll all every cycle, skip InfluxDB write if not due
→ Selected: Tick-at-minimum + per-device due check — main loop ticks at min(all device intervals); each tick checks which devices are 'due'; polls only those devices sequentially

**Q: Should the sub-500ms warning (added in Phase 3) apply per-device too?**
Options presented: Warn per device, Warn only if global interval < 500ms
→ Selected: Warn only if global interval < 500ms — per-device overrides don't trigger the warning

---

## Area 3: Global Interval Semantics

**Q: What role should the global poll_interval_ms/poll_interval_secs play when devices have per-device overrides?**
Options presented: Global is the default, Global becomes optional
→ Selected: Global is the default — devices without poll_interval_ms inherit the global; global is always required (or defaults to 10s)

**Q: How should the cycle-too-long warning behave with per-device intervals?**
Options presented: Compare against min device interval, Remove warning, Agent's discretion
→ Selected: Agent's discretion

---

*Discussion completed: 2026-04-28*
