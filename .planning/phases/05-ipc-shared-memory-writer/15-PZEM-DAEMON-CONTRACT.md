# PZEM Daemon — Shared Memory Writer Contract

> **Purpose:** This document defines the shared memory interface that the PZEM daemon must implement so the Solar Orchestrator can read per-inverter power data directly, bypassing InfluxDB latency.
>
> **Audience:** The AI agent (or developer) modifying the PZEM daemon codebase. Carry this file to the PZEM daemon repo and use it as the requirements source for the shared memory writer phase.

## Overview

The Solar Orchestrator controls 3 Deye microinverters by spoofing Smart Meter UDP packets. It currently reads per-inverter power output from InfluxDB (HTTP API, ~1s latency). This contract defines a shared memory interface so the orchestrator can read power data directly from the PZEM daemon with near-zero latency (<1μs).

**Architecture:**
```
PZEM-016 (RS485) → PZEM Daemon (writer) → /dev/shm/pzem-power.bin → Orchestrator (reader)
                         ↓
                    InfluxDB (unchanged — keep writing for Grafana/historical)
```

The PZEM daemon MUST continue writing to InfluxDB in addition to shared memory. InfluxDB is used for Grafana dashboards, historical analysis, and as the orchestrator's fallback data source.

## Shared Memory File

**Path:** `/dev/shm/pzem-power.bin` (configurable — the orchestrator reads this from its config)

**File creation:** The PZEM daemon creates the file on startup using `OpenOptions::create(true).read(true).write(true)`, then `set_len(96)` to ensure the file is exactly 96 bytes. If the file already exists, truncate to 96 bytes and reinitialize.

**File permissions:** `0o644` — readable by any local process, writable only by the PZEM daemon.

## Shared Memory Layout

Fixed-size, 96-byte struct. All multi-byte integers are **native endian** (both processes run on the same machine).

```
Offset  Size   Type           Field             Description
──────  ─────  ─────────────  ────────────────  ─────────────────────────────────────
0       8      u64            sequence          Seqlock counter (see Synchronization)
8       8      i64            timestamp_ns      Unix epoch nanoseconds of last write
16      1      u8             inverter_count    Number of valid readings (0-3)
17      7      [u8; 7]        _padding          Alignment padding (write zeros)
24      24     PzemReading    readings[0]       First inverter
48      24     PzemReading    readings[1]       Second inverter
72      24     PzemReading    readings[2]       Third inverter
──────  ─────
0       96     Total

PzemReading (24 bytes):
Offset  Size   Type           Field             Description
──────  ─────  ─────────────  ────────────────  ─────────────────────────────────────
0       16     [u8; 16]       name              Inverter name, null-terminated UTF-8
16      8      f64            power_watts       Current output power in watts
```

### Inverter Names

The `name` field MUST match the orchestrator's config inverter names exactly:
- `"east\0"` (padded to 16 bytes with zeros)
- `"west\0"` (padded to 16 bytes with zeros)
- `"west2\0"` (padded to 16 bytes with zeros)

The orchestrator matches these names against its `[inverter.east]`, `[inverter.west]`, `[inverter.west2]` config keys. **Case-sensitive, exact match.**

### Initial State

On startup (before first PZEM read), initialize the entire 96-byte region to zeros. This means:
- `sequence = 0` (even = not currently writing)
- `timestamp_ns = 0` (will be detected as stale by the orchestrator)
- `inverter_count = 0` (no valid readings)

## Synchronization: Seqlock Protocol

No semaphores, no mutexes, no kernel syscalls. The seqlock is a lightweight userspace protocol.

### Writer (PZEM daemon) — every write cycle:

```
1. Atomic store: sequence = sequence + 1    (now ODD → "writing in progress")
   Memory ordering: Release
2. Write timestamp_ns, inverter_count, readings[0..2]
   (regular memcpy / field-by-field write — no atomics needed for data)
3. Atomic store: sequence = sequence + 1    (now EVEN → "write complete")
   Memory ordering: Release
```

### Reader (Orchestrator) — every 300ms control cycle:

```
1. Atomic load: seq1 = sequence             (Acquire ordering)
2. If seq1 is ODD → writer is mid-write. Retry from step 1 (spin).
3. Read timestamp_ns, inverter_count, readings[0..2]
   (regular memcpy)
4. Atomic load: seq2 = sequence             (Acquire ordering)
5. If seq1 != seq2 → data changed during read. Retry from step 1.
6. Data is consistent. Use it.
```

**Retry budget:** The orchestrator retries up to 3 times. If all retries fail (writer is stuck mid-write), treat as stale data. At 300ms write intervals, this should never happen in practice.

