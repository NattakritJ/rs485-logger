# Pitfalls Research

**Domain:** Rust Modbus RTU data logger — PZEM-016 + RS485 on Raspberry Pi → InfluxDB 3
**Researched:** 2026-04-02
**Confidence:** HIGH (PZEM register layout: ESPHome source + community verified; InfluxDB: official docs; Rust serial/Modbus: crate changelogs + docs; cross-compilation: Cargo official docs)

---

## Critical Pitfalls

### Pitfall 1: PZEM-016 Energy Register Is 32-bit Split Across Two 16-bit Words — Word Order Is Low-High, Not High-Low

**What goes wrong:**
You read registers 0x0003–0x0004 (energy) and naively concatenate them as `(reg[0] << 16) | reg[1]`, giving a wildly wrong kWh value. The PZEM-016 stores the 32-bit energy accumulator as `(high_word << 16) | low_word`, but the two 16-bit Modbus registers arrive as `[low_word, high_word]` — so the correct reconstruction is `(raw[1] as u32) << 16 | (raw[0] as u32)`.

**Why it happens:**
The PZEM datasheet is ambiguous and the register description just says "two registers." The Modbus spec says multi-register values are big-endian (high word first), but PZEM-016 deviates and sends low word first. ESPHome's `pzemac.cpp` source (the most battle-tested reference implementation) confirms: `pzem_get_32bit(i)` does `(pzem_get_16bit(i+2) << 16) | pzem_get_16bit(i+0)` — register `i+0` is low word, `i+2` is high word.

Similarly, current is a 32-bit value at registers 0x0001–0x0002, and active power at 0x0003–0x0005 — all using the same low-high word order.

**How to avoid:**
Use the verified register map from ESPHome `pzemac.cpp`:
```
Offset  Register  Size   Decode
0       0x0000    16-bit voltage = raw / 10.0  (max 6553.5 V)
2       0x0001    32-bit current = (reg[3]<<16|reg[2]) / 1000.0  (low word first)
6       0x0003    32-bit power   = (reg[7]<<16|reg[6]) / 10.0    (low word first)
10      0x0005    32-bit energy  = (reg[11]<<16|reg[10])          (Wh, raw integer)
14      0x0007    16-bit frequency = raw / 10.0
16      0x0008    16-bit power_factor = raw / 100.0
```
Read **10 registers** starting at address 0x0000 (function code 0x04). Request exactly `read_input_registers(addr, 0x0000, 10)`.

**Warning signs:**
- Current, power, or energy values are factor-of-65536 off
- Energy jumps discontinuously (e.g., 1 Wh → 65537 Wh on rollover)
- Integration tests against a real device show non-physical values

**Phase to address:** Serial + Modbus polling phase (Phase 1 / core Modbus integration)

---

### Pitfall 2: PZEM-016 Address Register Uses Non-Standard Function Code 0x06 — Not the Standard Modbus Address Coil

**What goes wrong:**
You try to broadcast to all devices on the bus (address 0xF8) or change/read the stored Modbus address using standard function codes and it silently fails or corrupts the bus. PZEM-016 uses a proprietary function code 0x06 (Write Single Register) to read/write its address register at 0x0002. The "broadcast address" for PZEM is 0xF8, not 0x00 (the Modbus standard).

**Why it happens:**
The PZEM "Modbus" implementation is a simplified subset. It only responds to:
- Function code `0x04` (Read Input Registers) for measurements
- Function code `0x06` (Write Single Register) for the address register at 0x0002
- Function code `0x42` (proprietary) for energy reset

Using `tokio-modbus`'s standard `read_holding_registers` (FC 0x03) will return an exception or no response.

**How to avoid:**
- Only use `read_input_registers` (FC 0x04) to poll measurement data
- Do NOT use holding register reads (FC 0x03) — PZEM won't respond
- Address assignment is a one-time hardware setup step, not a runtime daemon concern
- Use Modbus address 1–16 for each device as configured on the hardware

**Warning signs:**
- `tokio-modbus` returns `ExceptionCode::IllegalFunction` or timeout for every device
- All devices respond identically (accidentally sending to broadcast address 0xF8)

