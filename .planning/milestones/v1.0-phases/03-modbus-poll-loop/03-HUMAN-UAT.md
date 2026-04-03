---
status: partial
phase: 03-modbus-poll-loop
source: [03-VERIFICATION.md]
started: 2026-04-02T00:00:00Z
updated: 2026-04-02T00:00:00Z
---

## Current Test

[awaiting human testing]

## Tests

### 1. Single-device poll against real hardware
expected: ModbusPoller.poll_device() returns a PowerReading with plausible values (voltage ~220–240V, frequency ~50Hz, power_factor 0.0–1.0) logged on first poll cycle
result: [pending]

### 2. Multi-device sequential poll produces separate InfluxDB measurements
expected: Two device entries (e.g. solar_panel, grid_meter) each appear as a distinct measurement in InfluxDB after one poll cycle
result: [pending]

### 3. Skip-and-continue: disconnect one PZEM-016 mid-run
expected: Daemon logs WARN with device name and error, continues polling remaining devices; no crash or restart
result: [pending]

### 4. SIGTERM graceful exit timing
expected: kill -SIGTERM <pid> causes INFO "Shutdown signal received" + INFO "rs485-logger stopped" and clean exit within 5 seconds
result: [pending]

## Summary

total: 4
passed: 0
issues: 0
pending: 4
skipped: 0
blocked: 0

## Gaps