### Memory Ordering

- Writer: `AtomicU64::store(val, Ordering::Release)` for sequence counter
- Reader: `AtomicU64::load(Ordering::Acquire)` for sequence counter
- Data fields (timestamp, readings): regular reads/writes — the seqlock sequence acts as the memory barrier

### Important: Sequence Counter Initialization

The sequence counter starts at 0. The first write makes it 1 (odd, writing), then 2 (even, done). The reader sees 0 (even, no data yet) and checks `timestamp_ns == 0` to detect "never written."

## Write Timing

- Write at the same interval as PZEM reads (~300ms target, matching orchestrator control cycle)
- Write ALL inverter data in a single atomic seqlock-protected transaction (not one inverter at a time)
- If a PZEM read fails for one inverter, still write the others — set `inverter_count` to the number of successful reads and only populate those `readings[]` entries

## Graceful Degradation

### PZEM daemon scenarios:

| Scenario | PZEM daemon behavior |
|----------|---------------------|
| Normal operation | Write to shm every ~300ms AND write to InfluxDB at existing interval |
| RS485 read failure (one inverter) | Write remaining inverters to shm, set `inverter_count` accordingly |
| RS485 bus failure (all inverters) | Stop updating shm (timestamp goes stale). Orchestrator detects in ≤5s |
| PZEM daemon shutdown | File persists in `/dev/shm/` but goes stale. Orchestrator falls back to InfluxDB |
| PZEM daemon restart | Reinitialize file (truncate to 96 bytes, zero-fill, start writing) |

### Orchestrator scenarios (for reference — implemented in the orchestrator repo):

| Scenario | Orchestrator behavior |
|----------|----------------------|
| shm file doesn't exist | Use InfluxDB only (current behavior) |
| shm file exists, data fresh (<5s) | Use PZEM as primary data source |
| shm file exists, data stale (>5s) | Fall back to InfluxDB polling |
| shm file exists, InfluxDB also down | Fall back to equal distribution mode |

## Testing

### Writer-side verification:

1. **File creation:** `ls -la /dev/shm/pzem-power.bin` — should be 96 bytes, readable
2. **Content inspection:** `hexdump -C /dev/shm/pzem-power.bin` — verify sequence counter increments, timestamp updates, inverter names visible as ASCII
3. **Staleness:** Stop the PZEM daemon, verify timestamp stops updating
4. **Restart:** Restart the PZEM daemon, verify file is reinitialized and writes resume

### Cross-process verification (after orchestrator implements reader):

1. Start PZEM daemon first → start orchestrator → verify PZEM data source active in logs
2. Stop PZEM daemon → verify orchestrator falls back to InfluxDB within 5s
3. Restart PZEM daemon → verify orchestrator switches back to PZEM
4. Start orchestrator first → start PZEM daemon → verify lazy detection picks up shm file

## Reference: Orchestrator Config

The orchestrator will have this config section (for reference):

```toml
[pzem]
enabled = true
shm_path = "/dev/shm/pzem-power.bin"
stale_threshold_s = 5
```

## Rust Implementation Hints

For the PZEM daemon (assuming Rust — adapt if using Python/C/etc.):

```rust
// Cargo.toml dependency
// memmap2 = "0.9"

use std::fs::OpenOptions;
use std::sync::atomic::{AtomicU64, Ordering};
use memmap2::MmapMut;

// Create/open the shared memory file
let file = OpenOptions::new()
    .read(true)
    .write(true)
    .create(true)
    .open("/dev/shm/pzem-power.bin")?;
file.set_len(96)?;
let mut mmap = unsafe { MmapMut::map_mut(&file)? };

// Write cycle (every ~300ms after reading PZEM sensors):
let seq_ptr = mmap.as_ptr() as *const AtomicU64;
let seq = unsafe { &*seq_ptr };
let current = seq.load(Ordering::Relaxed);

// Step 1: signal write-in-progress (odd)
seq.store(current + 1, Ordering::Release);

// Step 2: write data (regular writes to mmap[8..96])
// ... write timestamp_ns at offset 8
// ... write inverter_count at offset 16
// ... write readings at offset 24, 48, 72

// Step 3: signal write-complete (even)
seq.store(current + 2, Ordering::Release);
```

**If the PZEM daemon is Python:** Use `mmap` module + `struct.pack()` for the writes. The seqlock can use a simple integer at offset 0 — Python's GIL prevents torn writes for small aligned values, but for correctness, use `ctypes` atomic operations or accept the tiny race window (at 300ms intervals, it's practically zero).

---

*Contract version: 1.0*
*Defined: 2026-04-28*
*Source: Solar Orchestrator Phase 15 discussion*
