# Phase 6: Energy Reset at 00:00 Thailand Time - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-02
**Phase:** 06-send-command-to-reset-energy-at-beginning-of-day
**Areas discussed:** Reset scope, Sending the 0x42 command, Scheduling approach, Config & observability

---

## Reset Scope

### Q1: Which devices to reset?

| Option | Description | Selected |
|--------|-------------|----------|
| Per-device with confirmation | Send 0x42 to each device individually; wait for echo reply; skip-and-log on failure | ✓ |
| Broadcast (no reply) | Send to address 0x00; resets all at once; no per-device confirmation | |
| Per-device fire-and-forget | Send to each device but don't wait for reply | |

**User's choice:** Per-device with confirmation  
**Notes:** Aligns with existing skip-and-log pattern from poll loop.

### Q2: Where in program flow?

| Option | Description | Selected |
|--------|-------------|----------|
| Reset inside poll loop at 00:00 tick | Third arm in existing tokio::select! loop | ✓ |
| Separate async task | Concurrent task independent of poll loop | |
| Reset before each poll at midnight | Runs in poll loop but before data read | |

**User's choice:** Reset inside poll loop at 00:00 tick

---

## Sending the 0x42 Command

### Q3: How to send 0x42?

| Option | Description | Selected |
|--------|-------------|----------|
| tokio-modbus call() for raw PDU | RTU framer handles CRC; Request::Custom(0x42, empty) | ✓ |
| Bypass tokio-modbus, write raw bytes | Manual CRC, manual frame parsing | |
| Try standard Modbus FC | Won't work — 0x42 is not a standard register FC | |

**User's choice:** Use tokio-modbus call() for raw PDU

### Q4: How to validate the reply?

| Option | Description | Selected |
|--------|-------------|----------|
| Validate echo reply | Expect FC 0x42 in response; 0xC2 = error → WARN per device | ✓ |
| Accept any reply | No FC byte check | |
| Ignore reply | Fire-and-forget, log "sent" | |

**User's choice:** Validate echo reply  
**Notes:** Per ct_datasheet.txt §2.5 specification.

---

## Scheduling Approach

### Q5: How to compute next midnight?

| Option | Description | Selected |
|--------|-------------|----------|
| chrono + chrono-tz | Asia/Bangkok IANA tz; DateTime<Tz>; tokio::time::sleep_until | ✓ |
| Manual UTC+7 offset arithmetic | No extra crate; brittle | |
| Cron scheduler crate | Heavy dependency for single daily trigger | |

**User's choice:** chrono + chrono-tz (Recommended)

### Q6: How to reschedule after reset fires?

| Option | Description | Selected |
|--------|-------------|----------|
| Recompute next midnight after each reset | next_midnight_bangkok(now) every time | ✓ |
| Sleep 86400s after reset | Drifts over time | |
| tokio::time::interval from next midnight | Fixed interval from first midnight | |

**User's choice:** Recompute next midnight after each reset

### Clarification (user-initiated)

**User question:** "When program just start, it have to calc for the next midnight for energy reset, right?"

**Confirmed:** Yes. On startup, the daemon computes the next future midnight in Asia/Bangkok from the current wall-clock time and sleeps until then. It does not fire immediately. If started at 01:00, the next reset is tomorrow's 00:00 (~23 hours away). If started at 23:55, the next reset is ~5 minutes away.

---

## Config & Observability

### Q7: Should reset be configurable?

| Option | Description | Selected |
|--------|-------------|----------|
| Configurable: enabled + timezone + time | [energy_reset] section in config.toml | ✓ |
| Hardcode — always 00:00 Bangkok | No config changes needed | |
| Config: enabled flag only | Timezone and time remain hardcoded | |

**User's choice:** Configurable with enabled, timezone, time fields

### Q8: Log output granularity?

| Option | Description | Selected |
|--------|-------------|----------|
| info on success, warn on failure per device | Mirrors poll loop log convention | ✓ |
| Single batch-level info log | Less per-device detail | |
| debug on success, error on failure | Verbose success, severe failure | |

**User's choice:** info on success, warn on failure per device

### Q9: Restart / missed reset behaviour?

| Option | Description | Selected |
|--------|-------------|----------|
| Skip if window passed, schedule next midnight | Safe; avoids stale reset after long outage | ✓ |
| Fire if started within N minutes of midnight | Catches brief reboots near midnight | |
| Always fire on startup | Unconditional reset at startup | |

**User's choice:** Skip if window already passed, schedule next midnight

---

## Agent's Discretion

- Exact naming for reset method on ModbusPoller (e.g. `reset_energy()`)
- Whether EnergyResetConfig struct lives in `config.rs` or a new `reset.rs`
- Error message wording

## Deferred Ideas

None.
