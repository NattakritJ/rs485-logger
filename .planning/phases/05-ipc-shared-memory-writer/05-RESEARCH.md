# Phase 5: IPC Shared Memory Writer - Research

**Researched:** 2026-04-28
**Domain:** Memory-mapped IPC, seqlock synchronization, binary struct layout in Rust
**Confidence:** HIGH

## Summary

This phase adds a shared memory writer to the PZEM daemon, writing per-inverter power data to a 96-byte memory-mapped file in `/dev/shm/pzem-power.bin` using a seqlock protocol. The implementation is well-constrained: the contract defines the exact binary layout (96 bytes), the synchronization protocol (seqlock with AtomicU64), and the crate to use (memmap2 0.9). All patterns needed — optional config sections, validation rules, startup logging, module structure — are already established in the codebase.

The primary technical challenges are: (1) safely casting mmap pointers to AtomicU64 references for the seqlock, (2) writing fields at exact byte offsets using raw pointer arithmetic, and (3) handling the `unsafe` blocks correctly. These are all well-understood Rust patterns with clear solutions documented below.

**Primary recommendation:** Implement `ShmWriter` in `src/shm.rs` following the `InfluxWriter` structural pattern — create in startup, call `write()` in the poll loop. Use `memmap2::MmapMut` with raw pointer writes at fixed offsets. The seqlock is 3 lines of atomic stores wrapping regular writes.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- **D-01:** Add `shm_name: Option<String>` to `DeviceConfig` in `src/config.rs`. Only devices with a `shm_name` value are written to the shared memory file.
- **D-02:** The `shm_name` value is written verbatim into the 16-byte `name` field in the binary layout (null-terminated, zero-padded). It must match the orchestrator's config keys exactly (e.g., `"east"`, `"west"`, `"west2"`).
- **D-03:** Devices without `shm_name` are ignored for shared memory — they continue to write to InfluxDB as usual.
- **D-04:** Add an optional `[shm]` section to `AppConfig`, following the `energy_reset` pattern.
- **D-05:** `ShmConfig` struct: `enabled: bool`, `path: String`. If the `[shm]` section is absent from config, shared memory is disabled.
- **D-06:** Validate in `validate_config`: at least one device must have `shm_name` if shm.enabled; no more than 3 devices with `shm_name`; `shm_name` values must be unique; `shm_name` must be ≤15 bytes.
- **D-07:** Write to shared memory synchronously in the poll loop, after all devices have been polled in the current tick.
- **D-08:** New `src/shm.rs` module with `ShmWriter` struct.
- **D-09:** `ShmWriter::new(path: &str) -> anyhow::Result<Self>` — creates file, maps memory via `memmap2`, zero-initializes.
- **D-10:** `ShmWriter::write(&mut self, readings: &[PowerReading]) -> anyhow::Result<()>` — accepts the subset of readings that have `shm_name`, performs the seqlock write. Only `power_watts` and `shm_name` are written per the contract layout.
- **D-11:** Sequence counter at offset 0 as `AtomicU64`. Writer uses `Ordering::Release` for both increments.
- **D-12:** Data fields use regular memory writes between the two atomic sequence stores.
- **D-13:** Add `memmap2 = "0.9"` to `Cargo.toml`.
- **D-14:** On daemon shutdown, do NOT delete the shm file.
- **D-15:** On daemon restart, reinitialize the file (truncate to 96 bytes, zero-fill, resume writing).
- **D-16:** Log at startup: `tracing::info!(path = shm_path, "Shared memory writer enabled")`.

### Agent's Discretion
- Whether `ShmWriter` holds the `MmapMut` directly or wraps it in a helper struct
- Exact error handling if mmap fails mid-operation (log + continue polling, or escalate)
- Whether to add a `write_shm` convenience method on the poll loop or call `shm_writer.write()` directly
- Whether the config example shows `shm_name` on all devices or just one as a hint

