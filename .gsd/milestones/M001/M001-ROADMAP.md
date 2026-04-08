# M001: rs485-logger v1.0 MVP

**Vision:** A Rust daemon that polls multiple PZEM-016 power meters connected in a Modbus RS485 daisy chain via USB-to-RS485 adapter on a Raspberry Pi. It reads all available measurements at a configurable interval, writes them into InfluxDB 3, performs a daily energy reset, and runs as a hardened systemd service designed for indefinite unattended operation.

## Success Criteria

- Daemon polls all configured PZEM-016 devices sequentially via Modbus RTU and writes readings to InfluxDB 3
- A single device failure does not crash the daemon or stop polling of other devices
- Daemon runs as a systemd service with `Restart=always` and survives reboots
- Daily energy counter reset fires at 00:00 in configured timezone (Asia/Bangkok)
- All 14 daemon reliability findings resolved; daemon runs indefinitely without operator intervention

## Slices

- [x] **S01: Foundation** `risk:medium` `depends:[]`
  > After this: TOML config parsing, PowerReading struct, and register decoder are unit-tested and correct. Project compiles for aarch64. ✅ 2026-04-02
- [x] **S02: InfluxDB Integration** `risk:medium` `depends:[S01]`
  > After this: line protocol formatter and InfluxDB 3 HTTP write client validated against a live local instance; float field types locked to prevent schema conflicts. ✅ 2026-04-02
- [x] **S03: Modbus + Poll Loop** `risk:medium` `depends:[S02]`
  > After this: full sequential poll loop running against real PZEM-016 hardware; skip-and-continue error isolation; graceful SIGTERM shutdown; structured logging to journald + file. ✅ 2026-04-02
- [x] **S04: Systemd Deployment** `risk:medium` `depends:[S03]`
  > After this: daemon deployed on Raspberry Pi via systemd with stable `/dev/ttyRS485` udev symlink; cross-compiled aarch64 release binary builds without errors. ✅ 2026-04-02
- [x] **S05: README / Manual** `risk:low` `depends:[S04]`
  > After this: comprehensive E2E README.md covering hardware wiring → RS485 daisy-chain → Pi setup → config → deployment → InfluxDB verification → troubleshooting. ✅ 2026-04-02
- [x] **S06: Daily Energy Reset** `risk:medium` `depends:[S05]`
  > After this: Modbus FC 0x42 fires once per day at 00:00 Asia/Bangkok; chrono-tz bundles IANA timezone data; far_future() parks the select! arm when reset is disabled. ✅ 2026-04-03
- [x] **S07: Daemon Reliability Hardening** `risk:high` `depends:[S06]`
  > After this: all 14 reliability findings resolved — InfluxDB HTTP timeouts, serial recovery via exit+restart, Modbus drain delay, InfluxDB health state machine, log rotation, config validation, git secret hygiene. ✅ 2026-04-03
