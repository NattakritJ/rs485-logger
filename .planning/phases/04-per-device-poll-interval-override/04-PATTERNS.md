# Phase 4: Per-Device Poll Interval Override - Pattern Map

**Mapped:** 2025-07-14
**Files analyzed:** 5 (modify)
**Analogs found:** 5 / 5

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `src/config.rs` | config | transform | `src/config.rs` (self — existing `Option<u64>` fields) | exact |
| `src/main.rs` | controller | event-driven | `src/main.rs` (self — existing poll loop) | exact |
| `src/poller.rs` | service | request-response | `src/poller.rs` (self — test struct literals) | exact |
| `config.toml` | config | — | `config.toml` (self — existing `[[devices]]` blocks) | exact |
| `config.toml.example` | config | — | `config.toml.example` (self — existing `[[devices]]` blocks) | exact |

All files are modifications to existing code. The analogs are the files themselves — the patterns to follow are already established within each file.

## Pattern Assignments

### `src/config.rs` (config, transform)

**Analog:** Self — existing `Option<u64>` config patterns and `validate_config` device loop.

**DeviceConfig struct — add optional field pattern** (lines 45-49):
```rust
#[derive(Debug, serde::Deserialize, Clone)]
pub struct DeviceConfig {
    pub address: u8,
    pub name: String,
    // NEW field follows same Option<u64> pattern as SerialConfig.read_timeout_ms (line 32)
}
```

**Option<u64> field pattern precedent** — `SerialConfig` (lines 30-32):
```rust
    /// Modbus read timeout in milliseconds. Default: 150ms.
    /// Only affects FC 0x04 (read input registers). Energy reset (FC 0x42) always uses 500ms.
    pub read_timeout_ms: Option<u64>,
```

**Global interval validation pattern** — copy for per-device (lines 64-66):
```rust
    if let Some(ms) = cfg.poll_interval_ms {
        anyhow::ensure!(ms > 0, "poll_interval_ms must be > 0");
    }
```

**Device validation loop — insertion point** (lines 81-98):
```rust
    for device in &cfg.devices {
        anyhow::ensure!(
            device.address >= 1 && device.address <= 247,
            "device '{}' has invalid Modbus address {} (must be 1–247)",
            device.name,
            device.address
        );
        anyhow::ensure!(
            !device.name.is_empty(),
            "device at address {} has empty name",
            device.address
        );
        anyhow::ensure!(
            device.name.chars().all(|c| c.is_alphanumeric() || c == '_'),
            "device name '{}' contains invalid characters — use only alphanumeric and underscore (spaces, commas, newlines break InfluxDB line protocol)",
            device.name
        );
        // NEW: per-device poll_interval_ms validation goes here, using same if-let + ensure! pattern
    }
```

**Test struct literal — `test_empty_device_list_rejected`** (lines 171-189):
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
Note: `devices: vec![]` means no `DeviceConfig` struct literal here, but any test that builds `DeviceConfig` directly must add `poll_interval_ms: None`.

---

### `src/main.rs` (controller, event-driven)

**Analog:** Self — existing poll interval resolution, ticker setup, poll loop, CRIT-02 counter, and cycle-too-long warning.

**Global interval resolution — add tick_interval_ms after this** (lines 139-140):
```rust
    let poll_interval_ms: u64 = cfg.poll_interval_ms
        .unwrap_or_else(|| cfg.poll_interval_secs.unwrap_or(10) * 1000);
```

**Sub-500ms warning — remains global-only, no change** (lines 142-149):
```rust
    // D-08: Warn at startup if the interval is very short.
    if poll_interval_ms < 500 {
        tracing::warn!(
            poll_interval_ms = poll_interval_ms,
            "Poll interval is very short ({}ms) — a 5-device cycle takes ~450ms, polling may not complete within the interval",
            poll_interval_ms
        );
    }
```

**Startup logging pattern** (lines 151-155):
```rust
    tracing::info!(
        devices = cfg.devices.len(),
        interval_ms = poll_interval_ms,
        "rs485-logger starting"
    );
```

**Ticker setup — change `poll_interval_ms` to `tick_interval_ms`** (lines 194-201):
```rust
    let mut ticker = tokio::time::interval(
        std::time::Duration::from_millis(poll_interval_ms),
    );
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
```

**Poll loop — tick arm with device iteration** (lines 312-377):
```rust
            _ = ticker.tick() => {
                // D-08: Track cycle duration to warn when poll exceeds configured interval.
                let cycle_start = std::time::Instant::now();

                // CRIT-02: Track whether at least one device succeeded this cycle.
                let mut any_ok = false;

                for device in &cfg.devices {
                    match poller.poll_device(device).await {
                        Ok(reading) => {
                            any_ok = true;
                            tracing::info!(
                                device = %device.name,
                                "Poll success"
                            );
                            // ... InfluxDB write spawn (lines 330-357) ...
                        }
                        Err(e) => {
                            tracing::warn!(
                                device = %device.name,
                                error = %e,
                                "Device poll failed, skipping"
                            );
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        }
                    }
                    poller.bus_delay_read().await;
                }
```
Transform: `for device in &cfg.devices` → `for (i, device) in cfg.devices.iter().enumerate()` with due-check guard.