### Deferred Ideas (OUT OF SCOPE)
None — discussion stayed within phase scope.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| D-01–D-03 | Device-to-inverter mapping via `shm_name: Option<String>` | Config pattern section; DeviceConfig extension pattern |
| D-04–D-06 | Config schema with `[shm]` section and validation | Config pattern section; validate_config extension pattern |
| D-07 | Synchronous write in poll loop | Integration point analysis; poll loop structure |
| D-08–D-10 | ShmWriter struct in src/shm.rs | memmap2 usage patterns; code examples |
| D-11–D-12 | Seqlock protocol with AtomicU64 | Seqlock implementation pattern; memory ordering |
| D-13 | memmap2 dependency | Version verification (0.9.10 latest) |
| D-14–D-16 | Lifecycle (no delete, reinit, logging) | /dev/shm behavior; startup pattern |
</phase_requirements>

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Shared memory file creation | Daemon (Linux userspace) | — | File created via standard POSIX open+mmap on /dev/shm tmpfs |
| Seqlock write protocol | Daemon (poll loop) | — | Writer-side only; reader is in separate orchestrator repo |
| Config parsing (shm section) | Config module | — | Follows existing `energy_reset` optional section pattern |
| Config validation (shm rules) | Config module | — | Centralized in `validate_config` |
| Binary struct serialization | ShmWriter | — | Raw pointer writes at fixed offsets — no serde/bincode needed |
| Poll loop integration | main.rs tick arm | — | Synchronous call after device polling, before next tick |

## Standard Stack

### Core (New Dependency)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `memmap2` | 0.9.10 | Memory-mapped file I/O | [VERIFIED: cargo search] The standard Rust crate for mmap. Fork of unmaintained `memmap` with active maintenance. 0.9.10 is latest stable. Pure Rust wrapper over platform mmap APIs. |

### Existing (No Changes Needed)
| Library | Already In | Purpose for This Phase |
|---------|-----------|------------------------|
| `std::sync::atomic` | stdlib | AtomicU64 for seqlock sequence counter |
| `std::fs::OpenOptions` | stdlib | File creation for /dev/shm/pzem-power.bin |
| `std::os::unix::fs::PermissionsExt` | stdlib | Setting file permissions to 0o644 |
| `anyhow` | Cargo.toml | Error handling for ShmWriter |
| `tracing` | Cargo.toml | Startup and error logging |
| `chrono` | Cargo.toml | Timestamp generation (already used in codebase) |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| memmap2 | shared_memory crate | Higher-level API but adds unnecessary abstraction for a fixed 96-byte layout. memmap2 gives exact control needed. |
| memmap2 | Manual `libc::mmap` via `nix` crate | Lower-level, more boilerplate, no benefit. memmap2 wraps this safely. |
| Raw pointer writes | `bincode` / `zerocopy` | Overkill for 96 bytes at fixed offsets. Raw writes are simpler and match the contract exactly. |

**Installation (addition to Cargo.toml):**
```toml
memmap2 = "0.9"
```

**Version verification:** `memmap2` 0.9.10 is latest stable [VERIFIED: cargo search 2026-04-28]. The `"0.9"` semver range in Cargo.toml will resolve to 0.9.10.

## Architecture Patterns

### System Architecture Diagram

```
                  ┌─────────────────────────────────────────────┐
                  │              Poll Loop (main.rs)             │
                  │                                              │
  RS485 Bus ──►   │  for device in devices:                      │
  (PZEM-016)      │    reading = poller.poll_device(device)      │
                  │    spawn influx_writer.write(reading)        │
                  │                                              │
                  │  ── after all devices polled ──               │
                  │                                              │
                  │  shm_readings = filter devices with shm_name │
                  │  shm_writer.write(shm_readings)  ← NEW      │
                  └──────────────┬───────────────────────────────┘
                                 │
                    ┌────────────▼────────────┐
                    │  ShmWriter (src/shm.rs)  │
                    │                          │
                    │  MmapMut → 96 bytes      │
                    │  [seq|ts|cnt|pad|r0|r1|r2]│
                    └────────────┬─────────────┘
                                 │
                    ┌────────────▼────────────┐
                    │  /dev/shm/pzem-power.bin │
                    │  (tmpfs, 96 bytes)       │
                    └────────────┬─────────────┘
                                 │ mmap read
                    ┌────────────▼────────────┐
                    │  Solar Orchestrator      │
                    │  (separate repo/process) │
                    └─────────────────────────┘
```

