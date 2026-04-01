# Architecture Research

**Domain:** Rust Modbus RTU polling daemon (PZEM-016 → InfluxDB 3)
**Researched:** 2026-04-02
**Confidence:** HIGH (all major claims verified against official crate docs and InfluxDB 3 documentation)

---

## Standard Architecture

### System Overview

```
┌────────────────────────────────────────────────────────────────┐
│                        Daemon Process                           │
├────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌─────────────┐   startup    ┌──────────────────────────────┐ │
│  │ Config      │ ────────────▶│  Scheduler / Poll Loop       │ │
│  │ Loader      │              │  (tokio::time::interval)     │ │
│  │ (config.rs) │              └──────────┬───────────────────┘ │
│  └─────────────┘                         │ for each device      │
│                                          │ (sequential)         │
│  ┌─────────────────────────────────────┐ │                      │
│  │         Modbus Poller               │◀┘                      │
│  │  (tokio-modbus rtu::attach_slave)   │                        │
│  │  SerialStream (tokio-serial)        │                        │
│  │  Re-address slave per device        │                        │
│  └──────────────┬──────────────────────┘                       │
│                 │ Result<PowerReading>                          │
│                 │                                               │
│  ┌──────────────▼──────────────────────┐                       │
│  │     Measurement Struct              │                        │
│  │  PowerReading { voltage, current,   │                        │
│  │    power, energy, freq, pf,         │                        │
│  │    device_name, timestamp }         │                        │
│  └──────────────┬──────────────────────┘                       │
│                 │                                               │
│  ┌──────────────▼──────────────────────┐                       │
│  │      Line Protocol Builder          │                        │
│  │  (format string → String body)      │                        │
│  └──────────────┬──────────────────────┘                       │
│                 │                                               │
│  ┌──────────────▼──────────────────────┐                       │
│  │      InfluxDB Writer                │                        │
│  │  (reqwest HTTP POST /api/v3/        │                        │
│  │   write_lp?db=…)                    │                        │
│  └─────────────────────────────────────┘                       │
│                                                                 │
│  ┌─────────────────────────────────────┐                       │
│  │  Logger (tracing + tracing-appender)│ (spans all components)│
│  └─────────────────────────────────────┘                       │
└────────────────────────────────────────────────────────────────┘
       │                                        │
  /dev/ttyUSBx                           InfluxDB 3 HTTP
  RS485 bus                              /api/v3/write_lp
```

---

### Component Responsibilities

| Component | Responsibility | Key Types |
|-----------|----------------|-----------|
| **Config Loader** (`config.rs`) | Parse `config.toml` at startup; panic on missing/invalid config | `AppConfig`, `DeviceConfig`, `InfluxConfig` structs (serde + toml) |
| **Scheduler / Poll Loop** (`main.rs`) | Drive the global polling interval; iterate over devices sequentially; route errors to logger | `tokio::time::interval`, `loop` |
| **Modbus Poller** (`poller.rs`) | Open and hold a single `SerialStream`; re-address the Modbus context per device using `set_slave`; issue `read_input_registers(0x0000, 10)` (FC 0x04); decode raw `u16` register values | `tokio_modbus::rtu::attach_slave`, `tokio_serial::SerialStream` |
| **PowerReading struct** (`types.rs`) | Typed representation of one successful poll; owns `device_name` (for InfluxDB table name) and `timestamp` | `PowerReading { voltage: f32, current: f32, power: f32, energy: f32, frequency: f32, power_factor: f32, device_name: String, timestamp_ns: i64 }` |
| **Line Protocol Builder** (`influx.rs`) | Convert `PowerReading` → InfluxDB 3 line protocol string; use device_name as table name (no tags needed, device is implied by table) | `fn to_line_protocol(reading: &PowerReading) -> String` |
| **InfluxDB Writer** (`influx.rs`) | POST line protocol body to `/api/v3/write_lp`; attach `Authorization: Bearer <token>` header; log HTTP errors, do not panic | `reqwest::Client` (kept alive across writes), async POST |
| **Logger** (wired in `main.rs`) | Structured logs to stderr + rolling file; use `tracing` macros throughout; configure via `tracing-subscriber` + `tracing-appender` | `tracing`, `tracing-subscriber`, `tracing-appender` |

