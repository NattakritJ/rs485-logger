# Phase 5: IPC Shared Memory Writer - Pattern Map

**Mapped:** 2025-01-28
**Files analyzed:** 6 (new/modified)
**Analogs found:** 5 / 6

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `src/shm.rs` | service | file-I/O | `src/influx.rs` (InfluxWriter) | role-match |
| `src/config.rs` | config | transform | self (existing patterns) | exact |
| `src/main.rs` | controller | event-driven | self (existing patterns) | exact |
| `Cargo.toml` | config | N/A | self | exact |
| `config.toml.example` | config | N/A | self | exact |
| `config.toml` | config | N/A | self | exact |

## Pattern Assignments

### `src/shm.rs` (service, file-I/O) — NEW FILE

**Analog:** `src/influx.rs`

**Imports pattern** (influx.rs lines 1-4):
```rust
use anyhow::Context;

use crate::config::InfluxConfig;
use crate::types::PowerReading;
```
→ For shm.rs, adapt to:
```rust
use std::fs::OpenOptions;
use std::sync::atomic::{AtomicU64, Ordering};
use memmap2::MmapMut;
use anyhow::Context;

use crate::types::PowerReading;
```

**Struct pattern** (influx.rs lines 27-33):
```rust
#[derive(Clone)]
pub struct InfluxWriter {
    client: reqwest::Client,
    url: String,
    token: String,
    database: String,
}
```
→ For shm.rs: `ShmWriter` holds `MmapMut` and `_file: std::fs::File` (keep file handle alive). Note: `MmapMut` is NOT Clone, so `ShmWriter` should NOT derive Clone.

**Constructor pattern** (influx.rs lines 35-49):
```rust
impl InfluxWriter {
    pub fn new(config: &InfluxConfig) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .context("Failed to build HTTP client")?;
        let url = format!("{}/api/v3/write_lp", config.url.trim_end_matches('/'));
        Ok(InfluxWriter {
            client,
            url,
            token: config.token.clone(),
            database: config.database.clone(),
        })
    }
```
→ For shm.rs: `ShmWriter::new(path: &str) -> anyhow::Result<Self>` — create file, set_len(96), mmap, zero-init.

**Write method pattern** (influx.rs lines 51-73):
```rust
    pub async fn write(&self, reading: &PowerReading) -> anyhow::Result<()> {
        let body = to_line_protocol(reading);
        // ... do work ...
        Ok(())
    }
```
→ For shm.rs: `pub fn write(&mut self, readings: &[PowerReading]) -> anyhow::Result<()>` — synchronous (not async), takes `&mut self` (writes to mmap), takes slice of readings.

**Test pattern** (influx.rs lines 76-163):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn make_reading(device_name: &str, power: f64) -> PowerReading {
        PowerReading {
            device_name: device_name.to_string(),
            voltage: 230.1,
            current: 1.234,
            power,
            energy: 10240.0,
            frequency: 50.0,
            power_factor: 0.95,
            timestamp_secs: 1700000000,
        }
    }
    // ... unit tests ...
}
```

---

### `src/config.rs` (config, transform) — MODIFY

**Optional config section pattern** (config.rs lines 5-24):
```rust
#[derive(Debug, serde::Deserialize)]
pub struct AppConfig {
    // ... existing fields ...
    // Phase 06: optional daily energy reset
    pub energy_reset: Option<EnergyResetConfig>,
}

#[derive(Debug, serde::Deserialize)]
pub struct EnergyResetConfig {
    pub enabled: bool,
    pub timezone: String,
    pub time: String,
}
```
→ Add `pub shm: Option<ShmConfig>` to `AppConfig`, add `ShmConfig` struct with `enabled: bool, path: String`. Follow exact same `Option<SectionConfig>` pattern as `energy_reset`.

**DeviceConfig with Option field pattern** (config.rs lines 45-50):
```rust
#[derive(Debug, serde::Deserialize, Clone)]
pub struct DeviceConfig {
    pub address: u8,
    pub name: String,
    pub poll_interval_ms: Option<u64>,
}
```
→ Add `pub shm_name: Option<String>` — must keep `Clone` derive.

**validate_config conditional validation pattern** (config.rs lines 107-119):
```rust
    if let Some(ref er) = cfg.energy_reset {
        if er.enabled {
            er.timezone.parse::<chrono_tz::Tz>().map_err(|_| {
                anyhow::anyhow!("Unknown timezone '{}' in energy_reset", er.timezone)
            })?;
            chrono::NaiveTime::parse_from_str(&er.time, "%H:%M").with_context(|| {
                format!(
                    "Invalid time '{}' in energy_reset — expected HH:MM",
                    er.time
                )
            })?;
        }
    }