### Recommended Project Structure (changes only)
```
src/
├── config.rs    # + ShmConfig struct, shm_name on DeviceConfig, validation
├── main.rs      # + mod shm; ShmWriter initialization; write call in poll loop
├── shm.rs       # NEW — ShmWriter struct with new() and write()
├── influx.rs    # unchanged
├── poller.rs    # unchanged
├── scheduler.rs # unchanged
└── types.rs     # unchanged (PowerReading used as input)
```

### Pattern 1: ShmWriter Module Structure

**What:** Single-responsibility struct holding the MmapMut and sequence counter, following the `InfluxWriter` pattern.
**When to use:** Always — this is the locked decision (D-08).

```rust
// src/shm.rs
use std::fs::OpenOptions;
use std::sync::atomic::{AtomicU64, Ordering};
use memmap2::MmapMut;
use anyhow::Context;
use crate::types::PowerReading;

const SHM_SIZE: usize = 96;

/// Pairs a successful PowerReading with the inverter name for shared memory.
pub struct ShmReading<'a> {
    pub shm_name: &'a str,
    pub power_watts: f64,
}

pub struct ShmWriter {
    mmap: MmapMut,
}

impl ShmWriter {
    pub fn new(path: &str) -> anyhow::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)   // D-15: reinitialize on restart
            .open(path)
            .with_context(|| format!("Failed to open shm file: {}", path))?;

        file.set_len(SHM_SIZE as u64)
            .with_context(|| "Failed to set shm file length")?;

        // Set permissions to 0o644
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o644);
            file.set_permissions(perms)
                .with_context(|| "Failed to set shm file permissions")?;
        }

        let mut mmap = unsafe { MmapMut::map_mut(&file)? };

        // Zero-initialize the entire region
        mmap.fill(0);

        Ok(ShmWriter { mmap })
    }

    pub fn write(&mut self, readings: &[ShmReading]) -> anyhow::Result<()> {
        let ptr = self.mmap.as_mut_ptr();

        // Step 1: Increment sequence to odd (write-in-progress)
        let seq_ref = unsafe { &*(ptr as *const AtomicU64) };
        let current = seq_ref.load(Ordering::Relaxed);
        seq_ref.store(current + 1, Ordering::Release);

        // Step 2: Write data fields
        unsafe {
            // timestamp_ns at offset 8
            let now_ns = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as i64;
            std::ptr::copy_nonoverlapping(
                &now_ns as *const i64 as *const u8,
                ptr.add(8),
                8,
            );

            // inverter_count at offset 16
            let count = readings.len().min(3) as u8;
            *ptr.add(16) = count;

            // padding at offset 17..24 (zero — already zeroed, but be explicit)
            std::ptr::write_bytes(ptr.add(17), 0, 7);

            // Write each reading at offsets 24, 48, 72
            for (i, reading) in readings.iter().take(3).enumerate() {
                let base = 24 + i * 24;

                // name: 16 bytes, null-terminated, zero-padded
                std::ptr::write_bytes(ptr.add(base), 0, 16);
                let name_bytes = reading.shm_name.as_bytes();
                let copy_len = name_bytes.len().min(15);
                std::ptr::copy_nonoverlapping(
                    name_bytes.as_ptr(),
                    ptr.add(base),
                    copy_len,
                );

                // power_watts: f64 at base+16
                std::ptr::copy_nonoverlapping(
                    &reading.power_watts as *const f64 as *const u8,
                    ptr.add(base + 16),
                    8,
                );
            }

            // Zero out unused reading slots
            for i in readings.len().min(3)..3 {
                let base = 24 + i * 24;
                std::ptr::write_bytes(ptr.add(base), 0, 24);
            }
        }

        // Step 3: Increment sequence to even (write-complete)
        seq_ref.store(current + 2, Ordering::Release);

        Ok(())
    }
}
```