**Phase to address:** Phase 1 (Modbus RTU polling implementation)

---

### Pitfall 3: Insufficient Inter-Request Delay Causes Bus Collisions on Multi-Device Daisy Chain

**What goes wrong:**
You poll multiple PZEM-016 units back-to-back in a tight async loop. Device N is still transmitting its response when you start sending the request to device N+1. The RS485 bus enters an undefined state: garbled bytes, partial frames, CRC errors on subsequent responses, and the entire polling cycle for that interval is corrupted.

**Why it happens:**
RS485 is half-duplex. The USB-RS485 adapter must switch from RX mode to TX mode (RTS/DE signal toggle). Many cheap CH340/CP2102-based adapters have 1–5 ms hardware latency for this direction switch. The PZEM-016 response for 10 registers is ~25 bytes at 9600 baud ≈ 26 ms. If you send the next request before that window fully clears, you collide.

Additionally, `tokio-modbus` v0.8.2 added "clear rx buffer before sending" as a fix for exactly this scenario (see changelog), indicating this is a known real-world problem.

**How to avoid:**
- Add an explicit inter-request delay of **≥ 50 ms** between device polls (100 ms is safe margin)
- Configure a **per-request read timeout of ≥ 500 ms** in `tokio-modbus` to handle slow devices
- Sequence all device polls **serially** (not concurrently) — RS485 is a single shared bus
- Do NOT use `tokio::join!` or `FuturesUnordered` to poll devices in parallel

**Warning signs:**
- Increasing CRC error rate when more devices are added
- Intermittent `Err(TimedOut)` responses that correlate with bus load
- Works with 1 device, fails with 3+

**Phase to address:** Phase 1 (Modbus RTU polling) and Phase 3 (multi-device polling loop)

---

### Pitfall 4: USB-RS485 Adapter Device Path Is Not Stable Across Reboots

**What goes wrong:**
You hardcode `/dev/ttyUSB0` in the config. After a Pi reboot, or after plugging in a second USB device, the adapter appears as `/dev/ttyUSB1` and the daemon fails to open the port with `ENOENT` or `EACCES` but nothing in the logs makes this obvious.

**Why it happens:**
Linux assigns `/dev/ttyUSBn` in USB enumeration order, which is non-deterministic if other USB-serial devices are present (keyboard hubs, another adapter, etc.). CH340 and CP2102 both use `ttyUSB` — no stable naming.

**How to avoid:**
- Use a **udev rule** to create a stable symlink based on USB VID/PID and serial number:
  ```
  # /etc/udev/rules.d/99-rs485.rules
  SUBSYSTEM=="tty", ATTRS{idVendor}=="1a86", ATTRS{idProduct}=="7523", \
    SYMLINK+="ttyRS485"
  ```
- Configure the daemon to use `/dev/ttyRS485` (the symlink)
- Document the udev rule as part of deployment
- For multi-adapter setups, use `ATTRS{serial}` to distinguish

**Warning signs:**
- Daemon starts successfully in dev environment, fails on fresh Pi deployment
- `systemctl status rs485-logger` shows `Error opening serial port: No such file or directory`
- Works after `sudo systemctl restart` if adapter enumeration happens to land back on ttyUSB0

**Phase to address:** Phase 4 (systemd deployment and hardening)

---

### Pitfall 5: Serial Port Permissions — Non-root Daemon Silently Denied Access

**What goes wrong:**
The systemd service runs as a dedicated non-root user (correct for security) but the user is not in the `dialout` group. Opening `/dev/ttyUSB0` returns `EACCES`. The daemon crashes at startup and systemd keeps restarting it — burning restart budget and filling logs.

**Why it happens:**
On Raspberry Pi OS (Debian-based), `/dev/ttyUSB*` are owned by `root:dialout` with mode `0660`. Non-root users must be in the `dialout` group (or the udev rule must set permissions).

**How to avoid:**
- Add the service user to the `dialout` group: `usermod -aG dialout rs485-logger`
- OR use a udev rule to set `MODE="0660", GROUP="rs485-logger"` for the specific device
- Verify with `ls -la /dev/ttyUSB*` during setup
- Add to deployment documentation / install script

