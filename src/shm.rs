//! Shared memory writer — writes per-inverter power data to a memory-mapped file
//! using a seqlock protocol for lock-free IPC with the Solar Orchestrator.

use std::fs::OpenOptions;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Context;
use memmap2::MmapMut;

/// Total shared memory file size in bytes (fixed by contract).
const SHM_SIZE: u64 = 96;

/// Maximum number of inverter slots in the binary layout.
const MAX_INVERTERS: usize = 3;

/// A reading paired with its shm_name for writing to shared memory.
pub struct ShmReading<'a> {
    pub shm_name: &'a str,
    pub power_watts: f64,
}

/// Writes per-inverter power data to a memory-mapped file using seqlock synchronization.
pub struct ShmWriter {
    mmap: MmapMut,
}

impl ShmWriter {
    /// Create (or reinitialize) the shared memory file at `path`.
    ///
    /// - Creates the file if it doesn't exist
    /// - Truncates to exactly 96 bytes (reinitializes on restart per D-15)
    /// - Sets file permissions to 0o644
    /// - Zero-fills the entire region
    pub fn new(path: &str) -> anyhow::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .with_context(|| format!("Failed to open shared memory file: {}", path))?;

        file.set_len(SHM_SIZE)
            .with_context(|| format!("Failed to set shared memory file size to {} bytes", SHM_SIZE))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o644);
            file.set_permissions(perms)
                .with_context(|| "Failed to set shared memory file permissions to 0644")?;
        }

        let mut mmap = unsafe {
            MmapMut::map_mut(&file)
                .with_context(|| "Failed to memory-map shared memory file")?
        };

        // Zero-initialize entire region (sequence=0, timestamp=0, count=0, all readings zeroed)
        mmap.fill(0);

        Ok(ShmWriter { mmap })
    }

    /// Write inverter power readings to shared memory using seqlock protocol.
    ///
    /// At most 3 readings are written (contract limit). Excess readings are silently ignored.
    ///
    /// Seqlock protocol (D-11, D-12):
    /// 1. Atomic store: sequence += 1 (odd = writing in progress) with Release ordering
    /// 2. Write data fields — regular memory writes
    /// 3. Atomic store: sequence += 1 (even = write complete) with Release ordering
    pub fn write(&mut self, readings: &[ShmReading<'_>]) -> anyhow::Result<()> {
        let count = readings.len().min(MAX_INVERTERS);

        // Derive all pointers from as_mut_ptr() to keep a single provenance chain
        let base = self.mmap.as_mut_ptr();
        let seq_ptr = base as *mut AtomicU64;
        let seq = unsafe { &*seq_ptr };
        let current = seq.load(Ordering::Relaxed);

        // Step 1: Signal write-in-progress (odd)
        seq.store(current + 1, Ordering::Release);

        // Step 2: Write data fields using raw pointer arithmetic
        unsafe {
            // Offset 8: timestamp_ns (i64)
            let now_ns = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as i64;
            std::ptr::write_unaligned(base.add(8) as *mut i64, now_ns);

            // Offset 16: inverter_count (u8)
            std::ptr::write(base.add(16), count as u8);

            // Offset 17-23: padding (7 bytes of zeros)
            std::ptr::write_bytes(base.add(17), 0, 7);

            // Offset 24, 48, 72: PzemReading entries (24 bytes each)
            for (i, reading) in readings.iter().take(MAX_INVERTERS).enumerate() {
                let entry_offset = 24 + (i * 24);

                // Name: 16 bytes, null-terminated, zero-padded
                let name_bytes = reading.shm_name.as_bytes();
                let name_len = name_bytes.len().min(15);
                std::ptr::write_bytes(base.add(entry_offset), 0, 16);
                std::ptr::copy_nonoverlapping(
                    name_bytes.as_ptr(),
                    base.add(entry_offset),
                    name_len,
                );

                // Power: f64 at offset +16 within the entry
                std::ptr::write_unaligned(
                    base.add(entry_offset + 16) as *mut f64,
                    reading.power_watts,
                );
            }

            // Zero out unused inverter slots
            for i in count..MAX_INVERTERS {
                let entry_offset = 24 + (i * 24);
                std::ptr::write_bytes(base.add(entry_offset), 0, 24);
            }
        }

        // Step 3: Signal write-complete (even)
        seq.store(current + 2, Ordering::Release);

        Ok(())
    }
}
