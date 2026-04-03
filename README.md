# rs485-logger

> Rust daemon that polls PZEM-016 power meters over Modbus RS485 and writes all measurements to InfluxDB 3.

---

## Overview

`rs485-logger` is a lightweight system daemon written in Rust that continuously reads voltage, current, power, energy, frequency, and power factor from one or more PZEM-016 power meters connected in a Modbus RS485 daisy chain. It exists to fill the gap between cheap, widely available power meters and modern time-series infrastructure: the PZEM-016 has no network interface, so this daemon bridges it to InfluxDB 3 over HTTP.

Key design decisions:

- **Single serial bus, sequential polling** — all PZEM-016 devices share one USB-RS485 adapter; they are polled one at a time to avoid bus contention.
- **One InfluxDB measurement per device** — each device's data lands in its own named measurement (e.g. `solar_panel`, `grid_meter`) for clean querying.
- **Fault-tolerant** — if one device times out or goes offline, the daemon logs a warning and continues polling the remaining devices; no data gaps for healthy devices.
- **Self-recovering** — after 10 consecutive poll cycles where every device fails (e.g. serial adapter disconnected), the daemon exits cleanly so systemd's `Restart=always` restarts it and re-opens the port.

---

## Prerequisites

### Hardware

- **Raspberry Pi** — any model (Pi 2/3/4/5 all supported; see [Build Options](#71-build-options) for architecture targets)
- **PZEM-016 power meter** — one or more; each needs a unique Modbus address (factory default is `1`)
- **USB-to-RS485 adapter** — chips: SiLabs CP2102/CP2104 (`cp210x` driver), WCH CH340/CH341 (`ch341` driver), or FTDI FT232R (`ftdi_sio` driver)
- **A live AC circuit to measure** — the PZEM-016 connects directly to mains voltage (120V or 240V, single-phase)

  > ⚠️ **Electrical safety:** The PZEM-016 connects to mains AC voltage. Follow electrical safety practices. Only work on de-energized circuits when making connections. The current transformer (CT) clamp is safe to install on a live wire — it does not break the circuit.

### Software

- **Raspberry Pi OS** — Bullseye (11) or Bookworm (12); 32-bit or 64-bit
- **InfluxDB 3** — Core (self-hosted) or Cloud (Serverless/Dedicated); a database and API token with write permission are required

---

## Hardware: PZEM-016 Wiring

### 4.1 PZEM-016 Terminal Overview

The PZEM-016 has three connection points:

| Terminal                | Description                                                                                                                                                 |
| ----------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **L / N** (power input) | Connect to the AC line you want to meter. The device needs AC power to operate. For 120V/240V single-phase: L = live, N = neutral.                          |
| **CT clamp**            | Clips around the **live wire (L) only** — does NOT break the circuit. The arrow on the CT body must point **away from the power source** (toward the load). |
| **A / B** (RS485)       | Differential data pair. A = D+ (positive), B = D− (negative).                                                                                               |

### 4.2 USB-RS485 Adapter Wiring

Connect the PZEM-016 RS485 terminals to your USB-RS485 adapter:

```
PZEM-016 RS485-A (D+) ──── Adapter A / D+ / T+
PZEM-016 RS485-B (D−) ──── Adapter B / D− / T−
GND (if present)       ──── GND  (optional but recommended for noise reduction)
```

> **Note:** Adapter labels vary — `A/B`, `D+/D−`, `T+/T−`, and `R+/R−` all refer to the same differential pair. Polarity matters: if the device does not respond, try swapping the A and B wires.

### 4.3 Daisy-Chaining Multiple PZEM-016 Devices

All PZEM-016 devices share the same A/B bus in parallel. Each device must have a **unique Modbus address** (1–247). The factory default is address `1`.

```
Pi USB ── [USB-RS485 Adapter] ── A/B bus ┬── [PZEM-016  addr=1]
                                          ├── [PZEM-016  addr=2]
                                          └── [PZEM-016  addr=3]
```

To assign a unique address to each PZEM-016 **before** wiring them together:

1. Connect one PZEM-016 at a time to the adapter.
2. Use the PZEM Windows configuration software (or a Modbus address-change utility) to write a new address to register `0x0002`.
3. Repeat for each device.

### 4.4 Termination Resistor

For RS485 runs longer than approximately 1 meter, add a **120Ω resistor** across the A and B terminals at the far end of the bus. Most short bench setups (< 1m) work reliably without one.

### 4.5 Connecting the Adapter to Raspberry Pi

Plug the USB-RS485 adapter into any USB port on the Pi. The kernel assigns a device path:

```bash
ls /dev/ttyUSB*
# Typically: /dev/ttyUSB0
```

To identify the port and confirm the driver:

```bash
dmesg | tail -20
# Look for lines containing "cp210x", "ch341", or "ftdi_sio" + port name
```

After installing the udev rule (see [Section 10](#10-udev-rule-stable-device-path)), the adapter will appear as `/dev/ttyRS485`.

---

## Configuration (`config.toml`)

Create a `config.toml` file based on the annotated example below. When running under systemd, place it at `/etc/rs485-logger/config.toml`.

> **Security:** `config.toml` is listed in `.gitignore` — it contains your InfluxDB API token and must not be committed to version control. Use `config.toml.example` as a template.

```toml
# How often to poll all devices (seconds). Minimum: 1. Typical: 10.
poll_interval_secs = 10

# Optional: override log verbosity.
# Values: "error", "warn", "info" (default), "debug", "trace"
# log_level = "debug"

[serial]
# Serial port path.
# Use /dev/ttyRS485 after the udev rule is in place, or /dev/ttyUSB0 for testing.
port = "/dev/ttyRS485"
# Baud rate. PZEM-016 factory default is 9600. Do not change unless you have
# explicitly re-configured the device.
baud_rate = 9600

[influxdb]
# InfluxDB 3 base URL — no trailing slash.
url = "http://192.168.1.100:8086"
# InfluxDB 3 API token.
# Get this from: InfluxDB UI → Load Data → API Tokens → Generate API Token.
token = "your-influxdb-api-token"
# Target database (bucket) name.
# The database is created automatically on the first write if it does not exist.
database = "power"

# One [[devices]] block per PZEM-016.
# Each device lands in its own InfluxDB measurement named by the `name` field.
[[devices]]
address = 1              # Modbus slave address (1–247). Must be unique per device.
name = "solar_panel"     # InfluxDB measurement name. Alphanumeric + underscore only (no spaces or commas).

[[devices]]
address = 2
name = "grid_meter"

# Optional: write logs to a file in addition to the systemd journal.
# The directory must be writable by the rs485logger service user.
# log_file = "/var/log/rs485-logger/rs485.log"

# Optional: reset the energy counter on every device at a fixed time each day.
# Omit this section entirely to disable the feature.
# [energy_reset]
# enabled = true
# timezone = "Asia/Bangkok"   # IANA timezone name (e.g. "UTC", "Asia/Bangkok", "America/New_York")
# time = "00:00"              # Wall-clock time in HH:MM (24-hour)
```

### Configuration Field Reference

| Field                   | Type     | Required | Default  | Notes                                                                                                                       |
| ----------------------- | -------- | -------- | -------- | --------------------------------------------------------------------------------------------------------------------------- |
| `poll_interval_secs`    | `u64`    | ✓        | —        | Seconds between full poll cycles. Minimum: 1.                                                                               |
| `serial.port`           | `string` | ✓        | —        | `/dev/ttyRS485` (with udev) or `/dev/ttyUSB0`                                                                               |
| `serial.baud_rate`      | `u32`    | ✓        | —        | `9600` for PZEM-016 (factory default)                                                                                       |
| `influxdb.url`          | `string` | ✓        | —        | Base URL, no trailing slash                                                                                                 |
| `influxdb.token`        | `string` | ✓        | —        | Bearer token from InfluxDB UI                                                                                               |
| `influxdb.database`     | `string` | ✓        | —        | Database/bucket name — alphanumeric, `_`, `-` only                                                                          |
| `devices[].address`     | `u8`     | ✓        | —        | Modbus address 1–247; must be unique                                                                                        |
| `devices[].name`        | `string` | ✓        | —        | InfluxDB measurement name — alphanumeric and `_` only (no spaces, commas, or special chars)                                 |
| `log_file`              | `string` | —        | none     | Optional file path for persistent log output. Files rotate daily — a date suffix is appended (e.g. `rs485.log.2026-04-03`). |
| `log_level`             | `string` | —        | `"info"` | `error` / `warn` / `info` / `debug` / `trace`                                                                               |
| `energy_reset.enabled`  | `bool`   | —        | —        | Set `false` to disable without removing the section                                                                         |
| `energy_reset.timezone` | `string` | —        | —        | IANA timezone, e.g. `"Asia/Bangkok"`, `"UTC"`                                                                               |
| `energy_reset.time`     | `string` | —        | —        | Reset time in `HH:MM` 24-hour format, e.g. `"00:00"`                                                                        |

---

## InfluxDB 3 Setup

### Self-hosted (InfluxDB 3 Core)

```bash
# Quick start with Docker:
docker run -d --name influxdb3 -p 8086:8086 influxdb:3-core

# Verify it is running:
curl -s "http://localhost:8086/health"
# Expected: {"status":"pass", ...}
```

### InfluxDB Cloud

Use your Cloud cluster URL as `influxdb.url` (e.g. `https://us-east-1-1.aws.cloud2.influxdata.com`).

### Creating an API Token

1. Open the InfluxDB UI in your browser.
2. Navigate to **Load Data → API Tokens**.
3. Click **Generate API Token → All Access Token** (or create a custom token with write access to your target database).
4. Copy the token value — it is only shown once.

> The database specified in `influxdb.database` is **auto-created on the first write**. You do not need to create it manually.

---

## Installation

### 7.1 Build Options

#### Option A — Native build on the Raspberry Pi (recommended for simplicity)

```bash
# Install Rust (on the Pi):
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Clone the repository:
git clone https://github.com/YOUR_REPO/rs485-logger.git
cd rs485-logger

# Build (allow 5–15 minutes for the first build):
cargo build --release

# Binary output:
# target/release/rs485-logger
```

#### Option B — Cross-compile from x86 Linux or macOS (faster iteration)

Requirements: [Docker Desktop](https://www.docker.com/products/docker-desktop/) running, `cross` installed.

```bash
# Install cross (on your dev machine):
cargo install cross --git https://github.com/cross-rs/cross

# Build for Raspberry Pi 4 / Pi 5 / Pi 3 (64-bit OS):
./deploy/build-release.sh

# Build for Raspberry Pi 2 / Pi 3 (32-bit OS):
TARGET=armv7-unknown-linux-gnueabihf ./deploy/build-release.sh

# Binary output (64-bit):
# target/aarch64-unknown-linux-gnu/release/rs485-logger

# Binary output (32-bit):
# target/armv7-unknown-linux-gnueabihf/release/rs485-logger
```

### 7.2 Deploy and Install

```bash
# Copy binary and deploy scripts to the Pi (cross-compile case):
scp target/aarch64-unknown-linux-gnu/release/rs485-logger pi@<PI_IP>:~/rs485-logger
scp deploy/install.sh deploy/rs485-logger.service deploy/99-rs485.rules pi@<PI_IP>:~/deploy/

# SSH into the Pi:
ssh pi@<PI_IP>

# Run the install script as root (requires sudo):
sudo ~/deploy/install.sh ~/rs485-logger
```

The install script performs the following steps automatically:

1. Creates the `rs485logger` system user (no login shell, no home directory)
2. Adds `rs485logger` to the `dialout` group (serial port access)
3. Installs the binary to `/usr/local/bin/rs485-logger`
4. Creates `/etc/rs485-logger/` (config directory, owned `root:rs485logger`, mode `750`)
5. Creates `/var/log/rs485-logger/` (log directory, writable by service user)
6. Installs and enables the systemd service unit
7. Installs the udev rule for `/dev/ttyRS485`

### 7.3 Configure

Place your `config.toml` in the config directory and secure it:

```bash
sudo cp config.toml /etc/rs485-logger/config.toml
sudo chmod 600 /etc/rs485-logger/config.toml
sudo chown rs485logger:rs485logger /etc/rs485-logger/config.toml
```

---

## Running the Daemon

```bash
# Start:
sudo systemctl start rs485-logger

# Check status:
sudo systemctl status rs485-logger

# Watch live logs:
sudo journalctl -u rs485-logger -f

# Stop:
sudo systemctl stop rs485-logger

# Restart after a config change:
sudo systemctl restart rs485-logger

# Disable auto-start on boot:
sudo systemctl disable rs485-logger

# Re-enable auto-start on boot (enabled by default after install):
sudo systemctl enable rs485-logger
```

### Expected Log Output (healthy startup)

```
rs485-logger starting devices=2 interval_secs=10
Next energy reset scheduled next_reset=2026-04-03T00:00:00+07:00
Poll success device=solar_panel
Poll success device=grid_meter
Poll success device=solar_panel
Poll success device=grid_meter
```

When the daily reset fires:

```
Daily energy reset starting
Energy reset OK device=solar_panel
Energy reset OK device=grid_meter
Next energy reset scheduled next_reset=2026-04-04T00:00:00+07:00
```

If `[energy_reset]` is omitted, the startup and reset log lines are absent and the daemon runs in poll-only mode.

Each device is polled in order, every `poll_interval_secs` seconds. A device that fails to respond produces a `WARN` and the daemon moves on to the next device.

#### Fault and recovery log patterns

When a device times out:

```
WARN Device poll failed, skipping device=solar_panel error=Timeout polling device 'solar_panel'
```

When InfluxDB becomes unreachable (first failure only — subsequent failures are suppressed):

```
WARN InfluxDB write failed — suppressing further warnings until restored device=solar_panel error=...
```

When InfluxDB recovers:

```
INFO InfluxDB connection restored
```

When all devices fail 10 consecutive poll cycles (triggers systemd restart):

```
WARN All devices failed this poll cycle consecutive_failures=1
...
WARN All devices failed this poll cycle consecutive_failures=10
ERROR All devices failed 10 consecutive polls — exiting for systemd restart consecutive_failures=10
```

System clock sanity warning (logged at most once if the Pi clock is wrong at boot):

```
WARN System clock appears incorrect (before 2024-01-01) — data may have wrong timestamps timestamp_secs=...
```

---

## Verifying Data in InfluxDB

After starting the daemon, confirm that measurements are landing in InfluxDB.

### Using curl (InfluxDB 3 SQL query API)

```bash
curl -s \
  -H "Authorization: Bearer YOUR_TOKEN" \
  -H "Content-Type: application/json" \
  "http://localhost:8086/api/v3/query_sql" \
  -d '{"db":"power","q":"SELECT * FROM solar_panel ORDER BY time DESC LIMIT 5"}'
```

Expected response: a JSON array with records containing the following fields (all as floats):

| Field          | Unit | Description            |
| -------------- | ---- | ---------------------- |
| `voltage`      | V    | RMS voltage            |
| `current`      | A    | RMS current            |
| `power`        | W    | Active power           |
| `energy`       | Wh   | Accumulated energy     |
| `frequency`    | Hz   | AC frequency           |
| `power_factor` | —    | Power factor (0.0–1.0) |

---

## udev Rule (Stable Device Path)

The file `deploy/99-rs485.rules` creates a `/dev/ttyRS485` symlink that persists across reboots and adapter re-plugs. Without it, the kernel may assign `/dev/ttyUSB0`, `/dev/ttyUSB1`, etc. depending on plug-in order.

The default rule targets the `cp210x` driver (SiLabs CP2102/CP2104 — the most common chip on cheap USB-RS485 adapters). **If your adapter uses a different chip, you must edit the rule before running `install.sh`.**

### Finding Your Adapter's Driver

```bash
udevadm info -a -n /dev/ttyUSB0 | grep -E 'DRIVERS|idVendor|idProduct'
```

Look for the `DRIVERS` line in the `usb` subsystem block:

| Value      | Chip                   | Adapters                                  |
| ---------- | ---------------------- | ----------------------------------------- |
| `cp210x`   | SiLabs CP2102 / CP2104 | Most cheap Amazon/eBay USB-RS485 adapters |
| `ch341`    | WCH CH340 / CH341      | Common on blue USB-RS485 sticks           |
| `ftdi_sio` | FTDI FT232R / FT2232   | Higher-quality adapters                   |

If your adapter uses `cp210x` or `ftdi_sio`, open `deploy/99-rs485.rules` and change `DRIVERS=="ch341"` to match before running `install.sh`:

```bash
# Example: edit rule for ftdi_sio adapters
sudo nano /etc/udev/rules.d/99-rs485.rules
# Change: DRIVERS=="ch341"
# To:     DRIVERS=="ftdi_sio"
```

### Apply Rule Changes Without Rebooting

```bash
sudo udevadm control --reload-rules && sudo udevadm trigger
ls -la /dev/ttyRS485   # symlink should now appear
```

---

## Troubleshooting

| Symptom                                               | Likely Cause                                                | Fix                                                                                                                                               |
| ----------------------------------------------------- | ----------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Failed to read config file: config.toml`             | Config not found at expected path                           | Check that `/etc/rs485-logger/config.toml` exists and is readable by `rs485logger` user                                                           |
| `Failed to open serial port '/dev/ttyRS485'`          | Adapter not plugged in, or udev rule not loaded             | Check `ls /dev/ttyUSB*`; run `sudo udevadm control --reload-rules && sudo udevadm trigger`                                                        |
| `Permission denied: /dev/ttyRS485`                    | `rs485logger` user not in `dialout` group                   | Run `sudo usermod -aG dialout rs485logger` then `sudo systemctl restart rs485-logger`                                                             |
| `Timeout polling device 'X'`                          | Wrong baud rate, wrong Modbus address, or wiring reversed   | Verify `baud_rate = 9600`, confirm device address, try swapping A/B wires                                                                         |
| All devices show timeout but wiring looks correct     | Bus noise or missing termination                            | Add a 120Ω resistor across A/B at far end; reduce cable length                                                                                    |
| `InfluxDB write failed: HTTP 401`                     | Expired or incorrect API token                              | Regenerate token in InfluxDB UI; update `influxdb.token` in `config.toml`; restart daemon                                                         |
| `InfluxDB write failed: connection refused`           | InfluxDB not running, or wrong URL                          | Check InfluxDB service status; verify `influxdb.url` in config (no trailing slash)                                                                |
| InfluxDB write errors logged once then silent         | Expected — suppression is intentional                       | First failure logs `WARN`; subsequent ones are suppressed until connectivity is restored; `INFO InfluxDB connection restored` appears on recovery |
| Daemon crashes immediately on startup                 | Config parse error or invalid field value                   | Run manually to see the error: `rs485-logger --config /etc/rs485-logger/config.toml`                                                              |
| `device name '...' contains invalid characters`       | Device name has spaces, commas, or special chars            | Use only alphanumeric characters and underscores in `devices[].name` (e.g. `solar_panel`)                                                         |
| `influxdb.database '...' contains invalid characters` | Database name has slashes, spaces, or special chars         | Use only alphanumeric characters, underscores, and dashes in `influxdb.database`                                                                  |
| `Unknown timezone '...' in energy_reset`              | Unrecognised IANA timezone string                           | Use a valid IANA name from the [tz database](https://en.wikipedia.org/wiki/List_of_tz_database_time_zones) (e.g. `"Asia/Bangkok"`, `"UTC"`)       |
| Daemon keeps restarting via systemd                   | All devices failing 10+ consecutive polls; intentional exit | Check serial adapter is plugged in; verify RS485 wiring; check `journalctl -u rs485-logger` for the `exiting for systemd restart` error           |
| Daemon starts but no data in InfluxDB                 | Writes are silently failing                                 | Check `journalctl -u rs485-logger -f` for `WARN InfluxDB write failed` lines                                                                      |
| No data after reboot                                  | systemd unit not enabled                                    | Run `sudo systemctl enable rs485-logger`                                                                                                          |
| 32-bit word order produces wrong values               | Hardware word-order deviation                               | PZEM-016 uses low-word-first 32-bit order — verify against physical hardware readings                                                             |

### Manual Startup for Debugging

You can run `rs485-logger` directly to test a config file before relying on systemd:

```bash
# Run as your current user (not rs485logger) for quick tests:
./target/release/rs485-logger --config config.toml

# Or test the installed binary against the system config:
rs485-logger --config /etc/rs485-logger/config.toml
```

#### `--clear` flag (manual energy reset)

The `--clear` flag sends an energy reset command to every configured device and exits immediately, without starting the poll loop. Useful for manually zeroing energy counters outside of the scheduled daily reset:

```bash
rs485-logger --config /etc/rs485-logger/config.toml --clear
```

Output:

```
Energy reset sending command device=solar_panel
Energy reset OK device=solar_panel
Energy reset sending command device=grid_meter
Energy reset OK device=grid_meter
--clear mode: done
```

---

## PZEM-016 Register Map (Reference)

This table documents the Modbus register layout used by the daemon for users who want to understand the raw data or write alternative tooling.

| Register(s)     | Field        | Scale  | Unit | Notes                                   |
| --------------- | ------------ | ------ | ---- | --------------------------------------- |
| `0x0000`        | Voltage      | ÷ 10   | V    | Single 16-bit register                  |
| `0x0001–0x0002` | Current      | ÷ 1000 | A    | 32-bit, **low-word-first**              |
| `0x0003–0x0004` | Power        | ÷ 10   | W    | 32-bit, **low-word-first**              |
| `0x0005–0x0006` | Energy       | × 1    | Wh   | 32-bit, **low-word-first**              |
| `0x0007`        | Frequency    | ÷ 10   | Hz   | Single 16-bit register                  |
| `0x0008`        | Power Factor | ÷ 100  | —    | Single 16-bit register; range 0.00–1.00 |
| `0x0009`        | Alarm status | —      | —    | Not logged by this daemon               |

> **Word order note:** The PZEM-016 encodes 32-bit values in **low-word-first** order (least significant 16-bit word at the lower register address). This deviates from the Modbus standard (big-endian, high-word-first). The daemon handles this automatically.

---

## License

MIT — see [LICENSE](LICENSE) for details.