**Warning signs:**
- `Permission denied` in daemon logs at startup
- `journalctl -u rs485-logger` shows immediate crash loop
- Works when running as root but fails under systemd unit

**Phase to address:** Phase 4 (systemd deployment)

---

### Pitfall 6: InfluxDB 3 Field Type Is Locked on First Write — Mixed Integer/Float Writes Cause Silent Data Loss

**What goes wrong:**
In one polling cycle you write `power=0i` (integer, because power is 0W), then in the next cycle you write `power=45.2` (float). InfluxDB 3 rejects all subsequent writes to that field because it expects the type established at schema creation time. The write API returns HTTP 400, your error handler logs it, but you continue polling — dropping data silently.

**Why it happens:**
InfluxDB 3 (IOx-based) uses an immutable column schema. The first write to a measurement establishes the data type for each field. The PZEM-016 returns raw 16-bit integers from the hardware but they represent physical quantities (voltage in 0.1V units, power factor in 0.01 units). If you conditionally use integer vs. float formatting based on whether the decimal part is zero, you create a type instability.

**How to avoid:**
- **Always write all numeric fields as `f64` floats** in line protocol (no `i` suffix)
- Apply scaling (e.g., `raw / 10.0`) before writing — never write raw register values
- Line protocol: `voltage=231.5,current=1.234,power=285.0,energy=1024.0,frequency=50.0,power_factor=0.95`
- Keep a schema document noting each field is always float

**Warning signs:**
- HTTP 422 or 400 responses from InfluxDB write endpoint with "field type conflict" in body
- Data gaps in Grafana that correlate with power=0 (common at night for solar monitoring)
- InfluxDB logs show `ERR: column type conflict`

**Phase to address:** Phase 2 (InfluxDB write integration)

---

### Pitfall 7: InfluxDB 3 API Endpoint Differs From v1/v2 — Using Wrong Write Path Gives 404 or Silent Drops

**What goes wrong:**
You target `/write?db=...` (v1 API) or `/api/v2/write` (v2 API). InfluxDB 3 Core/Enterprise responds with 404 or returns success without actually storing data, depending on whether the v1 compatibility layer is enabled. Data appears to be written but queries return no results.

**Why it happens:**
InfluxDB 3 is a completely rewritten storage engine (Apache Arrow/DataFusion). The write API endpoint is `/api/v3/write_lp` for the native v3 API, or `/api/v2/write` if the v2 compatibility shim is enabled. The v1 `/write` endpoint exists only on explicitly configured v1-compat setups. Many examples online still show v1/v2 paths.

**How to avoid:**
- Use the **native v3 write endpoint**: `POST /api/v3/write_lp?db=<bucket>`
- Authentication header: `Authorization: Bearer <token>`
- Content-Type: `text/plain; charset=utf-8`
- Verify the InfluxDB 3 instance version before development begins
- Test with `curl` first: `curl -X POST "http://host:8086/api/v3/write_lp?db=power" -H "Authorization: Bearer TOKEN" --data-raw "test_measurement field=1.0"`

**Warning signs:**
- HTTP 404 from write endpoint
- HTTP 200 but data never appears in queries
- `influx3 query "SELECT * FROM ..."` returns empty results despite writes "succeeding"

**Phase to address:** Phase 2 (InfluxDB integration)

---

### Pitfall 8: Energy Register Rollover Creates Negative Delta or Data Spike in InfluxDB

**What goes wrong:**
The PZEM-016 energy counter is a 32-bit unsigned integer in Wh, maxing out at 4,294,967,295 Wh (≈ 4.3 GWh). In high-consumption installations this wraps to 0. Grafana's "non-negative derivative" shows a massive negative spike; alerts fire; cumulative energy charts are permanently corrupted.

More practically: **the energy register also resets to 0 when power is cut**. Any power outage resets it. This is the much more common rollover scenario in residential/commercial use.

**Why it happens:**
The PZEM-016 stores energy in EEPROM (flash) with limited write cycles, and hardware designs typically reset the register on power loss. The Modbus register just reads whatever is currently stored — it provides no "total lifetime" semantics.