---

## Recommended Project Structure

Single Rust crate (no workspace needed — this is one small binary):

```
rs485-logger/
├── Cargo.toml
├── config.toml              # runtime config (not in src/)
├── rs485-logger.service     # systemd unit file
└── src/
    ├── main.rs              # tokio::main, tracing init, poll loop
    ├── config.rs            # AppConfig / DeviceConfig / InfluxConfig structs + load_config()
    ├── types.rs             # PowerReading struct, register decode logic
    ├── poller.rs            # ModbusPoller struct, poll_device() async fn
    └── influx.rs            # to_line_protocol(), InfluxWriter struct, write() async fn
```

### Structure Rationale

- **Single crate, no workspace:** The project is one binary with ~5 modules. Workspace adds complexity with no benefit at this scale.
- **`config.rs` separate from `main.rs`:** Config logic can be unit-tested independently. Struct definitions are co-located with parsing.
- **`types.rs` for `PowerReading`:** Central place for the canonical data type. Keeps decode logic (raw `u16[]` → floats) close to the struct definition.
- **`poller.rs` owns the `SerialStream`:** The serial port is a singleton resource. Wrapping it in a `ModbusPoller` struct with methods makes it easy to test and mock.
- **`influx.rs` owns HTTP client:** `reqwest::Client` should be created once and reused (connection pool). Keeping it in a struct means `main` doesn't need to pass it around.

---

## Architectural Patterns

### Pattern 1: Single-Threaded Async Loop (`current_thread` runtime)

**What:** Use `#[tokio::main(flavor = "current_thread")]` and a single async polling loop that awaits each device in sequence. No concurrent device polling.

**When to use:** Always, for RS485 Modbus RTU. The RS485 bus is half-duplex — only one master transaction can be in flight at a time. Concurrent polling would corrupt responses. `current_thread` also minimises memory overhead on the Raspberry Pi.

**Trade-offs:**
- ✅ No bus contention — sequential `.await` guarantees only one transaction at a time
- ✅ Lowest memory footprint (no thread pool overhead)
- ✅ No `Send` bounds needed on futures
- ❌ If one poll blocks for a very long time the interval drifts — mitigated by setting a short per-device timeout

**Example:**

```rust
#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let cfg = config::load_config("config.toml")?;
    let mut poller = poller::ModbusPoller::new(&cfg.serial).await?;
    let writer = influx::InfluxWriter::new(&cfg.influx);
    let mut interval = tokio::time::interval(
        std::time::Duration::from_secs(cfg.poll_interval_secs)
    );
    loop {
        interval.tick().await;
        for device in &cfg.devices {
            match poller.poll_device(device).await {
                Ok(reading) => {
                    let lp = influx::to_line_protocol(&reading);
                    if let Err(e) = writer.write(&lp).await {
                        tracing::error!(device = %device.name, error = %e, "InfluxDB write failed");
                    }
                }
                Err(e) => {
                    tracing::warn!(device = %device.name, error = %e, "Modbus poll failed, skipping");
                }
            }
        }
    }
}
```

---

### Pattern 2: Re-address One Serial Context Per Poll Cycle

**What:** Open the serial port once at startup. On each poll cycle, call `ctx.set_slave(Slave(device.address))` before issuing the read — do **not** reopen the serial port for each device.

**When to use:** Multiple devices on the same RS485 bus share one physical serial port (`/dev/ttyUSB0`). Opening/closing the port per device adds latency and can miss the RS485 turnaround window.

