# S05: README / Manual

**Status:** ✅ Completed 2026-04-02
**Goal:** Write a standalone, end-to-end README.md that takes a user from hardware unboxing through PZEM-016 wiring, RS485 daisy-chain topology, Raspberry Pi connection, config.toml setup, building and deploying the binary, running the systemd daemon, verifying data in InfluxDB 3, and troubleshooting common failure modes.
**Demo:** A user with no prior exposure to the project can wire hardware, configure, deploy, and verify data in InfluxDB using only the README — no source code reading required.

## Must-Haves

- Hardware wiring section (PZEM-016 → RS485 daisy chain → USB adapter → Pi)
- `config.toml` setup guide with all fields explained
- Build and cross-compilation instructions
- systemd deployment walkthrough
- InfluxDB 3 data verification steps
- Troubleshooting section covering common failure modes

## Tasks

- [x] T01: Write comprehensive README.md E2E manual

## Files Likely Touched

- `README.md`