**How to avoid:**
- Write the **raw energy value** to InfluxDB as a gauge, not as a monotone counter
- Track a `last_energy` value in the daemon and detect rollovers: if `new_value < last_value` → reset detected → log warning, still write new value
- In InfluxDB/Grafana, use `non_negative_difference()` or `non_negative_derivative()` to compute energy delta per interval
- Log reset events at `WARN` level with timestamps for auditability

**Warning signs:**
- Energy value drops to 0 in dashboard after a power event
- Negative energy delta in derived calculations
- Energy counter jumps backward when a PZEM is replaced with a new unit

**Phase to address:** Phase 1 (Modbus polling — add reset detection) and Phase 3 (multi-device resilience)

---

### Pitfall 9: Tokio Async Runtime + tokio-serial: Opening Serial Port Outside Runtime Context Panics

**What goes wrong:**
You call `tokio_serial::SerialStream::open(...)` or `tokio_modbus::rtu::connect_slave(...)` in the synchronous `main()` body before calling `tokio::main` or inside a `std::thread::spawn`. The runtime is not active and the call panics with "no reactor running" or produces a port that immediately returns `WouldBlock` on every operation.

**Why it happens:**
`tokio-serial` requires an active Tokio runtime to register the file descriptor with the reactor. The sync `SerialPortBuilder::open()` approach in `tokio-modbus` v0.8.0+ replaced the async `connect()` with synchronous `attach()`, but the `SerialStream` itself must still be created within a runtime context (or via `tokio::task::block_in_place` in a sync context).

**How to avoid:**
- Use `#[tokio::main]` on `main()` — create the serial port **inside** the async context
- Use `tokio_modbus::rtu::attach_slave(serial_builder, slave)` (the v0.8+ API), not the old `connect()` API
- Never store `SerialStream` in a struct that is initialized before the runtime starts

**Warning signs:**
- `thread 'main' panicked at 'there is no reactor running'`
- Port opens without error but every `read_input_registers` call returns `WouldBlock` immediately
- Works in `tokio::test` but panics in `main()`

**Phase to address:** Phase 1 (Modbus RTU async setup)

---

### Pitfall 10: Cross-Compilation Fails Due to Missing `libudev` or OpenSSL — Linker Errors for ARM Targets

**What goes wrong:**
You set up `cargo build --target aarch64-unknown-linux-gnu` and hit linker errors: `cannot find -ludev` or `cannot find -lssl`. The build fails even though the Rust code itself is fine. Alternatively, you add `serialport = "4"` which transitively depends on `libudev` on Linux, and the cross linker can't find the ARM version.

**Why it happens:**
`tokio-serial` → `serialport` → `libudev` (on Linux, for USB hotplug detection). Cross-compilation requires either:
1. ARM sysroot with `libudev-dev:arm64` installed, or
2. Disabling `libudev` with `serialport = { features = [], default-features = false }` (disables USB port listing — acceptable since we use a static `/dev/ttyRS485` path)

**How to avoid:**
- Add to `Cargo.toml`:
  ```toml
  [dependencies]
  serialport = { version = "4", default-features = false }
  # This disables libudev dependency — we use a fixed device path, not USB discovery
  ```
- For cross-compilation: use `cross` (the Rust cross-compilation tool) which handles sysroots automatically:
  ```bash
  cargo install cross
  cross build --release --target aarch64-unknown-linux-gnu
  ```
- OR compile natively on the Pi (slow but zero cross-compilation complexity)
- Add `.cargo/config.toml` with the target linker:
  ```toml
  [target.aarch64-unknown-linux-gnu]
  linker = "aarch64-linux-gnu-gcc"
  ```

**Warning signs:**
- `error: linking with 'aarch64-linux-gnu-gcc' failed: exit status: 1`
- `cannot find -ludev` in linker output
- Build succeeds on x86 host but fails when adding `--target armv7-unknown-linux-gnueabihf`

**Phase to address:** Phase 4 (deployment / build pipeline)

---