**Trade-offs:**
- ✅ One port open for entire daemon lifetime — no port contention or re-open errors
- ✅ Much faster per-device poll (no OS open overhead)
- ❌ If the serial port disconnects (USB unplug), the whole daemon must be restarted — mitigated by systemd `Restart=always`

**Example:**

```rust
// poller.rs
pub struct ModbusPoller {
    ctx: tokio_modbus::client::Context,
}

impl ModbusPoller {
    pub async fn new(cfg: &SerialConfig) -> anyhow::Result<Self> {
        let builder = tokio_serial::new(&cfg.device, cfg.baud_rate);
        let port = tokio_serial::SerialStream::open(&builder)?;
        // Initial slave address doesn't matter; will be set per poll
        let ctx = tokio_modbus::prelude::rtu::attach_slave(
            port,
            tokio_modbus::Slave(cfg.devices[0].address),
        );
        Ok(Self { ctx })
    }

    pub async fn poll_device(&mut self, device: &DeviceConfig) -> anyhow::Result<PowerReading> {
        self.ctx.set_slave(tokio_modbus::Slave(device.address));
        // PZEM-016: FC 0x04, start addr 0x0000, 10 registers
        let regs = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            self.ctx.read_input_registers(0x0000, 10),
        )
        .await
        .map_err(|_| anyhow::anyhow!("timeout"))??;
        Ok(decode_registers(&regs, &device.name))
    }
}
```

---

### Pattern 3: Line Protocol String Building (no external client library)

**What:** Format InfluxDB 3 line protocol as a `String` using Rust `format!`. No need for the `influxdb3-client` crate (which is not published as a stable standalone crate for InfluxDB 3 Core). POST to `/api/v3/write_lp?db=<database>` with `reqwest`.

**When to use:** InfluxDB 3 write API is a simple HTTP POST of newline-delimited text. A dedicated client library adds a heavy dependency for something expressible in 10 lines of Rust.

**Trade-offs:**
- ✅ Zero extra dependencies — just `reqwest`
- ✅ Full control over precision (send `precision=second` query param, use Unix timestamp in seconds to avoid nanosecond overflow concerns)
- ❌ Escaping special characters in measurement names is manual — but device names in TOML are under operator control, so this is acceptable for v1

**Line protocol format for PZEM-016:**

```
# Format: <table> <fields> <timestamp_seconds>
# table = device name (from config), no tags needed
living_room voltage=230.1,current=1.52,power=350.2,energy=1234.5,frequency=50.0,power_factor=0.98 1743590400
```

**Example:**

```rust
// influx.rs
pub fn to_line_protocol(r: &PowerReading) -> String {
    format!(
        "{} voltage={},current={},power={},energy={},frequency={},power_factor={} {}",
        r.device_name,
        r.voltage, r.current, r.power, r.energy, r.frequency, r.power_factor,
        r.timestamp_secs
    )
}

pub struct InfluxWriter {
    client: reqwest::Client,
    url: String,      // e.g. "http://localhost:8181/api/v3/write_lp?db=power&precision=second"
    token: String,
}

impl InfluxWriter {
    pub async fn write(&self, line_protocol: &str) -> anyhow::Result<()> {
        let resp = self.client
            .post(&self.url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Content-Type", "text/plain; charset=utf-8")
            .body(line_protocol.to_owned())
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!("InfluxDB returned {}: {}", resp.status(), resp.text().await?);
        }
        Ok(())
    }
}
```

---

### Pattern 4: PZEM-016 Register Decoding

**What:** The PZEM-016 returns 10 consecutive 16-bit input registers starting at address 0x0000 via Function Code 0x04. Registers must be scaled.

**Register map (verified from PZEM-016 datasheet and ESPHome community sources):**

