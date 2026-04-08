# S04: Systemd Deployment

**Status:** ✅ Completed 2026-04-02
**Goal:** Package the daemon for production on Raspberry Pi — systemd service unit, stable `/dev/ttyRS485` udev symlink, serial port permissions, and cross-compiled release binary.
**Demo:** `systemctl start rs485-logger` shows `active (running)`; daemon survives reboot; `/dev/ttyRS485` symlink persists after USB adapter replug; cross-compiled `aarch64-unknown-linux-gnu` binary builds without linker errors.

## Must-Haves

- systemd `.service` unit with `Restart=always`, `RestartSec=5`, no `PrivateDevices=true`
- udev rule creating stable `/dev/ttyRS485` symlink
- `install.sh` deployment script
- `Cross.toml` for aarch64/armv7 targets + `deploy/build-release.sh`

## Tasks

- [x] T01: systemd `.service` unit + udev rule `/dev/ttyRS485` + `install.sh` deployment script
- [x] T02: `Cross.toml` for aarch64/armv7 targets + `deploy/build-release.sh` + cross-compiled binary verification

## Files Likely Touched

- `rs485-logger.service`
- `deploy/99-rs485.rules` (udev)
- `deploy/install.sh`
- `Cross.toml`
- `deploy/build-release.sh`