### Pitfall 11: systemd Service Hardening Breaks Serial Port Access

**What goes wrong:**
You copy a hardened systemd service template that includes `PrivateDevices=true` or `DeviceAllow=` restrictions. The service fails to start with `Permission denied` opening the serial port — even though the user is in `dialout` — because `PrivateDevices=true` creates a separate `/dev` mount that omits USB serial devices.

**Why it happens:**
`PrivateDevices=true` creates a minimal `devtmpfs` with only pseudo-devices (null, zero, urandom, etc.). USB serial devices (`/dev/ttyUSB*`, `/dev/ttyACM*`) are excluded from this private namespace.

**How to avoid:**
- Do **not** use `PrivateDevices=true` for a serial port daemon
- Use `DeviceAllow` explicitly instead:
  ```ini
  DeviceAllow=/dev/ttyUSB0 rw
  # Or for stable symlink:
  DeviceAllow=/dev/ttyRS485 rw
  ```
- Safe hardening options that work with serial access:
  ```ini
  [Service]
  User=rs485-logger
  NoNewPrivileges=true
  ProtectSystem=strict
  ProtectHome=true
  PrivateTmp=true
  # NOT PrivateDevices=true
  ReadWritePaths=/var/log/rs485-logger
  ```

**Warning signs:**
- `EACCES` or `ENOENT` on serial port open despite correct file permissions
- Service works when run manually as the same user, fails under systemd
- `systemd-analyze security rs485-logger` shows good scores but service won't start

**Phase to address:** Phase 4 (systemd service unit)

---

### Pitfall 12: InfluxDB Line Protocol Timestamp Precision Mismatch Causes Data Rejection or Silent Duplicate Collisions

**What goes wrong:**
You generate timestamps in milliseconds (e.g., `SystemTime::now()` → `.as_millis()`) but send them to InfluxDB without specifying `precision=ms` in the query parameter. InfluxDB 3 defaults to **nanoseconds**. Your 13-digit ms timestamp is interpreted as nanoseconds — placing all writes in year 1970. All data is written to the distant past and queries against the current time range return nothing.

Conversely, if you specify `precision=s` but send nanosecond timestamps, writes are rejected as out-of-range.

**How to avoid:**
- Use `SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as i64` for native nanosecond precision
- OR use `precision=ms` in write URL and `as_millis() as i64`
- Set the precision **explicitly** in the URL: `POST /api/v3/write_lp?db=power&precision=ns`
- Write a unit test that verifies the timestamp written can be queried back within ±5s of `now()`

**Warning signs:**
- Writes return HTTP 200 but queries `SELECT * FROM ... WHERE time > now() - 1h` return 0 rows
- Data appears if you query `WHERE time > '1970-01-01T00:00:01Z'` (milliseconds mistaken for nanoseconds)
- Grafana shows "No data" for the last hour despite active polling

**Phase to address:** Phase 2 (InfluxDB write integration)

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Hardcode `/dev/ttyUSB0` in config | No udev setup needed | Daemon breaks on reboot if enumeration order changes | Never — udev rule is 5 minutes of work |
| Write raw register integers to InfluxDB | Simpler code | Field type conflicts when value hits 0; misleading dashboards | Never — always scale to physical units |
| Ignore HTTP errors from InfluxDB write | Simpler error path | Silent data loss for hours; no observability | Never — log every non-2xx response |
| Use `unwrap()` on serial read | Faster prototype | Daemon crashes on first transient CRC error, defeating resilience goal | Prototype only — replace before first deployment |
| Single global `tokio-modbus` context for all devices | Simpler code | Context holds Modbus state; one device error can poison bus state for others | Acceptable for v1 if devices are polled sequentially with error recovery |
| Skip timestamp in line protocol (let InfluxDB use server time) | No clock sync needed | Poll time ≠ measurement time; drift during InfluxDB overload causes phantom gaps | Acceptable only if sub-second accuracy is irrelevant |

