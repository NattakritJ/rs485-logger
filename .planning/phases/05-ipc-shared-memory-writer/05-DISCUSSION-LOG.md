# Phase 5: IPC Shared Memory Writer - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-28
**Phase:** 05-ipc-shared-memory-writer
**Areas discussed:** Device-to-inverter mapping, Config schema, Write integration point

---

## Device-to-Inverter Mapping

| Option | Description | Selected |
|--------|-------------|----------|
| Config-driven filter | Add `shm_name` field to [[devices]] in TOML. Only devices with `shm_name` get written to shm | ✓ |
| Separate mapping in [shm] | A mapping table like shm.devices = [{name="east", address=1}, ...] | |
| Use device names directly | First 3 devices become readings[0-2], names written as-is | |

**User's choice:** Config-driven filter — `shm_name: Option<String>` on `DeviceConfig`
**Notes:** Cleanly separates "what gets polled" from "what gets exposed to orchestrator"

---

## Config Schema

| Option | Description | Selected |
|--------|-------------|----------|
| Optional [shm] section | [shm] with enabled, path. If absent, disabled. Follows energy_reset pattern | ✓ |
| Top-level fields | shm_enabled, shm_path at root config level | |
| Always-on, path only | Just shm_path at root. Present = enabled | |

**User's choice:** Optional [shm] section — consistent with existing energy_reset config pattern
**Notes:** `enabled: bool` + `path: String` (default "/dev/shm/pzem-power.bin")

---

## Write Integration Point

| Option | Description | Selected |
|--------|-------------|----------|
| Synchronous in poll loop | After all devices polled, write shm synchronously. New shm.rs module | ✓ |
| Fire-and-forget like InfluxDB | Spawn async task for shm write | |
| Inline in main.rs | Write shm directly without separate module | |

**User's choice:** Synchronous in poll loop with new `shm.rs` module containing `ShmWriter` struct
**Notes:** Shm write is memcpy (<1μs), no need for async overhead

---

## Agent's Discretion

- ShmWriter internal struct layout (MmapMut wrapping)
- Error handling for mmap failures mid-operation
- Config example formatting choices

## Deferred Ideas

None — discussion stayed within phase scope.
