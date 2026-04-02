---
phase: quick
plan: 260403-0gn
subsystem: cli
tags: [cli, modbus, energy-reset, operator-tooling]
dependency_graph:
  requires: [src/poller.rs:reset_energy, src/main.rs:arg-parsing]
  provides: [--clear CLI flag, manual energy counter reset]
  affects: [src/main.rs]
tech_stack:
  added: []
  patterns: [early-exit branch, reuse existing poller.reset_energy(), skip-and-log per device]
key_files:
  modified: [src/main.rs]
decisions:
  - "--clear shares the same arg-parsing loop as --config (no new parser crate needed)"
  - "Early-exit before InfluxWriter::new — avoids requiring InfluxDB config to be valid for reset-only operation"
  - "Error handling mirrors the daily reset arm: WARN and continue per device (skip-and-log)"
metrics:
  duration: "~4 min"
  completed: "2026-04-03"
  tasks: 1
  files_modified: 1
---

# Quick Task 260403-0gn: Add --clear Parameter for Manual Energy Reset Summary

**One-liner:** Added `--clear` CLI flag that opens the Modbus port, sends FC 0x42 energy reset to every configured device sequentially, then exits — reusing existing `poller.reset_energy()` without touching the poll loop.

## What Was Built

The `--clear` flag allows operators to manually trigger an energy counter reset on all configured PZEM-016 devices without waiting for the daily midnight scheduler or editing the TOML config.

**Behaviour:**
- `rs485-logger --clear` — opens serial port, resets all devices, exits with code 0
- `rs485-logger --clear --config /etc/rs485-logger/config.toml` — same with custom config
- `rs485-logger` (no flag) — unchanged normal poll loop behaviour

## Tasks Completed

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | Add --clear argument parsing and early-exit branch in main.rs | b53d656 | src/main.rs |

## Changes Made

### `src/main.rs`

1. **Arg parsing block** extended to detect `--clear` alongside existing `--config`:
   ```rust
   } else if arg == "--clear" {
       clear = true;
   }
   ```
   Returns a tuple `(config_path, clear_mode)` instead of just `config_path`.

2. **Early-exit branch** inserted after tracing init, before `ModbusPoller::new` (normal-mode):
   ```rust
   if clear_mode {
       tracing::info!("--clear mode: sending energy reset to all devices");
       let mut poller = ModbusPoller::new(&cfg.serial)?;
       for device in &cfg.devices {
           tracing::info!(device = %device.name, "Energy reset sending command");
           match poller.reset_energy(device).await {
               Ok(()) => tracing::info!(device = %device.name, "Energy reset OK"),
               Err(e) => tracing::warn!(device = %device.name, error = %e,
                                         "Energy reset failed, skipping"),
           }
       }
       tracing::info!("--clear mode: done");
       return Ok(());
   }
   ```

3. Normal-mode code (InfluxWriter, ticker, select! loop) is **completely unchanged**.

## Verification

```
$ cargo build 2>&1 | grep -E "^error" || echo "BUILD OK"
BUILD OK
```

Zero errors, zero new warnings.

**Code inspection confirms:**
- `clear_mode` variable present in arg-parsing block
- Early-return branch exists before `InfluxWriter::new` on line 149
- `reset_energy()` is called inside the branch for each `cfg.devices` entry
- `InfluxDB` config is never accessed in `--clear` mode

## Deviations from Plan

None — plan executed exactly as written.

## Known Stubs

None.

## Self-Check: PASSED

- [x] `src/main.rs` modified and committed (b53d656)
- [x] `cargo build` passes with zero errors
- [x] `clear_mode` variable exists in arg-parsing loop
- [x] Early-exit branch present before `InfluxWriter::new`
- [x] `reset_energy()` called per device inside branch
- [x] Normal mode code unchanged