---

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| PZEM-016 via Modbus | Use `read_holding_registers` (FC 0x03) | Use `read_input_registers` (FC 0x04) — PZEM ignores FC 0x03 |
| PZEM-016 32-bit values | Assume Modbus big-endian word order (high word first) | PZEM uses low-word-first; reconstruct as `(reg[n+1] as u32) << 16 \| reg[n] as u32` |
| InfluxDB 3 write API | POST to `/write` (v1) or `/api/v2/write` | POST to `/api/v3/write_lp?db=<bucket>` with Bearer token |
| InfluxDB 3 field types | Mix integer `0i` and float `0.0` for same field | Always use float suffix — NEVER `i` for measurement fields |
| tokio-modbus RTU connect | Call `connect()` (removed in v0.8+) | Use `attach()` / `attach_slave()` — sync, infallible, requires active runtime |
| RS485 multi-device | Poll all devices concurrently | Poll strictly serially with ≥ 50 ms inter-device gap |
| systemd hardening | Add `PrivateDevices=true` from template | Use `DeviceAllow=/dev/ttyRS485 rw` instead — serial devices excluded from private devtmpfs |

---

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Write one line-protocol record per device per field | HTTP connection overhead for 6 fields × N devices per interval | Batch all fields for one device in a single line; batch all devices in one HTTP POST body | Even at 1 device / 10s, connection overhead accumulates on Pi's CPU |
| Create a new HTTP client per write | TLS handshake per poll cycle, high CPU on Pi | Reuse `reqwest::Client` (connection pool) across all writes | With 5+ devices at 5s intervals, CPU spikes noticeable |
| Blocking serial read inside async task without `spawn_blocking` | tokio thread pool starved; timer drift | Use `tokio-serial`'s async `SerialStream` — non-blocking by design | Under load with many devices or short intervals |
| Large `Vec<u8>` allocations per Modbus frame | Minor — 25 bytes per frame | Pre-allocate or use stack buffers; tokio-modbus handles this internally since v0.9 | Not a real concern at this scale |

---

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| InfluxDB token stored world-readable in config file | Token leakage → unauthorized writes, data deletion | `chmod 640 /etc/rs485-logger/config.toml` + `chown root:rs485-logger` |
| Running daemon as root (avoid for convenience) | Serial port exploit or panic → full Pi access | Run as dedicated user; use `dialout` group membership |
| Config file without validation on startup | Bad TOML (e.g., empty device list) causes runtime panics vs. early failure | Validate config at startup before opening serial port; fail fast with clear error |
| Logging InfluxDB token to file at DEBUG level | Token appears in log files | Never log the token value; log only "token configured: yes/no" |

---

## "Looks Done But Isn't" Checklist

- [ ] **Modbus polling:** Verify device address range — PZEM factory default is 0x01; confirm actual addresses assigned to hardware before writing config
- [ ] **Energy rollover:** Daemon must log a warning when energy register value decreases between polls — verify this with a simulated reset test
- [ ] **InfluxDB field types:** After first write, query `SHOW COLUMNS FROM <measurement>` to confirm all numeric fields are `DOUBLE` not `INT64`
- [ ] **Timestamp precision:** Verify written timestamps appear in correct time range with `SELECT * FROM ... WHERE time > now() - 5m`
- [ ] **Serial permissions:** Confirm daemon starts successfully after `reboot` (not just after manual deployment)
- [ ] **udev symlink:** Confirm `/dev/ttyRS485` exists after reboot AND after unplugging/replugging the adapter
- [ ] **systemd restart:** Confirm `Restart=on-failure` and `RestartSec=5s` are in service unit — first device timeout on startup must not kill the daemon permanently
- [ ] **Skip-and-continue:** Disconnect one PZEM device and verify the daemon continues polling the remaining devices without restarting
- [ ] **InfluxDB unavailable:** Kill InfluxDB and verify daemon logs HTTP errors but does NOT crash — then reconnect and verify data resumes