```
→ Add shm validation block in same style:
```rust
    if let Some(ref shm) = cfg.shm {
        if shm.enabled {
            // D-06: at least one device must have shm_name
            // D-06: no more than 3 devices with shm_name
            // D-06: shm_name values must be unique
            // D-06: shm_name must be ≤15 bytes
        }
    }
```

**Device-level validation loop pattern** (config.rs lines 82-106):
```rust
    for device in &cfg.devices {
        anyhow::ensure!(
            device.address >= 1 && device.address <= 247,
            "device '{}' has invalid Modbus address {} (must be 1–247)",
            device.name,
            device.address
        );
        // ... more per-device checks ...
        if let Some(ms) = device.poll_interval_ms {
            anyhow::ensure!(
                ms > 0,
                "device '{}' has invalid poll_interval_ms {} (must be > 0)",
                device.name, ms
            );
        }
    }
```
→ `shm_name` byte-length checks can go in this existing device loop. Uniqueness + count checks go in the `if let Some(ref shm)` block.

**Test config construction pattern** (config.rs lines 178-197):
```rust
        let cfg = AppConfig {
            poll_interval_secs: Some(10),
            poll_interval_ms: None,
            serial: SerialConfig {
                port: "/dev/ttyUSB0".to_string(),
                baud_rate: 9600,
                read_timeout_ms: None,
                parity: None,
            },
            influxdb: InfluxConfig {
                url: "http://localhost:8086".to_string(),
                token: "my-token".to_string(),
                database: "power".to_string(),
            },
            devices: vec![],
            log_file: None,
            log_level: None,
            energy_reset: None,
        };
```
→ Must add `shm: None` to all existing test AppConfig constructions, and add new tests for shm validation.

**TOML test string pattern** (config.rs lines 135-154):
```rust
    const VALID_CONFIG: &str = r#"
poll_interval_secs = 10

[serial]
port = "/dev/ttyUSB0"
baud_rate = 9600

[influxdb]
url = "http://localhost:8086"
token = "my-token"
database = "power"

[[devices]]
address = 1
name = "solar_panel"

[[devices]]
address = 2
name = "grid_meter"
"#;
```
→ `VALID_CONFIG` does NOT include `[shm]` — tests that `shm: None` works by default (backward compatibility).

---

### `src/main.rs` (controller, event-driven) — MODIFY

**Module declaration pattern** (main.rs lines 1-5):
```rust
mod config;
mod influx;
mod poller;
mod scheduler;
mod types;
```
→ Add `mod shm;` in alphabetical position (after `scheduler`, before `types`).

**Writer initialization at startup pattern** (main.rs lines 206-207):
```rust
    let mut poller = ModbusPoller::new(&cfg.serial)?;
    let writer = InfluxWriter::new(&cfg.influxdb)?;
```
→ Add conditional ShmWriter initialization:
```rust
    let mut shm_writer = if let Some(ref shm_cfg) = cfg.shm {
        if shm_cfg.enabled {
            // ... create ShmWriter, log info ...
            Some(shm::ShmWriter::new(&shm_cfg.path)?)
        } else { None }
    } else { None };
```

**Optional feature startup logging pattern** (main.rs lines 245-247):
```rust
    if !reset_enabled && cfg.energy_reset.is_some() {
        tracing::info!("Energy reset configured but disabled (enabled = false)");
    }
