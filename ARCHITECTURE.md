# Architecture

This document explains how **rs485-logger** works, with particular attention to
Rust-specific concepts for developers coming from Python, JavaScript, or Go.

## 1. High-Level Overview

```
  +-----------+   +-----------+   +-----------+
  | PZEM-016  |   | PZEM-016  |   | PZEM-016  |
  | addr=1    |   | addr=2    |   | addr=N    |
  +-----+-----+   +-----+-----+   +-----+-----+
        |               |               |
        +-------+-------+-------+-------+    RS485 daisy chain
                |                            (shared wire — one device talks at a time)
         +------+------+
         | USB-to-RS485 |
         |   adapter    |
         +------+------+
                |
         +------+------+
         | Raspberry Pi |
         |              |
         | rs485-logger |  ← tokio single-threaded async daemon
         +------+------+
                |  HTTP POST (line protocol)
                v
         +-------------+
         | InfluxDB 3   |
         | /api/v3/     |
         | write_lp     |
         +-------------+
```

The daemon runs a single async loop:

1. **Open** the serial port once at startup
2. **Tick** every N seconds (configurable)
3. **Poll** each PZEM-016 device sequentially (switch Modbus address, read registers)
4. **Write** each reading to InfluxDB 3 via HTTP
5. **Repeat** until SIGTERM or SIGINT

**Why sequential polling?** RS485 is a shared electrical bus — only one device
can transmit at a time. Parallel requests would corrupt frames on the wire.
The single-threaded tokio runtime (`current_thread`) matches this physical
constraint perfectly: no thread synchronisation overhead, no `Send` bounds
on the serial port handle.

---

## 2. Source File Map

| File | Purpose |
|------|---------|
| `src/main.rs` | Entry point: CLI arg parsing, config loading, tracing init, poll loop with graceful shutdown, InfluxDB health tracking, serial recovery counter |
| `src/config.rs` | TOML config structs (`AppConfig`, `SerialConfig`, `InfluxConfig`, `DeviceConfig`, `EnergyResetConfig`) + validation (device names, database name, energy reset timezone/time) |
| `src/types.rs` | `PowerReading` struct and `decode_registers()` — decodes raw Modbus registers into typed readings; system clock sanity warning |
| `src/influx.rs` | `InfluxWriter` and `to_line_protocol()` — formats readings as InfluxDB line protocol, sends via HTTP with 5s connect / 10s request timeouts |
| `src/poller.rs` | `ModbusPoller` — opens serial port, switches Modbus slave address per device, reads input registers, sends FC 0x42 energy reset |
| `src/scheduler.rs` | `next_reset_instant()` — pure timezone-aware scheduling: computes next wall-clock instant for daily energy reset |
| `Cargo.toml` | Rust package manifest — dependencies, features, build profile |
| `config.toml.example` | Annotated config template with placeholder token — safe to commit; copy to `config.toml` and fill in real values |
| `deploy/rs485-logger.service` | systemd service unit for running as a daemon on the Pi |
| `deploy/99-rs485.rules` | udev rule — creates a stable `/dev/ttyRS485` symlink for the USB adapter |
| `deploy/install.sh` | Installs the binary, config, and service on the Pi |
| `deploy/build-release.sh` | Cross-compiles a release binary for ARM |

---

## 3. Data Flow (One Poll Cycle)

Here's exactly what happens each time the interval timer fires:

### Step 1 — Timer tick
```rust
// main.rs:117-119
let mut ticker = tokio::time::interval(
    std::time::Duration::from_secs(cfg.poll_interval_secs),
);
```
`tokio::time::interval` creates a repeating ticker. It fires immediately at
t=0 (so the daemon polls on startup without waiting) and then every
`poll_interval_secs` thereafter.

### Step 2 — Iterate devices
```rust
// main.rs:129-130
_ = ticker.tick() => {
    for device in &cfg.devices {
```
When the ticker fires, we loop through every configured device. The `&` means
we borrow the device list (read-only) — the config is not consumed or moved.

### Step 3 — Switch Modbus address
```rust
// poller.rs:48
self.ctx.set_slave(Slave(device.address));
```
The single Modbus context is reused for all devices. Before each read, we tell
it which slave address to target. This avoids reopening the serial port.