---

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| Wrong 32-bit word order in register decode | LOW | Fix decode function; historical data is permanently wrong but new data correct immediately |
| InfluxDB field type conflict (int vs float) | HIGH | Must drop the measurement and re-create; all historical data lost for that measurement |
| Energy counter reset undetected, corrupted cumulative in Grafana | MEDIUM | Add `WHERE energy > 0` filter to queries; use `non_negative_derivative()` going forward |
| Serial port path instability | LOW | Add udev rule; restart daemon |
| systemd PrivateDevices blocks serial | LOW | Edit service unit; `systemctl daemon-reload && systemctl restart` |
| Timestamp precision mismatch (writes going to 1970) | MEDIUM | Fix precision in write URL; delete incorrect data manually; no historical gap but cleanup needed |

---

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| PZEM 32-bit word order | Phase 1: Modbus polling | Integration test: read one device, assert voltage within 100–260V range |
| PZEM function code (FC 0x04 only) | Phase 1: Modbus polling | Confirm no ExceptionCode in first successful poll |
| Inter-request delay on multi-device bus | Phase 1 (single device) + Phase 3 (multi-device) | Run 3+ device poll for 1 hour, count CRC errors = 0 |
| Serial port path unstable | Phase 4: systemd deployment | Reboot Pi, confirm service starts without intervention |
| Serial port permissions | Phase 4: systemd deployment | Confirm daemon runs under non-root user with dialout group |
| InfluxDB field type conflict | Phase 2: InfluxDB integration | Inspect column schema after first write; regression test for zero-value fields |
| Wrong InfluxDB write endpoint | Phase 2: InfluxDB integration | curl smoke test against exact endpoint before integrating |
| Energy register rollover handling | Phase 1: polling + Phase 3: resilience | Unit test for decreasing energy value; daemon logs WARN |
| tokio-modbus API changes (attach vs connect) | Phase 1: Modbus RTU setup | Compile with latest tokio-modbus; check CHANGELOG for breaking changes |
| Cross-compilation linker/libudev | Phase 4: build + deployment | CI build for aarch64-unknown-linux-gnu passes cleanly |
| systemd PrivateDevices blocks serial | Phase 4: service unit | `systemd-analyze security` + verify `DeviceAllow` works on Pi |
| Timestamp precision mismatch | Phase 2: InfluxDB integration | Assert written records are queryable within 60s of write |

---

## Sources

- **ESPHome PZEM-AC source** (`pzemac.cpp`): Verified register layout, word order, function codes, scaling factors — https://github.com/esphome/esphome/blob/dev/esphome/components/pzemac/pzemac.cpp (HIGH confidence)
- **tokio-modbus CHANGELOG**: `v0.8.2` — "Clear rx buffer before sending to help with error recovery on unreliable physical connections" — https://github.com/slowtec/tokio-modbus/blob/main/CHANGELOG.md (HIGH confidence)
- **tokio-modbus CHANGELOG**: `v0.8.0` — replaced `connect()` with synchronous `attach()` API (HIGH confidence)
- **InfluxDB 3 line protocol reference**: Field type immutability, timestamp precision default (nanoseconds), special characters — https://docs.influxdata.com/influxdb3/cloud-serverless/reference/syntax/line-protocol/ (HIGH confidence)
- **InfluxDB 3 schema design guide**: Field type conflicts, column schema enforcement — https://docs.influxdata.com/influxdb3/cloud-serverless/write-data/best-practices/schema-design/ (HIGH confidence)
- **tokio-serial docs.rs**: Struct/trait layout confirming serialport dependency chain — https://docs.rs/tokio-serial/latest/tokio_serial/ (HIGH confidence)
- **Cargo configuration reference**: Cross-compilation target linker configuration — https://doc.rust-lang.org/cargo/reference/config.html#target (HIGH confidence)
- **influxdata/line-protocol Go README**: Notes on uint64 truncation and type enforcement at encode time (MEDIUM confidence — Go library, but same protocol rules apply)
- **Domain knowledge (HIGH confidence)**: RS485 half-duplex timing, CH340/CP2102 direction-switch latency, Linux udev stable device naming, systemd `PrivateDevices` behavior with `/dev/ttyUSB*`, PZEM-016 power-loss energy reset behavior

---
*Pitfalls research for: Rust Modbus RTU + PZEM-016 + RS485 + InfluxDB 3 on Raspberry Pi*
*Researched: 2026-04-02*