**CRIT-02 counter — add `any_polled` guard** (lines 379-398):
```rust
                if any_ok {
                    consecutive_all_fail = 0;
                } else {
                    consecutive_all_fail += 1;
                    tracing::warn!(
                        consecutive_failures = consecutive_all_fail,
                        "All devices failed this poll cycle"
                    );
                    if consecutive_all_fail >= MAX_CONSECUTIVE_ALL_FAIL {
                        tracing::error!(
                            consecutive_failures = consecutive_all_fail,
                            "All devices failed {} consecutive polls — exiting for systemd restart",
                            MAX_CONSECUTIVE_ALL_FAIL
                        );
                        break;
                    }
                }
```

**Cycle-too-long warning — change `poll_interval_ms` to `tick_interval_ms`** (lines 400-410):
```rust
                let cycle_ms = cycle_start.elapsed().as_millis();
                if cycle_ms > poll_interval_ms as u128 {
                    tracing::warn!(
                        cycle_ms = cycle_ms as u64,
                        interval_ms = poll_interval_ms,
                        "Poll cycle took {}ms, exceeds configured interval of {}ms — consider reducing read_timeout_ms or device count",
                        cycle_ms,
                        poll_interval_ms
                    );
                }
```

---

### `src/poller.rs` (service, request-response — test struct literals only)

**Analog:** Self — test `DeviceConfig` struct literals.

**Test 1 — `test_poll_device_signature_compiles`** (lines 182-185):
```rust
        let device = DeviceConfig {
            address: 1,
            name: "test_device".to_string(),
        };
```
Add `poll_interval_ms: None,` to this struct literal.

**Test 2 — `test_reset_energy_signature_compiles`** (lines 199-202):
```rust
        let device = DeviceConfig {
            address: 1,
            name: "test_device".to_string(),
        };
```
Add `poll_interval_ms: None,` to this struct literal.

---

### `config.toml` (config file)

**Analog:** Self — existing `[[devices]]` block format.

**Existing device block pattern** (lines 45-48):
```toml
[[devices]]
address = 1              # Modbus slave address (1–247). Must be unique per device.
name = "235_floor_1"     # InfluxDB measurement name. Use snake_case, no spaces.
```
Add commented `# poll_interval_ms = 500` line after the `name` field.

---

### `config.toml.example` (config file)

**Analog:** Self — existing `[[devices]]` block with commented optional fields pattern.

**Existing device block pattern** (lines 51-53):
```toml
[[devices]]
address = 1              # Modbus slave address (1–247). Must be unique per device.
name = "floor_1"         # InfluxDB measurement name. Use alphanumeric and underscore only.
```
Add commented `# poll_interval_ms = 500` with explanation comment.

---

## Shared Patterns

### Option<u64> Config Field Pattern
**Source:** `src/config.rs` lines 30-32 (`read_timeout_ms`) and lines 7-8 (`poll_interval_ms`, `poll_interval_secs`)
**Apply to:** `DeviceConfig.poll_interval_ms` field addition
```rust
// Declaration: Option<u64> — serde handles missing TOML fields as None
pub read_timeout_ms: Option<u64>,

// Resolution: unwrap_or with sensible default
serial.read_timeout_ms.unwrap_or(150)

// Validation: if-let + anyhow::ensure!
if let Some(ms) = cfg.poll_interval_ms {
    anyhow::ensure!(ms > 0, "poll_interval_ms must be > 0");
}
```

### Structured Logging Pattern
**Source:** `src/main.rs` lines 151-155
**Apply to:** New startup log for per-device interval overrides
```rust
tracing::info!(
    devices = cfg.devices.len(),
    interval_ms = poll_interval_ms,
    "rs485-logger starting"
);
```

### Cycle Timer Infrastructure
**Source:** `src/main.rs` lines 314, 401
**Apply to:** Per-tick duration comparison against `tick_interval_ms`
```rust
let cycle_start = std::time::Instant::now();
// ... polling ...
let cycle_ms = cycle_start.elapsed().as_millis();
```

## No Analog Found

No files lack analogs — all modifications follow patterns already established in the codebase.

## Metadata

**Analog search scope:** `src/` directory, root config files
**Files scanned:** 6 source files (`config.rs`, `main.rs`, `poller.rs`, `influx.rs`, `scheduler.rs`, `types.rs`) + 2 config files
**Pattern extraction date:** 2025-07-14