### Step 4 — Read registers with timeout
```rust
// poller.rs:56-63
let regs = tokio::time::timeout(
    Duration::from_millis(500),
    self.ctx.read_input_registers(0x0000, 10),
)
.await
.with_context(|| format!("Timeout polling device '{}'", device.name))?
.with_context(|| format!("Modbus transport error from device '{}'", device.name))?
.with_context(|| format!("Modbus exception from device '{}'", device.name))?;
```
This reads 10 input registers (FC 0x04) starting at address 0x0000 with a
500ms timeout. The triple `?` chain handles three distinct failure modes — see
[Rust Concepts: Result and ?](#resultt-e-and-the--operator) below.

### Step 5 — Decode registers
```rust
// types.rs:33-59
pub fn decode_registers(regs: &[u16], device_name: &str) -> anyhow::Result<PowerReading> {
    let voltage = regs[0] as f64 / 10.0;
    let current = ((regs[2] as u32) << 16 | regs[1] as u32) as f64 / 1000.0;
    // ... frequency, power_factor, etc.
}
```
The 10 raw `u16` registers are decoded into human-readable values. Note the
low-word-first byte order for 32-bit values (current, power, energy) — this is
a PZEM-016 quirk that deviates from standard Modbus convention.

### Step 6 — Format as line protocol
```rust
// influx.rs:12-25
pub fn to_line_protocol(reading: &PowerReading) -> String {
    let ts_ns = reading.timestamp_secs * 1_000_000_000_i64;
    format!(
        "{} voltage={:.4},current={:.4},power={:.4},energy={:.4},frequency={:.4},power_factor={:.4} {}",
        reading.device_name, reading.voltage, reading.current,
        reading.power, reading.energy, reading.frequency,
        reading.power_factor, ts_ns,
    )
}
```
Each reading becomes a single line: `measurement field=val,field=val timestamp`.
The device name _is_ the measurement name — no tags needed. All values use
`{:.4}` (4 decimal places) to ensure they're always written as floats, because
InfluxDB 3 locks field types on first write.

### Step 7 — POST to InfluxDB
```rust
// influx.rs:34-48 (InfluxWriter::new)
let client = reqwest::Client::builder()
    .connect_timeout(std::time::Duration::from_secs(5))
    .timeout(std::time::Duration::from_secs(10))
    .build()
    .context("Failed to build HTTP client")?;

// influx.rs:50-72 (InfluxWriter::write)
pub async fn write(&self, reading: &PowerReading) -> anyhow::Result<()> {
    let body = to_line_protocol(reading);
    let url = format!("{}?db={}&precision=ns", self.url, self.database);
    let response = self.client
        .post(&url)
        .bearer_auth(&self.token)
        .body(body)
        .send()
        .await
        .with_context(|| format!("Failed to connect to InfluxDB at {}", self.url))?;
    // ... check for HTTP 204 success
}
```
The line protocol body is POSTed to `/api/v3/write_lp?db=<DATABASE>&precision=ns`
with a `Bearer` token header. InfluxDB 3 returns HTTP 204 on success.

The HTTP client is built once at startup with a **5-second connect timeout** and
a **10-second total request timeout** (covering the entire lifecycle including
response body read). If InfluxDB is unreachable, these timeouts bound the worst-case
latency per write attempt rather than blocking indefinitely.

---

## 4. Rust Concepts for Non-Rust Developers

### Ownership & Borrowing (`&`, `&mut`)

Rust tracks who "owns" each value. When you see `&cfg.serial`, that's a
**borrow** — a read-only reference that can't modify the data. `&mut self`
means "I need exclusive, mutable access". The compiler enforces at compile
time that you can't have a mutable reference and a read-only reference to
the same thing simultaneously — this prevents data races without a garbage
collector.

**Compare to:** `const&` in C++, `readonly` in TypeScript, or passing a
pointer in Go — except Rust enforces the rules at compile time, not by
convention.

### `Result<T, E>` and the `?` Operator

Rust has no exceptions. Every function that can fail returns
`Result<OkValue, ErrorValue>`. The `?` operator is shorthand for:
"if this is an `Err`, return early with the error; otherwise unwrap the `Ok`
value and continue."

The triple `?` chain in `poller.rs:56-63` handles three nested Results:

```
tokio::time::timeout(...)     → Result<_, Elapsed>        ← timeout expired?
  .ctx.read_input_registers() → Result<_, TransportError>  ← serial/IO error?
    inner Result              → Result<Vec<u16>, ExceptionCode> ← device NAK?
```

Each `.with_context(|| "msg")?` adds a human-readable message if that layer
fails, then propagates the error upward. The caller sees a nicely chained
error like: `Timeout polling device 'solar_panel'`.

**Compare to:** `try/except` with re-raise in Python, `if err != nil { return err }`
in Go, `try/catch` in JS — but the compiler forces you to handle every error.

### `anyhow::Result` and `.with_context()`

`anyhow` is an error library for applications (not libraries). It lets you
chain context strings onto errors, producing error messages like:
`Failed to open serial port '/dev/ttyUSB0': No such file or directory`.

`with_context(|| "msg")?` adds `"msg"` to the error if the operation fails.
The closure `|| "msg"` means the message string is only allocated on the error
path (zero cost on the happy path).

**Compare to:** Wrapping exceptions in Python (`raise NewError("context") from e`),
`fmt.Errorf("context: %w", err)` in Go.

### `#[derive(Debug, Deserialize)]`

Rust's compile-time code generation. Putting `#[derive(Deserialize)]` above a
struct tells the compiler to auto-generate TOML/JSON parsing code for that
struct at compile time. No reflection, no runtime cost, no magic — the parser
code literally exists in the compiled binary as if you'd written it by hand.

```rust
// config.rs:5-14
#[derive(Debug, serde::Deserialize)]
pub struct AppConfig {
    pub poll_interval_secs: u64,
    pub serial: SerialConfig,
    // ...
}
```

This single annotation means `toml::from_str::<AppConfig>(text)?` just works.

**Compare to:** Decorators in Python (`@dataclass`), but these run at compile
time. Closest to Go's struct tags (`json:"name"`) or Java's annotation
processors.

### `async`/`await` and `tokio`

Rust's async is zero-cost: no garbage collector, no green threads, no hidden
allocations. An `async fn` returns a state machine (a `Future`) that does
nothing until you `.await` it.

`tokio` is the runtime that drives these futures. The attribute:
```rust
#[tokio::main(flavor = "current_thread")]
```
means "run everything on a single OS thread." This is appropriate here
because RS485 is inherently sequential — there's nothing to parallelise.

**Compare to:** `async/await` in Python (asyncio) or JavaScript (Promises) —
same concept, but Rust's version compiles down to a state machine with no
runtime overhead.

### `tokio::select!`

Waits on multiple async operations simultaneously and runs whichever branch
completes first.

```rust
// main.rs:225-342
tokio::select! {
    biased;

    _ = &mut reset_sleep, if reset_enabled => {
        // Daily energy reset arm — checked first (biased) so it wins
        // over the ticker when both fire at midnight simultaneously.
    }
    _ = ticker.tick() => {
        // Poll all devices sequentially; track consecutive failures.
    }
    _ = &mut shutdown => {
        tracing::info!("Shutdown signal received, exiting cleanly");
        break;
    }
}
```

The `biased` keyword makes `select!` check arms in declaration order rather than
randomly. This matters at midnight: both the reset timer and the poll ticker may
fire simultaneously. Without `biased`, the ticker occasionally wins and sends FC
0x04 read commands to devices; the reset FC 0x42 then arrives while the bus is
still settling, causing the second device to time out.

**Compare to:** `select` in Go, `Promise.race()` in JS,
`asyncio.wait(return_when=FIRST_COMPLETED)` in Python.

### `mod` and `use` (Modules)

Each `.rs` file is a module. In `main.rs`:

```rust
mod config;    // tells the compiler to include src/config.rs
mod influx;    // tells the compiler to include src/influx.rs

use config::load_config;   // import a specific function
use influx::InfluxWriter;  // import a specific struct
```

**Compare to:** `import config` in Python, `import { loadConfig } from './config'`
in JS, `import "project/config"` in Go.

### `pub` Visibility

Everything in Rust is private by default. `pub` makes it accessible from other
modules. Each item needs explicit opt-in:

```rust
pub struct AppConfig { ... }    // struct is public
    pub poll_interval_secs: u64 // field is public
pub fn load_config() { ... }   // function is public
```

If you remove `pub` from a field, other modules can't read it — the compiler
will refuse to compile.

**Compare to:** Default private in Java/Kotlin/C#. Opposite of Python/JS where
everything is public by default.

### `Option<T>` (Nullable Types)

Rust has no `null`. Instead, `Option<T>` means "might be a `T`, might be
`None`".

```rust
// config.rs:12-13
pub log_file: Option<String>,   // this field is optional in TOML
pub log_level: Option<String>,
```

If the TOML file omits `log_file`, it deserializes as `None`. Code that uses
it must explicitly handle the `None` case (the compiler won't let you forget).

**Compare to:** `Optional[str]` in Python, `string | undefined` in TypeScript,
`*string` (nil pointer) in Go — except Rust enforces handling at compile time.

### Lifetimes and `String` vs `&str`

Rust tracks how long references are valid. In practice for this codebase:

- `String` = owned, heap-allocated text (like `std::string` in C++, or a normal
  string in Python/JS). Used in struct fields because the struct owns its data.
- `&str` = a borrowed view into a string. Used in function parameters when you
  only need to read the text.

```rust
pub fn decode_registers(regs: &[u16], device_name: &str) -> ...
//                            ^^^^                   ^^^^
//                     borrowed slice          borrowed string
```

You don't need to deeply understand lifetimes to work in this codebase — just
know that `String` = owned, `&str` = borrowed reference.

### `#[cfg(test)]` and `mod tests`

Test modules live inside the same file as the code they test:

```rust
// types.rs:62-63
#[cfg(test)]
mod tests {
    use super::*;  // import everything from the parent module
    // ... test functions ...
}
```

`#[cfg(test)]` means this code is only compiled when running `cargo test` —
it doesn't exist in the release binary. `use super::*` imports everything from
the parent module so tests can access private functions.

---

## 5. Error Handling Strategy

The daemon is designed to stay alive through transient failures. Only
unrecoverable errors at startup cause an exit.

| Failure | Handling | Severity |
|---------|----------|----------|
| **Device poll timeout/error** | `warn!` log + 100ms bus drain delay + skip device, continue to next | Non-fatal — one device offline doesn't affect others |
| **All devices fail (transient)** | `warn!` per cycle with `consecutive_failures` counter | Non-fatal — bus hiccup or brief cable issue |
| **All devices fail 10+ consecutive cycles** | `error!` log + `break` → exit code 1 → systemd `Restart=always` re-opens port | Controlled exit — serial adapter likely disconnected or stuck |
| **InfluxDB write error (first)** | `warn!` log with error detail; `influx_healthy = false` | Non-fatal — data gap, but daemon stays alive |
| **InfluxDB write error (subsequent)** | Silently dropped while `influx_healthy == false` — no log spam | Non-fatal — suppressed to avoid flooding the journal |
| **InfluxDB connection restored** | `info!` log; `influx_healthy = true` | Recovery — normal writes resume |
| **Config file missing/invalid** | `eprintln!` + `exit(1)` | Fatal — happens before logging is even initialized |
| **Config field invalid** (bad device name, bad database name, bad timezone/time) | `eprintln!` + `exit(1)` via `validate_config` | Fatal — caught at startup before any I/O |
| **Serial port open failure** | Error bubbles up via `?` → daemon exits | Fatal — can't recover without hardware reconnect |
| **SIGTERM / SIGINT** | Clean shutdown via `tokio::select!` | Normal — systemd sends this on `stop`/`restart` |

The pattern in the main loop (`main.rs:270-336`):

```rust
match poller.poll_device(device).await {
    Ok(reading) => {
        any_ok = true;
        // MED-04: InfluxDB health state machine
        if let Err(e) = writer.write(&reading).await {
            if influx_healthy {
                tracing::warn!(error = %e, "InfluxDB write failed — suppressing further warnings until restored");
                influx_healthy = false;
            }
        } else if !influx_healthy {
            tracing::info!("InfluxDB connection restored");
            influx_healthy = true;
        }
    }
    Err(e) => {
        tracing::warn!(error = %e, "Device poll failed, skipping");
        // HIGH-04: drain stale Modbus frames from the serial buffer
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

// CRIT-02: exit after 10 consecutive all-device-fail cycles
if any_ok {
    consecutive_all_fail = 0;
} else {
    consecutive_all_fail += 1;
    if consecutive_all_fail >= MAX_CONSECUTIVE_ALL_FAIL {
        tracing::error!("All devices failed {} consecutive polls — exiting for systemd restart", MAX_CONSECUTIVE_ALL_FAIL);
        break;
    }
}
```

Every device gets its own `match` — one device's failure is isolated from
all others.

---

## 6. Config Validation

All validation happens in `validate_config()` (`src/config.rs:53-102`) before
the daemon opens any I/O. A bad config causes an immediate `exit(1)` with a
descriptive message — never a runtime panic or silent misbehaviour.

| What's validated | Rule | Why |
|-----------------|------|-----|
| `poll_interval_secs` | > 0 | Interval of 0 would spin the CPU |
| `influxdb.url` | non-empty | Empty URL causes an opaque reqwest error |
| `influxdb.token` | non-empty | Empty token causes HTTP 401 on every write |
| `influxdb.database` | alphanumeric + `_` + `-` only | Slashes and query chars would corrupt the `?db=` URL parameter |
| `devices[]` | non-empty list | At least one device is required |
| `devices[].address` | 1–247 | Modbus valid slave range; 0 is broadcast |
| `devices[].name` | alphanumeric + `_` only | Spaces, commas, and newlines break InfluxDB line protocol parsing |
| `energy_reset.timezone` | valid IANA name | Parsed eagerly via `chrono_tz`; unknown names would silently disable resets at runtime |
| `energy_reset.time` | `HH:MM` format | Malformed times can't be parsed into `NaiveTime`; caught at startup |

The energy reset fields are only validated when `energy_reset.enabled = true` —
a disabled but malformed section is ignored.

---

## 7. Deployment Architecture

```
Raspberry Pi
├── /usr/local/bin/rs485-logger        ← compiled binary
├── /etc/rs485-logger/config.toml      ← runtime config
├── /etc/systemd/system/rs485-logger.service
├── /etc/udev/rules.d/99-rs485.rules   ← stable /dev/ttyRS485 symlink
└── /var/log/rs485-logger/             ← optional log file directory
```

### systemd Service

The service unit (`deploy/rs485-logger.service`) runs the daemon as a
dedicated non-root user with minimal privileges:

- **`User=rs485logger`** — dedicated service account, no login shell
- **`SupplementaryGroups=dialout`** — grants serial port access (`/dev/ttyUSB*`)
  without running as root
- **`After=network-online.target`** — waits for network (InfluxDB may be on
  another machine)
- **`Restart=always` + `RestartSec=5`** — auto-restarts on crash after 5 seconds
- **`ProtectSystem=strict`** — read-only filesystem except explicitly allowed paths
- **`ReadWritePaths=/var/log/rs485-logger`** — only directory the daemon can write to

### Logging

Logs go two places:

1. **stderr → journald** — `StandardError=journal` in the service unit means
   all `tracing::info!`, `warn!`, `error!` output goes to the system journal.
   View with: `journalctl -u rs485-logger -f`

2. **Optional file** — if `log_file` is set in `config.toml`, `tracing-appender`
   writes to a **daily rotating file** using a non-blocking writer (file I/O
   doesn't block the async runtime). The date is appended to the filename
   automatically (e.g. `rs485.log.2026-04-03`). Previous days' files are
   retained; add a logrotate rule or cron job if you need automatic pruning.

### udev Rule

The `99-rs485.rules` file creates a stable symlink `/dev/ttyRS485` for the
USB-to-RS485 adapter, so the config doesn't break if the USB device number
changes across reboots.

---

## 8. Daily Energy Reset Scheduling

The energy reset feature (`src/scheduler.rs`) resets the PZEM-016 energy
accumulator register on all devices at a fixed wall-clock time each day
(typically midnight in the local timezone).

### Why a dedicated scheduler module?

Computing "next midnight in Asia/Bangkok" is non-trivial:
- Timezones have DST offsets and historical rule changes
- "Midnight tomorrow" from a 01:00 startup must not fire until the next day
- The Modbus reset command (`FC 0x42`) conflicts with the poll ticker if both
  fire at the same instant

`next_reset_instant()` takes an injectable `now: DateTime<Utc>` so it is unit-
testable without mocking the system clock.

### Scheduling flow

```
startup
  └── validate energy_reset config (timezone + time parsed eagerly)
  └── compute initial_reset_deadline via next_reset_instant(Utc::now(), tz, time)
  └── pin reset_sleep = tokio::time::sleep_until(initial_reset_deadline)

loop (biased select!):
  ┌─ reset arm fires at deadline ──────────────────────────────────┐
  │   for each device: send FC 0x42 → wait → bus_delay()          │
  │   recompute next_deadline = next_reset_instant(Utc::now(), ...) │
  │   reset_sleep.reset(next_deadline)                              │
  └─────────────────────────────────────────────────────────────────┘
  ┌─ ticker fires every poll_interval_secs ────────────────────────┐
  │   for each device: poll → write to InfluxDB                    │
  └─────────────────────────────────────────────────────────────────┘
```

### Why recompute from `Utc::now()` instead of adding 86400s?

Adding a fixed 86400 seconds drifts across DST boundaries. Recomputing the
next midnight from the current wall clock each time correctly handles the
23-hour and 25-hour days that occur during DST transitions.

### `biased` and the midnight collision

When `poll_interval_secs` divides evenly into 86400 (e.g. `10`, `60`), both
the reset timer and the poll ticker fire at exactly midnight UTC. Without
`biased`, `tokio::select!` picks randomly — if the ticker wins, the bus is
busy with FC 0x04 reads when the reset command arrives, causing the second
device's FC 0x42 to time out. With `biased`, the reset arm always wins when
both are ready simultaneously.