### Pattern 2: Config Extension Pattern

**What:** Follow the `energy_reset: Option<EnergyResetConfig>` pattern for the new `[shm]` section.
**When to use:** Always — locked decision (D-04, D-05).

```rust
// In config.rs — additions

#[derive(Debug, serde::Deserialize)]
pub struct ShmConfig {
    pub enabled: bool,
    pub path: String,
}

// Add to AppConfig:
pub shm: Option<ShmConfig>,

// Add to DeviceConfig:
pub shm_name: Option<String>,
```

### Pattern 3: Poll Loop Integration

**What:** After the device polling for-loop completes, collect readings with `shm_name` and write to shared memory.
**When to use:** In the `ticker.tick()` arm of the select! loop in main.rs (D-07).

```rust
// In main.rs, after the `for device in &cfg.devices` loop and before cycle duration check:

// Shared memory write — synchronous, <1μs
if let Some(ref mut shm_writer) = shm_writer {
    let shm_readings: Vec<shm::ShmReading> = successful_readings
        .iter()
        .filter_map(|(device, reading)| {
            device.shm_name.as_ref().map(|name| shm::ShmReading {
                shm_name: name.as_str(),
                power_watts: reading.power,
            })
        })
        .collect();

    if let Err(e) = shm_writer.write(&shm_readings) {
        tracing::warn!(error = %e, "Shared memory write failed");
    }
}
```

**Note:** This requires collecting successful readings during the poll loop. Currently the poll loop fires-and-forgets each reading to InfluxDB immediately. The shm integration needs to accumulate readings from the current tick before writing. This means adding a `Vec` to collect `(device, reading)` pairs during the loop.

### Pattern 4: Validation Rules in validate_config

**What:** Add shm-specific validation rules following the existing pattern.
**When to use:** Always — locked decision (D-06).

```rust
// In validate_config(), after the energy_reset validation block:

if let Some(ref shm) = cfg.shm {
    if shm.enabled {
        let shm_devices: Vec<_> = cfg.devices.iter()
            .filter(|d| d.shm_name.is_some())
            .collect();
        anyhow::ensure!(
            !shm_devices.is_empty(),
            "shm.enabled is true but no devices have shm_name set"
        );
        anyhow::ensure!(
            shm_devices.len() <= 3,
            "At most 3 devices may have shm_name (contract defines 3 inverter slots), found {}",
            shm_devices.len()
        );
        // Uniqueness check
        let mut names: Vec<&str> = shm_devices.iter()
            .map(|d| d.shm_name.as_deref().unwrap())
            .collect();
        names.sort();
        names.dedup();
        anyhow::ensure!(
            names.len() == shm_devices.len(),
            "shm_name values must be unique across devices"
        );
        // Length check
        for device in &shm_devices {
            let name = device.shm_name.as_deref().unwrap();
            anyhow::ensure!(
                name.len() <= 15,
                "device '{}' shm_name '{}' exceeds 15 bytes (16-byte field minus null terminator)",
                device.name, name
            );
        }
    }
}
```

