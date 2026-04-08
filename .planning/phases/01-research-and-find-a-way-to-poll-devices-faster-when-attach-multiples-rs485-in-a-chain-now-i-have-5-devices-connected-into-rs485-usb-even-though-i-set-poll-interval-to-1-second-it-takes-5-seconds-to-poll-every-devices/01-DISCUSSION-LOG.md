# Phase 1: Poll Speed Optimization - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-09
**Phase:** 01-poll-speed-optimization
**Areas discussed:** Modbus timeout tuning, Inter-frame delay split, Interval semantics, Multiple USB adapters

---

## Modbus Timeout Tuning

| Option | Description | Selected |
|--------|-------------|----------|
| 150ms per device | Covers PZEM-016 even under load. Cuts worst-case cycle to ~1.25s for 5 devices. | ✓ |
| 200ms per device | Still 2.5x improvement; more margin for slow-responding devices | |
| Keep 500ms | Don't change timeout, use other techniques only | |

**User's choice:** 150ms per device

---

| Option | Description | Selected |
|--------|-------------|----------|
| 150ms for resets too | Apply same timeout to FC 0x42 | |
| Keep 500ms for resets only | EEPROM write requires full margin | ✓ |
| 300ms for resets | Middle ground | |

**User's choice:** Keep 500ms for energy resets (hardcoded)

---

| Option | Description | Selected |
|--------|-------------|----------|
| Fixed 150ms for all devices | Simple, predictable | |
| Per-device override in config | Optional per_device_timeout_ms in DeviceConfig | |
| Global config override | read_timeout_ms in [serial] config section | ✓ |

**User's choice:** Global `read_timeout_ms` in `[serial]` config (default 150ms)

---

## Inter-Frame Delay Split

| Option | Description | Selected |
|--------|-------------|----------|
| Split: 30ms reads, 100ms resets | Saves 70ms × 5 devices = 350ms per cycle | ✓ |
| Split: 50ms reads, 100ms resets | More conservative margin for reads | |
| Keep 100ms for everything | No code complexity | |
| Configurable in [serial] | inter_frame_delay_ms + reset override in config | |

**User's choice:** 30ms for reads, 100ms for resets

---

| Option | Description | Selected |
|--------|-------------|----------|
| Two constants in code | INTER_FRAME_DELAY_READ = 30ms, INTER_FRAME_DELAY_RESET = 100ms in poller.rs | ✓ |
| Both configurable in config | read_inter_frame_ms and reset_inter_frame_ms in [serial] | |
| One constant, document difference | Rename and add comment | |

**User's choice:** Two named constants in `src/poller.rs`

---

## Interval Semantics

| Option | Description | Selected |
|--------|-------------|----------|
| Keep tick-based (fix timing via tuning) | Once tuning is done, cycle fits in 1s tick | |
| Switch to sleep-after (gap-based) | poll_interval = sleep after cycle ends; less predictable timestamps | |
| Keep tick-based + add duration warning | Log WARN when cycle exceeds poll_interval_secs | ✓ |

**User's choice:** Keep tick-based + add WARN log

---

| Option | Description | Selected |
|--------|-------------|----------|
| WARN with cycle time + advice | "Poll cycle took Xms, exceeds interval Ys — consider tuning read_timeout_ms or reducing device count" | ✓ |
| INFO always (metrics-style) | Log cycle duration every tick | |
| DEBUG only | Low noise, visible when debugging | |

**User's choice:** WARN level with cycle time and tuning advice

---

## Multiple USB Adapters

| Option | Description | Selected |
|--------|-------------|----------|
| Yes, support multiple adapters | Design config for multiple serial ports; parallel polling via tokio::join! | |
| No, software-only approach | Timeout + IFD tuning gets cycle under 1.5s; no hardware change needed | ✓ |
| Document approach, implement later | README only, no code | |

**User's choice:** Software-only — defer hardware approach

---

## the agent's Discretion

- Exact location of cycle timer in the tick arm
- Whether to use `std::time::Instant` or `tokio::time::Instant` for measurement
- How to handle `read_timeout_ms` being absent from config (`Option<u64>` with fallback)

## Deferred Ideas

- Multiple USB-RS485 adapters — hardware change, future phase if tuning is insufficient
- Per-device timeout override in `DeviceConfig` — global config is sufficient for now
- Configurable inter-frame delays — two code constants are sufficient