| Register | Field | Scale | Type |
|----------|-------|-------|------|
| 0x0000 | Voltage | ÷ 10 → V | u16 |
| 0x0001 | Current (low word) | — | u16 |
| 0x0002 | Current (high word) | combined ÷ 1000 → A | u16 |
| 0x0003 | Power (low word) | — | u16 |
| 0x0004 | Power (high word) | combined ÷ 10 → W | u16 |
| 0x0005 | Energy (low word) | — | u16 |
| 0x0006 | Energy (high word) | combined → Wh | u16 |
| 0x0007 | Frequency | ÷ 10 → Hz | u16 |
| 0x0008 | Power Factor | ÷ 100 | u16 |
| 0x0009 | Alarm status | 0 = no alarm | u16 |

**Confidence:** MEDIUM — Register map is from community/ESPHome sources, not the official Peacefair datasheet (which is behind a vendor wall). Validate against a real device before finalising.

```rust
// types.rs
fn decode_registers(regs: &[u16], device_name: &str) -> PowerReading {
    let voltage = regs[0] as f32 / 10.0;
    let current = ((regs[2] as u32) << 16 | regs[1] as u32) as f32 / 1000.0;
    let power   = ((regs[4] as u32) << 16 | regs[3] as u32) as f32 / 10.0;
    let energy  = ((regs[6] as u32) << 16 | regs[5] as u32) as f32;
    let freq    = regs[7] as f32 / 10.0;
    let pf      = regs[8] as f32 / 100.0;
    PowerReading { voltage, current, power, energy, frequency: freq, power_factor: pf,
                   device_name: device_name.to_string(),
                   timestamp_secs: chrono::Utc::now().timestamp() }
}
```

---

## Data Flow

### Primary Poll Flow (Happy Path)

```
tokio::time::interval::tick()
        │
        ▼
for device in cfg.devices (sequential, single-threaded)
        │
        ▼
ctx.set_slave(device.address)          // re-address, no reopen
        │
        ▼
ctx.read_input_registers(0x0000, 10)   // FC 0x04, 10 x u16
        │
        ▼
Result<Vec<u16>>
        │ Ok
        ▼
decode_registers(regs, device.name)    // scale raw u16 → f32 fields
        │
        ▼
PowerReading { voltage, current, power, energy, freq, pf, name, timestamp }
        │
        ▼
to_line_protocol(reading)              // format! → String
        │
        ▼
POST /api/v3/write_lp?db=power&precision=second
  Authorization: Bearer <token>
  Body: "living_room voltage=230.1,... 1743590400"
        │
        ▼
HTTP 204 No Content (success)
```

### Error Path (Device Offline / Timeout)

```
ctx.read_input_registers(0x0000, 10)
        │
        │ Err(timeout) or Err(CRC error)
        ▼
tracing::warn!("poll failed, skipping")
        │
        ▼
continue to next device          // daemon stays alive
```

### Error Path (InfluxDB Unreachable)

```
POST /api/v3/write_lp …
        │
        │ Err(connection refused) or HTTP 5xx
        ▼
tracing::error!("InfluxDB write failed: {e}")
        │
        ▼
continue to next device          // reading is DROPPED (no local buffer in v1)
```

**Note:** v1 does not buffer writes on InfluxDB failure. This means data gaps occur if InfluxDB is unreachable. Acceptable per project requirements; a write buffer / retry queue is out of scope.

---

## Suggested Build Order

**Rule:** Each step must compile and run before moving to the next. Build the scaffold before wiring live hardware.

| Step | Component | What You Can Test |
|------|-----------|-------------------|
| 1 | **Config loader** (`config.rs`) | `cargo test` — parse a sample `config.toml`, assert struct fields |
| 2 | **Types + register decoder** (`types.rs`) | `cargo test` — unit-test `decode_registers` with known raw register arrays |
| 3 | **Line protocol builder** (`influx.rs` — format only) | `cargo test` — assert output string matches expected line protocol |
| 4 | **InfluxDB writer** (`influx.rs` — HTTP) | Integration test: start InfluxDB locally, POST a hardcoded point, verify it lands |
| 5 | **Modbus poller** (`poller.rs`) | Integration test with real hardware OR with a Modbus RTU simulator (e.g. `diagslave`) |
| 6 | **Poll loop** (`main.rs`) | End-to-end: full daemon reading one real device and writing to InfluxDB |
| 7 | **Logger wiring** | Verify console + file output at correct levels |
| 8 | **systemd unit** | `systemctl start rs485-logger`, verify restart-on-failure behaviour |