### Anti-Patterns to Avoid
- **Using `truncate(false)` on restart:** The file must be truncated and re-zeroed on every startup (D-15). Using `truncate(false)` would leave stale data from a previous run.
- **Writing one inverter at a time:** The contract requires all inverter data to be written in a single seqlock-protected transaction (not one at a time).
- **Using `Ordering::SeqCst`:** Overkill. The contract specifies `Release` for writer, `Acquire` for reader. SeqCst adds unnecessary overhead.
- **Deleting the shm file on shutdown:** D-14 explicitly prohibits this. The orchestrator detects staleness via timestamp.
- **Using serde/bincode for the 96-byte layout:** The contract defines exact byte offsets. Bincode adds padding and versioning headers that would break the layout.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Memory mapping | Custom `libc::mmap` calls | `memmap2::MmapMut` | Handles munmap on drop, platform differences, safety wrappers |
| File permissions | Manual `libc::chmod` | `std::os::unix::fs::PermissionsExt` | Idiomatic Rust, already in stdlib |
| Timestamp nanos | Manual arithmetic | `SystemTime::now().duration_since(UNIX_EPOCH).as_nanos()` | Correct, handles clock edge cases |

**Key insight:** The 96-byte fixed layout is simple enough that raw pointer writes are the correct approach — no serialization framework needed. The complexity is in getting the seqlock atomics right.

## Common Pitfalls

### Pitfall 1: SIGBUS on truncated mmap files
**What goes wrong:** If the backing file is shorter than the mapped region (e.g., another process truncates it), accessing bytes beyond the file length causes SIGBUS.
**Why it happens:** mmap maps virtual memory to file pages; accessing unmapped pages is a signal, not an error return.
**How to avoid:** Always `set_len(96)` before mapping; use `truncate(true)` in OpenOptions to ensure fresh state. The file lives on tmpfs (/dev/shm) where no external process should modify it.
**Warning signs:** Daemon crashes with "Bus error" in logs.

### Pitfall 2: Pointer alignment for AtomicU64
**What goes wrong:** Casting an unaligned pointer to `*const AtomicU64` is undefined behavior.
**Why it happens:** mmap returns page-aligned pointers (4096-byte aligned on Linux), so offset 0 is always 8-byte aligned. This is safe.
**How to avoid:** The sequence counter is at offset 0, which is guaranteed aligned by mmap. Document this assumption explicitly in a `// SAFETY:` comment.
**Warning signs:** Would only manifest on exotic platforms with non-page-aligned mmap — not Linux.

### Pitfall 3: Forgetting to zero unused reading slots
**What goes wrong:** If fewer than 3 inverters are polled, stale data remains in unused slots from previous writes.
**Why it happens:** Only writing the N active readings and leaving bytes 24+N*24..96 unchanged.
**How to avoid:** Always zero unused slots after writing active readings (shown in code example above).
**Warning signs:** Orchestrator reads stale inverter data for a slot that should be empty.

### Pitfall 4: Config deserialization breaking existing configs
**What goes wrong:** Adding `shm: Option<ShmConfig>` to AppConfig might break existing configs that don't have `[shm]`.
**Why it happens:** It won't — `Option<T>` fields in serde deserialize as `None` when absent from TOML. This is the established pattern (see `energy_reset`, `poll_interval_ms`, `log_file`).
**How to avoid:** Ensure the field is `Option<ShmConfig>` (not `ShmConfig`). Test with existing configs.
**Warning signs:** Config parse errors on startup with old configs.

### Pitfall 5: Collecting readings for shm write
**What goes wrong:** The current poll loop fires-and-forgets each reading to InfluxDB immediately inside the per-device loop. There's no collection point for "all readings this tick."
**Why it happens:** The original design didn't need to batch readings — each was independent.
**How to avoid:** Add a `Vec` before the device loop to collect successful `(DeviceConfig, PowerReading)` pairs. After the loop, filter for devices with `shm_name` and pass to `ShmWriter::write()`. This is a small structural change to the tick arm.
**Warning signs:** Only the last device's reading gets written to shm.