```
→ Follow same pattern for shm disabled logging.

**Integration point: after poll loop device iteration** (main.rs lines 404-438):
```rust
                    // RS-485 inter-frame delay between consecutive device polls.
                    poller.bus_delay_read().await;
                }

                // CRIT-02: Only count failures when at least one device was actually polled.
                if !any_polled {
```
→ Insert shm write call between `poller.bus_delay_read()` loop end (line 404) and the `if !any_polled` block (line 408). The shm write must collect readings with `shm_name` from the current tick and write them synchronously.

**Note on reading collection:** Currently each `reading` is consumed inside the device loop by the `tokio::spawn` InfluxDB write (lines 359-390). For shm, readings need to be collected during the loop. This requires storing successful readings with `shm_name` into a `Vec` during the loop, then passing that vec to `shm_writer.write()` after the loop.

---

### `Cargo.toml` (config) — MODIFY

**Dependency addition pattern** (Cargo.toml lines 10-22):
```toml
[dependencies]
tokio = { version = "1.50.0", features = ["rt-multi-thread", "time", "signal", "io-util", "macros", "sync"] }
tokio-modbus = { version = "0.17.0", default-features = false, features = ["rtu"] }
# ... more deps ...
anyhow = "1.0.102"
chrono = { version = "0.4", features = ["clock"] }
chrono-tz = "0.10"
```
→ Add `memmap2 = "0.9"` after the last dependency (alphabetical among simple deps, or at end).

---

### `config.toml.example` (config) — MODIFY

**Optional section example pattern** (config.toml.example lines 60-64):
```toml
# Optional: daily energy reset (sends Modbus FC 0x42 to all devices at midnight).
# [energy_reset]
# enabled = true
# timezone = "Asia/Bangkok"   # IANA timezone name
# time = "00:00"              # Time to reset (HH:MM, 24-hour)
```
→ Add `[shm]` section (commented out) in similar style before or after `[energy_reset]`.

**Device config example pattern** (config.toml.example lines 51-58):
```toml
[[devices]]
address = 1              # Modbus slave address (1–247). Must be unique per device.
name = "floor_1"         # InfluxDB measurement name. Use alphanumeric and underscore only.
# poll_interval_ms = 500  # Optional: override global poll interval for this device (milliseconds)

[[devices]]
address = 2
name = "floor_2"
```
→ Add `# shm_name = "east"` comment to show the optional field.

---

### `config.toml` (config) — MODIFY

**Minimal change:** Add commented-out `[shm]` section (disabled by default), following same pattern as existing commented-out `[energy_reset]` in config.toml.example.

---

## Shared Patterns

### Error Handling (anyhow)
**Source:** `src/influx.rs` lines 36-41, `src/config.rs` lines 52-58
**Apply to:** `src/shm.rs`, `src/config.rs` validation
```rust
// Constructor errors with context
.context("Failed to ...")?;

// Validation errors with anyhow::ensure!
anyhow::ensure!(condition, "message with {} interpolation", value);
```

### Structured Logging
**Source:** `src/main.rs` lines 165-169, 245-247
**Apply to:** `src/shm.rs` startup, `src/main.rs` shm initialization
```rust
// Startup info log with structured fields
tracing::info!(
    devices = cfg.devices.len(),
    interval_ms = poll_interval_ms,
    "rs485-logger starting"
);

// Optional feature disabled log
if !reset_enabled && cfg.energy_reset.is_some() {
    tracing::info!("Energy reset configured but disabled (enabled = false)");
}
```

### Optional Config Section (Option<SectionConfig>)
**Source:** `src/config.rs` lines 16, 19-24
**Apply to:** `src/config.rs` new ShmConfig
```rust
// On AppConfig:
pub energy_reset: Option<EnergyResetConfig>,

// The struct:
#[derive(Debug, serde::Deserialize)]
pub struct EnergyResetConfig {
    pub enabled: bool,
    // ... fields ...
}
```

### Conditional Initialization at Startup
**Source:** `src/main.rs` lines 239-266
**Apply to:** `src/main.rs` ShmWriter initialization
```rust
let reset_enabled = cfg
    .energy_reset
    .as_ref()
    .map(|r| r.enabled)
    .unwrap_or(false);

if !reset_enabled && cfg.energy_reset.is_some() {
    tracing::info!("Energy reset configured but disabled (enabled = false)");
}
```

### PowerReading struct (data source)
**Source:** `src/types.rs` lines 10-19
**Apply to:** `src/shm.rs` write method
```rust
#[derive(Debug, Clone)]
pub struct PowerReading {
    pub device_name: String,    // Maps to DeviceConfig.name (NOT shm_name)
    pub power: f64,             // W — the field written to shm binary layout
    pub timestamp_secs: i64,    // Unix epoch seconds
    // ... other fields not needed for shm ...
}
```
**Note:** `PowerReading` does not carry `shm_name`. The shm write method needs a way to associate readings with their `shm_name`. Options: (a) pass `(&PowerReading, &str)` tuples, (b) add `shm_name: Option<String>` to PowerReading, or (c) look up shm_name from DeviceConfig by matching `device_name`. Agent's discretion per CONTEXT.md.

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| `src/shm.rs` (seqlock internals) | service | file-I/O | No existing file uses `memmap2`, `AtomicU64`, or raw pointer writes. The struct/constructor/write pattern follows `InfluxWriter`, but the seqlock + mmap internals are novel. Use contract doc (`15-PZEM-DAEMON-CONTRACT.md` Rust Implementation Hints) as the pattern source for the unsafe mmap code. |

## Metadata

**Analog search scope:** `src/` directory (6 files), `Cargo.toml`, `config.toml`, `config.toml.example`
**Files scanned:** 9
**Pattern extraction date:** 2025-01-28