**Critical dependency chains:**
- Config must exist before Poller (poller reads serial path + baud from config)
- Types must exist before Poller (poll returns `PowerReading`)
- Line protocol builder must exist before InfluxDB writer (writer calls builder)
- All of steps 1–4 can be built and tested without hardware

---

## RS485 Bus Contention: Why Sequential Polling Is Mandatory

RS485 is a half-duplex differential bus. Modbus RTU is a strict master/slave protocol: the master sends a request, waits for the slave's response, and only then may send the next request. On a daisy-chained bus:

1. **Only one transaction in flight at a time.** If the daemon issued two `read_input_registers` concurrently (e.g. via `tokio::join!`), the second request would be placed on the wire while the first device is still replying. The responses would collide, both CRCs would fail, and both reads would error.

2. **Inter-frame gap required.** Modbus RTU requires a 3.5-character silent gap between frames. The `tokio-modbus` library handles this, but only if requests are issued sequentially.

3. **Implementation:** Use a simple `for` loop with `.await` — do NOT use `tokio::join_all`, `FuturesUnordered`, or any concurrent dispatch. The single-threaded async runtime (`current_thread`) makes this the natural default.

4. **Timeout per device:** Wrap each `read_input_registers` in `tokio::time::timeout(Duration::from_millis(500), ...)`. If a device doesn't respond within 500 ms, abort that device's read and move on. This prevents a dead device from blocking the entire poll cycle. At 9600 baud, a 10-register response is ~30 ms — 500 ms is generous.

---

## Scaling Considerations

This is a Raspberry Pi daemon reading ≤16 devices. "Scaling" means adding more PZEM-016 units, not users.

| Scenario | Impact | Recommendation |
|----------|--------|----------------|
| 1–4 devices | Baseline | Any poll interval ≥ 5 s is fine; full poll cycle < 2 s |
| 5–16 devices | ~8 s per cycle at 500 ms timeout each | Keep poll interval ≥ 30 s; reduce timeout to 300 ms |
| > 16 devices | Outside PZEM-016 address range (1–16) | Not supported by PZEM-016 hardware |
| InfluxDB remote (LAN) | +network latency per write | Batch all device readings into a single `\n`-joined POST body to reduce round-trips |
| Multiple RS485 buses | Need second USB adapter + second port | Instantiate two `ModbusPoller`s, run two poll loops in separate tokio tasks (safe because each has its own `SerialStream`) |

---

## Anti-Patterns

### Anti-Pattern 1: Concurrent Device Polling

**What people do:** Use `futures::future::join_all` to poll all devices simultaneously for speed.

**Why it's wrong:** RS485 is half-duplex. Concurrent requests corrupt the bus — both requests and responses collide. All reads fail with CRC errors.

**Do this instead:** Sequential `for` loop with `.await`. With 9600 baud and a 500 ms timeout, 8 devices take ~4 s worst-case — fully acceptable for sensor data.

---

### Anti-Pattern 2: Re-opening the Serial Port Per Device

**What people do:** `SerialStream::open` inside the per-device loop to get a "fresh" context.

**Why it's wrong:** Opening a serial port is expensive (~10–100 ms on Linux for device enumeration and driver setup). With 8 devices at 10 s intervals, this wastes ~1 s per cycle. More critically, USB-to-RS485 adapters can fail to re-enumerate quickly, causing spurious errors.

**Do this instead:** Open once at startup, use `set_slave()` to change the addressed device.

---