### Pitfall 6: AtomicU64 on 32-bit ARM
**What goes wrong:** `AtomicU64` may not be natively supported on 32-bit ARM (armv7).
**Why it happens:** ARMv7 doesn't have native 64-bit atomic instructions. Rust's `AtomicU64` falls back to a lock-based implementation on these targets via compiler intrinsics.
**How to avoid:** On the Raspberry Pi 4/5 (aarch64), this is not an issue. On 32-bit Pi 3 (armv7), the fallback still works correctly but uses a global lock. Since there's only one writer process, this is functionally correct. If targeting armv7, verify `AtomicU64::is_lock_free()` at startup and log a warning if false. [ASSUMED]
**Warning signs:** Compile error on armv7 if `target_has_atomic = "64"` is required. In practice, Rust compiles AtomicU64 on armv7 — it just may not be lock-free.

## Code Examples

### Example 1: Complete ShmWriter::new() with all safety requirements

```rust
// Source: memmap2 docs + contract requirements
use std::fs::OpenOptions;
use std::sync::atomic::{AtomicU64, Ordering};
use memmap2::MmapMut;

pub struct ShmWriter {
    mmap: MmapMut,
}

impl ShmWriter {
    pub fn new(path: &str) -> anyhow::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .with_context(|| format!("Failed to create shm file: {path}"))?;

        file.set_len(96)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o644))?;
        }

        // SAFETY: File is freshly created/truncated to 96 bytes.
        // MmapMut requires the file to remain at least this size.
        // The file lives on /dev/shm (tmpfs) and only this process writes to it.
        let mut mmap = unsafe { MmapMut::map_mut(&file)? };
        mmap.fill(0);  // Zero-initialize (contract: sequence=0, timestamp=0, count=0)

        Ok(ShmWriter { mmap })
    }
}
```

### Example 2: Seqlock write with correct memory ordering

```rust
// Source: contract section "Synchronization: Seqlock Protocol"
pub fn write(&mut self, readings: &[ShmReading]) -> anyhow::Result<()> {
    let ptr = self.mmap.as_mut_ptr();

    // SAFETY: ptr is page-aligned (mmap guarantee), so offset 0 is 8-byte aligned.
    // AtomicU64 requires 8-byte alignment. The mmap region is 96 bytes, so
    // all offsets accessed (0..96) are within bounds.
    let seq_ref = unsafe { &*(ptr as *const AtomicU64) };
    let current = seq_ref.load(Ordering::Relaxed);

    // Odd sequence = write in progress
    seq_ref.store(current + 1, Ordering::Release);

    // ... write data fields (regular memory writes) ...

    // Even sequence = write complete
    seq_ref.store(current + 2, Ordering::Release);

    Ok(())
}
```

### Example 3: Writing a 16-byte name field (null-terminated, zero-padded)

```rust
// Source: contract PzemReading layout
unsafe fn write_name(ptr: *mut u8, offset: usize, name: &str) {
    // Zero the entire 16-byte field first
    std::ptr::write_bytes(ptr.add(offset), 0, 16);
    // Copy name bytes (max 15 to leave room for null terminator)
    let bytes = name.as_bytes();
    let len = bytes.len().min(15);
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.add(offset), len);
}
```

### Example 4: Getting Unix epoch nanoseconds as i64

```rust
// Source: std::time docs
let now_ns = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap_or_default()
    .as_nanos() as i64;
// Note: as_nanos() returns u128, cast to i64. This is safe until year 2262.
// The contract specifies i64 for timestamp_ns.
```

### Example 5: Startup initialization in main.rs

