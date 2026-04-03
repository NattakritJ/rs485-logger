---
phase: "01"
plan: "03"
subsystem: types
tags: [types, pzem016, modbus, register-decode, tdd]
dependency_graph:
  requires: [01-01]
  provides: [PowerReading, decode_registers]
  affects: [Phase 2 influx.rs (to_line_protocol), Phase 3 poller.rs (poll loop)]
tech_stack:
  added: []
  patterns:
    - anyhow::anyhow! for returning errors (no panic on bad input)
    - std::time::SystemTime for epoch timestamp (no chrono dependency)
    - float tolerance comparison in tests (not assert_eq!)
    - low-word-first 32-bit reconstruction for PZEM-016
key_files:
  created: []
  modified:
    - src/types.rs
decisions:
  - "D-08 MEDIUM confidence: 32-bit word order (low-word-first) sourced from ESPHome pzemac.cpp — must verify against physical hardware in Phase 3"
  - "#[allow(dead_code)] added at module level — struct not yet used in main.rs; will be removed in Phase 2/3"
  - "unwrap_or_default() on SystemTime is safe — UNIX_EPOCH is always before SystemTime::now()"
metrics:
  duration: "~2 min"
  completed: "2026-04-02"
  tasks_completed: 1
  files_modified: 1
---

# Phase 1 Plan 03: Types / Register Decoder Summary

**One-liner:** PowerReading struct with PZEM-016 low-word-first 32-bit decode and 4 passing unit tests covering basic decode, rollover, zeros, and insufficient registers.

## What Was Built

`src/types.rs` — full implementation:

### Final PowerReading Struct

```rust
#[derive(Debug, Clone)]
pub struct PowerReading {
    pub device_name: String,    // InfluxDB measurement name (from config)
    pub voltage: f64,           // V
    pub current: f64,           // A
    pub power: f64,             // W
    pub energy: f64,            // Wh
    pub frequency: f64,         // Hz
    pub power_factor: f64,      // 0.0–1.0
    pub timestamp_secs: i64,    // Unix epoch seconds (std::time::SystemTime)
}
```

### Final decode_registers() Signature

```rust
pub fn decode_registers(regs: &[u16], device_name: &str) -> anyhow::Result<PowerReading>
```

## Register Map Used

| Index | Register | Field | 32-bit? | Word Order | Scale | Unit |
|-------|----------|-------|---------|------------|-------|------|
| [0] | 0x0000 | voltage | 16-bit | — | ÷ 10.0 | V |
| [1] | 0x0001 | current_lo | 32-bit | low word first | (hi<<16\|lo) ÷ 1000.0 | A |
| [2] | 0x0002 | current_hi | (above) | — | — | — |
| [3] | 0x0003 | power_lo | 32-bit | low word first | (hi<<16\|lo) ÷ 10.0 | W |
| [4] | 0x0004 | power_hi | (above) | — | — | — |
| [5] | 0x0005 | energy_lo | 32-bit | low word first | (hi<<16\|lo) as f64 | Wh |
| [6] | 0x0006 | energy_hi | (above) | — | — | — |
| [7] | 0x0007 | frequency | 16-bit | — | ÷ 10.0 | Hz |
| [8] | 0x0008 | power_factor | 16-bit | — | ÷ 100.0 | — |
| [9] | 0x0009 | alarm | 16-bit | — | (ignored) | — |

**Confidence: MEDIUM** — sourced from ESPHome pzemac.cpp cross-reference. Verify against physical PZEM-016 hardware in Phase 3.

**⚠️ D-08 FLAG: 32-bit word order (low-word-first) is MEDIUM confidence — must verify against hardware in Phase 3.**

## Test Suite

| Test | Input | Expected |
|------|-------|----------|
| `test_basic_decode` | REGS const (2301, 1234, 0, 2852, 0, 10240, 0, 500, 95, 0) | voltage=230.1, current=1.234, power=285.2, energy=10240.0, frequency=50.0, power_factor=0.95 |
| `test_32bit_rollover` | current_lo=0xFFFF, current_hi=0x0001 | current=131.071 A (verifies 32-bit >65535) |
| `test_zero_values` | [0; 10] | all fields = 0.0, no panic |
| `test_insufficient_registers_returns_err` | &REGS[..5] (len=5) | Err (not panic), message contains "5" |

## Verification Results

```
cargo test types -- --nocapture
running 4 tests
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 7 filtered out; finished in 0.00s

cargo test (all): 11 passed; 0 failed
cargo build: Finished `dev` profile — exit 0, zero warnings
```

## Deviations from Plan

None — plan executed exactly as written.

## Self-Check: PASSED
