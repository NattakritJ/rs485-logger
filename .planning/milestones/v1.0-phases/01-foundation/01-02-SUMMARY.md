---
phase: "01"
plan: "02"
subsystem: config
tags: [config, toml, serde, validation, tdd]
dependency_graph:
  requires: [01-01]
  provides: [AppConfig, SerialConfig, InfluxConfig, DeviceConfig, load_config, validate_config]
  affects: [Phase 3 poller, Phase 2 influx writer]
tech_stack:
  added: []
  patterns:
    - serde::Deserialize with derive macro for TOML deserialization
    - anyhow::ensure! for validation (no custom error types)
    - anyhow::Context for error chaining in load_config
    - inline #[cfg(test)] module with const TOML string (D-11)
key_files:
  created: []
  modified:
    - src/config.rs
decisions:
  - "test_empty_device_list_rejected uses AppConfig constructor directly — TOML inline array (devices=[]) at root requires placement before section headers, so direct struct construction is cleaner for this test case"
  - "#[allow(dead_code)] added at module level — structs/functions are public but not yet used in main.rs; will be removed when Phase 3 wires them in"
metrics:
  duration: "~1 min"
  completed: "2026-04-02"
  tasks_completed: 1
  files_modified: 1
---

# Phase 1 Plan 02: Config Parsing Summary

**One-liner:** AppConfig and all sub-structs with TOML deserialization, startup validation, and 7 passing unit tests.

## What Was Built

`src/config.rs` — full implementation:

### Struct Signatures (Final Public API)

```rust
#[derive(Debug, serde::Deserialize)]
pub struct AppConfig {
    pub poll_interval_secs: u64,
    pub serial: SerialConfig,
    pub influxdb: InfluxConfig,
    pub devices: Vec<DeviceConfig>,
}

#[derive(Debug, serde::Deserialize)]
pub struct SerialConfig {
    pub port: String,
    pub baud_rate: u32,
}

#[derive(Debug, serde::Deserialize)]
pub struct InfluxConfig {
    pub url: String,
    pub token: String,
    pub database: String,
}

#[derive(Debug, serde::Deserialize, Clone)]
pub struct DeviceConfig {
    pub address: u8,   // Modbus address 1–247
    pub name: String,  // InfluxDB measurement name
}
```

### Public API

```rust
pub fn load_config(path: &str) -> anyhow::Result<AppConfig>
pub fn validate_config(cfg: &AppConfig) -> anyhow::Result<()>
```

## Test Suite

| Test | Description | Input | Expected |
|------|-------------|-------|----------|
| `test_happy_path_deserializes_correctly` | 2 devices, all valid | VALID_CONFIG const | Ok, all fields match |
| `test_empty_device_list_rejected` | Empty devices vec | AppConfig with devices=[] | Err containing "device" or "empty" |
| `test_invalid_address_zero_rejected` | address = 0 | TOML with address=0 | Err containing "address" |
| `test_invalid_address_248_rejected` | address = 248 (out of 1–247) | TOML with address=248 | Err containing "address" |
| `test_empty_token_rejected` | token = "" | TOML with token="" | Err containing "token" |
| `test_poll_interval_zero_rejected` | poll_interval_secs = 0 | TOML with poll=0 | Err containing "poll_interval" |
| `test_load_config_file_not_found` | Non-existent file | "nonexistent.toml" | Err (no panic) |

## Verification Results

```
cargo test config -- --nocapture
running 7 tests
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

cargo build: Finished `dev` profile — exit 0, zero warnings
```

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed empty device list test to use direct struct construction**
- **Found during:** GREEN phase — first test run
- **Issue:** TOML inline array `devices = []` at root level gets placed inside `[influxdb]` section by TOML parser because `[influxdb]` section is still "open". The `toml` crate's TOML 1.1 parser requires inline arrays to appear before any `[section]` headers at root scope.
- **Fix:** Changed `test_empty_device_list_rejected` to construct `AppConfig` directly instead of parsing TOML string, then call `validate_config()` — tests the same behavior cleanly
- **Files modified:** `src/config.rs`
- **Commit:** 9109ceb

**2. [Rule 2 - Missing] Added `#[allow(dead_code)]` to suppress dead code warnings**
- **Found during:** GREEN phase — `cargo build` after implementation
- **Issue:** All 6 public items (4 structs + 2 functions) generate "never used" warnings since `main.rs` is still a skeleton
- **Fix:** Added `#![allow(dead_code)]` at module level — will be removed when Phase 3 wires config into the poll loop
- **Files modified:** `src/config.rs`
- **Commit:** 9109ceb

## Self-Check: PASSED