```rust
// In main.rs, after InfluxWriter::new() and before the poll loop:
let shm_enabled = cfg.shm.as_ref().map(|s| s.enabled).unwrap_or(false);

if !shm_enabled && cfg.shm.is_some() {
    tracing::info!("Shared memory configured but disabled (enabled = false)");
}

let mut shm_writer: Option<shm::ShmWriter> = if shm_enabled {
    let shm_cfg = cfg.shm.as_ref().unwrap();
    match shm::ShmWriter::new(&shm_cfg.path) {
        Ok(writer) => {
            tracing::info!(path = %shm_cfg.path, "Shared memory writer enabled");
            Some(writer)
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to initialize shared memory writer — continuing without shm");
            None
        }
    }
} else {
    None
};
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `memmap` crate | `memmap2` crate | 2020 (memmap unmaintained) | memmap2 is the maintained fork; drop-in replacement [VERIFIED: crates.io] |
| Manual `libc::mmap` | `memmap2` | N/A | memmap2 provides safe Rust wrapper with Drop-based munmap |

**Deprecated/outdated:**
- `memmap` crate: Unmaintained since 2020. Use `memmap2` instead. [VERIFIED: crates.io shows memmap last updated 2019]

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | AtomicU64 compiles and works on armv7 (32-bit Pi) via lock-based fallback | Pitfall 6 | Compile error on armv7 target; would need to use AtomicU32 or different approach for 32-bit |
| A2 | /dev/shm exists and is tmpfs on Raspberry Pi OS (Debian-based) | /dev/shm section | shm file creation fails at startup; would need alternative path |
| A3 | `as_nanos() as i64` doesn't overflow until year 2262 | Code Example 4 | Incorrect timestamps; extremely unlikely risk |

## Open Questions

1. **Error handling on shm write failure mid-operation**
   - What we know: D-07 says synchronous write. Mmap writes can't really "fail" in normal operation — they're memory writes. The only failure is initialization.
   - What's unclear: Should the method even return `Result<()>` or just be infallible after init?
   - Recommendation: Return `Result<()>` for future-proofing, but in practice the write body is infallible. Log + continue if somehow it fails. Agent's discretion per CONTEXT.md.

2. **Collecting readings during the poll loop**
   - What we know: Current loop fires-and-forgets to InfluxDB. SHM needs all readings collected first.
   - What's unclear: Best way to accumulate without changing the fire-and-forget InfluxDB pattern.
   - Recommendation: Add a `Vec<(&DeviceConfig, PowerReading)>` before the device loop. Push successful readings. After the loop, filter for shm_name and write. InfluxDB fire-and-forget continues as-is (spawned inside the loop).

## /dev/shm on Linux

- `/dev/shm` is a tmpfs filesystem mounted by default on all modern Linux distributions including Raspberry Pi OS (Debian-based). [ASSUMED — A2]
- Files persist across process restarts but are lost on reboot (RAM-backed).
- Default size is typically 50% of RAM. A 96-byte file is negligible.
- File permissions follow standard POSIX model. `0o644` allows any local process to read.
- No special configuration needed on Raspberry Pi. [ASSUMED — A2]

## Sources

### Primary (HIGH confidence)
- **Codebase analysis:** src/config.rs, src/main.rs, src/types.rs, src/influx.rs, src/poller.rs — established patterns for config, modules, poll loop
- **PZEM-DAEMON-CONTRACT.md** — 96-byte binary layout, seqlock protocol, write timing, lifecycle
- **CONTEXT.md** — All 16 locked decisions (D-01 through D-16)
- **cargo search memmap2** — version 0.9.10 confirmed as latest stable [VERIFIED: cargo search 2026-04-28]

### Secondary (MEDIUM confidence)
- **memmap2 API:** `MmapMut::map_mut(&file)`, `as_mut_ptr()`, `fill()` — standard API from docs.rs [ASSUMED — based on training data, consistent with crate description]
- **std::sync::atomic:** `AtomicU64`, `Ordering::Release/Acquire` — stable Rust stdlib [VERIFIED: Rust std docs]

### Tertiary (LOW confidence)
- **AtomicU64 on armv7:** Lock-based fallback behavior [ASSUMED — A1]

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — memmap2 is the only viable crate, version verified
- Architecture: HIGH — all patterns established in codebase, contract fully specified
- Pitfalls: HIGH — well-known mmap patterns, seqlock is textbook
- Integration: HIGH — poll loop structure is clear, insertion point is obvious

**Research date:** 2026-04-28
**Valid until:** 2026-05-28 (stable domain, no fast-moving dependencies)
