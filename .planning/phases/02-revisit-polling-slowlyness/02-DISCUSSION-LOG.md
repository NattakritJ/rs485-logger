# Phase 2: Revisit Polling Slowlyness - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-09
**Phase:** 02-revisit-polling-slowlyness
**Areas discussed:** Root cause diagnosis, Write decoupling strategy, InfluxDB health tracking, Write ordering, Backpressure, InfluxWriter sharing, CFG-02 parity field, RISK-1 udev rule

---

## Phase Scope Identification

| Option | Description | Selected |
|--------|-------------|----------|
| More tuning needed | ~900ms still too slow — need further Modbus tuning | |
| Fix remaining active issues | CFG-02, RISK-1 cleanup items from PROJECT.md | |
| Both: verify + fix | Verify Phase 1 results, fix CFG-02/RISK-1 | |
| New capability | Multiple USB adapters, async parallel polling | |

**User's choice:** Shared production logs showing poll cycles consistently at ~4723–5001ms despite Phase 1 optimizations.

**Notes:** User provided real systemd journal output from the deployed Pi. The logs revealed that the Modbus reads complete in ~60ms (not the 150ms timeout path at all), but each InfluxDB write takes ~630ms. The cycle timer confirms the bottleneck: 5 × (~60ms Modbus + ~630ms InfluxDB write + ~30ms IFD) ≈ 3600–5000ms.

---

## Root Cause Diagnosis

Analysis from logs:
- Device poll (FC 0x04): ~60–65ms (responding well under the 150ms timeout)
- InfluxDB HTTP write: ~630ms per device
- IFD read delay: 30ms
- Total per device: ~720ms × 5 devices = ~3.6s (plus minor overheads = ~5s observed)

The Phase 1 Modbus timeout reduction was not the bottleneck at all. The bottleneck is sequential InfluxDB writes blocking the poll loop.

---

## Write Decoupling Strategy

| Option | Description | Selected |
|--------|-------------|----------|
| Fire-and-forget writes | tokio::spawn per write, poll loop doesn't await | ✓ |
| Background write worker | Dedicated channel + writer task | |
| HTTP write timeout | Shorter timeout on InfluxDB writes | |

**User's choice:** Fire-and-forget writes
**Notes:** Simplest solution, no channel complexity needed.

---

## InfluxDB Health Tracking

| Option | Description | Selected |
|--------|-------------|----------|
| Arc<AtomicBool> shared flag | Move health tracking into spawned tasks via shared atomic | ✓ |
| Drop health tracking | Each spawned task logs independently, no suppression | |
| Log per-task, no suppression | WARN per task, no repeat suppression | |

**User's choice:** Arc<AtomicBool> shared flag
**Notes:** Preserves the Phase 7 MED-04 health suppression feature exactly.

---

## Write Ordering

| Option | Description | Selected |
|--------|-------------|----------|
| Accept any order (timestamps correct) | Out-of-order HTTP delivery is fine — timestamps set at poll time | ✓ |
| Ordered channel writer | Preserve write order via single sequential writer task | |

**User's choice:** Accept any order
**Notes:** PowerReading timestamps are set at Modbus read time. InfluxDB uses timestamps for ordering, not insertion order.

---

## Backpressure

| Option | Description | Selected |
|--------|-------------|----------|
| Unbounded spawn, rely on pool | No semaphore; 4GB RAM / 288MB used makes memory pressure irrelevant | ✓ |
| Arc<Semaphore> cap | Limit concurrent write tasks to 2–3 | |

**User's choice:** Unbounded spawn
**Notes:** User confirmed 4GB RAM, 288MB used, daemon-only workload. No memory pressure risk from queued write tasks.

---

## InfluxWriter Sharing

| Option | Description | Selected |
|--------|-------------|----------|
| Clone InfluxWriter (cheap Arc clone) | derive(Clone), clone into each task; O(1) cost | ✓ |
| Arc<InfluxWriter> | Wrap in Arc explicitly | |

**User's choice:** Clone InfluxWriter
**Notes:** reqwest::Client is Arc-based internally. derive(Clone) is idiomatic.

---

## CFG-02: Parity Field

| Option | Description | Selected |
|--------|-------------|----------|
| Fold into Phase 2 | Add parity field here, same deployment context | ✓ |
| Separate quick tasks | Handle CFG-02/RISK-1 after Phase 2 | |

**User's choice:** Fold into Phase 2

---

## RISK-1: Udev Rule Fix

| Option | Description | Selected |
|--------|-------------|----------|
| Match by VID/PID or product, drop DRIVERS== | Driver-agnostic rule; user adds VID/PID for precision | ✓ |
| Document both drivers, user picks | Keep DRIVERS== but document ch341 and cp210x options | |

**User's choice:** Match by VID/PID, drop DRIVERS==
**Notes:** Current rule has comment saying "cp210x (most common)" but actual rule uses `DRIVERS=="ch341"`. Both comment and rule need fixing.

---

## the agent's Discretion

- Whether to use `AtomicBool::compare_exchange` or `swap` for health state transitions
- Exact placement of spawned write helper (inline closure vs extracted `async fn`)
- Whether to also fix reqwest feature rename (`"rustls"` → `"rustls-tls"`) as incidental cleanup

## Deferred Ideas

- Channel-based write worker — not needed, fire-and-forget is sufficient
- reqwest feature rename — minor naming fix, can be quick task or folded in as trivial change
- Per-device write timeout — not needed