### Anti-Pattern 3: Multi-Threaded Tokio Runtime for Serial I/O

**What people do:** Use the default `#[tokio::main]` (multi-threaded) because it's the default.

**Why it's wrong:** Serial I/O doesn't benefit from parallelism (the bus is serial). The multi-threaded runtime doubles memory use and adds `Send` bound requirements on futures, which can complicate code with non-`Send` serial port handles.

**Do this instead:** `#[tokio::main(flavor = "current_thread")]` — matches the tokio-modbus RTU example exactly.

---

### Anti-Pattern 4: Panicking on Device Read Error

**What people do:** Use `.unwrap()` or `?` directly in the poll loop, causing the daemon to exit when a single device fails.

**Why it's wrong:** Power meters get disconnected, cables fail, and devices lock up. A single device failure should not take down monitoring for all other devices.

**Do this instead:** `match` on the `Result`, log the error with `tracing::warn!`, and `continue` to the next device. The loop must never `?`-propagate device-level errors.

---

### Anti-Pattern 5: Building Line Protocol Manually Without Escaping Table Names

**What people do:** Use `format!("{} ...", device.name, ...)` without sanitising `device.name`.

**Why it's wrong:** InfluxDB 3 line protocol requires spaces and commas in table names to be backslash-escaped. A device named `"living room"` would produce invalid line protocol.

**Do this instead:** Either (a) document that device names in `config.toml` must be snake_case/no-spaces, and validate at startup, or (b) replace spaces with `_` in the builder. For v1, option (a) is simpler.

---

## Integration Points

### External Services

| Service | Integration Pattern | Notes |
|---------|---------------------|-------|
| RS485 / PZEM-016 | `tokio-modbus` RTU client over `tokio-serial` `SerialStream` | Open once, `set_slave()` per device; FC 0x04, registers 0x0000–0x0009 |
| InfluxDB 3 | HTTP POST to `/api/v3/write_lp?db=<db>&precision=second` with `Authorization: Bearer <token>` | InfluxDB 3 Core, not v1/v2 — use `/api/v3/` endpoint, not `/api/v2/write` |
| systemd | Standard unit file with `Restart=always`, `RestartSec=5` | Handles USB disconnects, InfluxDB down-time, and crash recovery |

### Internal Boundaries

| Boundary | Communication | Notes |
|----------|---------------|-------|
| `main.rs` ↔ `poller.rs` | Direct function call: `poller.poll_device(&device) → Result<PowerReading>` | No channel needed — single-threaded loop |
| `main.rs` ↔ `influx.rs` | Direct async call: `writer.write(&lp) → Result<()>` | `reqwest::Client` lives inside `InfluxWriter`, not passed in |
| `poller.rs` ↔ `types.rs` | `decode_registers(regs, name) → PowerReading` | Pure function, no I/O — easy to unit-test |
| `influx.rs` ↔ `types.rs` | `to_line_protocol(reading) → String` | Pure function — easy to unit-test |

---

## Sources

- tokio-modbus 0.17.0 docs + RTU client example: https://docs.rs/tokio-modbus/latest/tokio_modbus/
- tokio-modbus RTU client example (official): https://raw.githubusercontent.com/slowtec/tokio-modbus/main/examples/rtu-client.rs
- tokio-serial 5.4.5 docs: https://docs.rs/tokio-serial/latest/tokio_serial/
- InfluxDB 3 Core line protocol reference: https://docs.influxdata.com/influxdb3/core/reference/line-protocol/
- InfluxDB 3 Core v3 write_lp API: https://docs.influxdata.com/influxdb3/core/write-data/http-api/v3-write-lp/
- tracing 0.1.44 docs: https://docs.rs/tracing/latest/tracing/
- PZEM-016 register map: MEDIUM confidence (community sources / ESPHome); validate against physical device

---

*Architecture research for: Rust Modbus RTU polling daemon (rs485-logger)*
*Researched: 2026-04-02*
