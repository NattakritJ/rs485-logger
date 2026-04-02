---
phase: 04-systemd-deployment
plan: "01"
subsystem: deployment
tags: [systemd, udev, deploy, raspberry-pi]
dependency_graph:
  requires: [03-modbus-poll-loop]
  provides: [deployment-artifacts, systemd-service, udev-rule, install-script]
  affects: [production-deployment]
tech_stack:
  added: []
  patterns: [systemd-service-hardening, udev-stable-symlink, idempotent-install-script]
key_files:
  created:
    - deploy/rs485-logger.service
    - deploy/99-rs485.rules
    - deploy/install.sh
  modified: []
decisions:
  - "SupplementaryGroups=dialout — serial port access without root, group is dialout on Raspberry Pi OS"
  - "After=network-online.target — ensures InfluxDB writes work on Pi boot before DHCP resolves"
  - "ProtectSystem=strict + ReadWritePaths=/var/log/rs485-logger — matches tracing-appender log path from Phase 3"
  - "cp210x driver in udev rule — covers most common SiLabs CP2102 USB-RS485 adapters with comment to customize"
metrics:
  duration: "~8 min"
  completed: "2026-04-02"
  tasks_completed: 2
  files_changed: 3
---

# Phase 4 Plan 01: Systemd Deployment Artifacts Summary

**One-liner:** Production systemd service unit, udev RS485 symlink rule, and idempotent install script for Raspberry Pi deployment.

## What Was Built

Three deployment artifacts in `deploy/` enabling one-shot install of the rs485-logger daemon on Raspberry Pi OS:

1. **`deploy/rs485-logger.service`** — systemd unit with `Restart=always`, `RestartSec=5`, dedicated `rs485logger` user, `SupplementaryGroups=dialout` for serial access, `After=network-online.target` for boot ordering, and systemd hardening (`ProtectSystem=strict`, `PrivateTmp=yes`, `NoNewPrivileges=yes`).

2. **`deploy/99-rs485.rules`** — udev rule creating `/dev/ttyRS485` stable symlink for USB-RS485 adapters using the `cp210x` (SiLabs) driver, with `MODE="0660"` and `GROUP="dialout"`. Includes documentation block for finding vendor/product IDs, testing without reboot, and common chip driver names.

3. **`deploy/install.sh`** — idempotent bash script that: creates `rs485logger` system user, adds to `dialout` group, installs binary to `/usr/local/bin/`, creates `/etc/rs485-logger/` config dir and `/var/log/rs485-logger/` log dir with correct ownership, installs and enables systemd service, installs and reloads udev rule. Ends with operator next-steps instructions.

## Tasks Completed

| Task | Name | Commit | Files |
|------|------|--------|-------|
| 1 | systemd service unit + udev rule | ec62ccf | deploy/rs485-logger.service, deploy/99-rs485.rules |
| 2 | install.sh deployment script | 1cb0b61 | deploy/install.sh |

## Deviations from Plan

None — plan executed exactly as written.

## Self-Check: PASSED
