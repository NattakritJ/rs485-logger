---
scope: full-codebase-daemon-reliability-review
verified: 2026-04-02T17:34:50Z
status: gaps_found
findings: 14
critical: 3
high: 4
medium: 5
low: 2
gaps:
  - id: CRIT-01
    title: "InfluxDB HTTP requests have no timeout — can hang the daemon indefinitely"
    severity: critical
    file: src/influx.rs
    lines: "36, 46-55"
    impact: "Complete daemon hang — all polling stops while one HTTP request blocks forever"
    fix: "Configure reqwest::Client with connect_timeout and request timeout"
  - id: CRIT-02
    title: "Serial port failure is unrecoverable — daemon must be restarted"
    severity: critical
    file: src/poller.rs
    lines: "30-36"
    impact: "USB adapter unplug/kernel driver reload permanently breaks polling until systemd restarts the process"
    fix: "Add reconnect logic to ModbusPoller when transport errors indicate broken pipe"
  - id: CRIT-03
    title: "API token committed to git in config.toml"
    severity: critical
    file: config.toml
    lines: "28"
    impact: "Secret exposure in version control"
    fix: "Add config.toml to .gitignore, provide config.toml.example instead"
  - id: HIGH-01
    title: "No request timeout on InfluxDB response body read"
    severity: high
    file: src/influx.rs
    lines: "61"
    impact: "response.text().await on error path can hang if server sends partial response"
    fix: "Use response.text().await with a tokio::time::timeout wrapper or configure client-level timeout"
  - id: HIGH-02
    title: "Device name not sanitized for InfluxDB line protocol injection"
    severity: high
    file: src/influx.rs
    lines: "12-25"
    impact: "Malicious or accidental spaces/commas/newlines in device name corrupt line protocol"
    fix: "Validate device names at config load (alphanumeric + underscore only)"
  - id: HIGH-03
    title: "Database name not URL-encoded in query parameter"
    severity: high
    file: src/influx.rs
    lines: "48"
    impact: "Database names with special characters break the URL"
    fix: "URL-encode the database parameter or validate at config time"
  - id: HIGH-04
    title: "Modbus context may enter broken state after transport error"
    severity: high
    file: src/poller.rs
    lines: "20-36"
    impact: "After a timeout or framing error, the RTU context's internal buffers may contain partial frames, corrupting subsequent reads"
    fix: "Consider reconnecting the Modbus context after transport-level errors, or add a flush/drain"
---

# Daemon Reliability Verification Report

**Scope:** Full codebase review — rs485-logger long-running daemon
**Verified:** 2026-04-02T17:34:50Z
**Status:** GAPS FOUND — 3 critical, 4 high, 5 medium, 2 low
**Verifier:** Code audit (manual + automated pattern analysis)

## Executive Summary

The rs485-logger codebase is well-structured and demonstrates solid Rust practices overall. The error handling strategy (skip-and-log per device) is sound, the `tokio::select!` shutdown is correctly pinned, and the `biased` select ordering prevents race conditions between reset and poll. However, there are **3 critical issues** that threaten the daemon's ability to run indefinitely without human intervention, plus several high/medium issues that could cause data integrity problems.

**Most dangerous finding:** The InfluxDB HTTP client has NO timeout configured. A single hung InfluxDB server will freeze the entire daemon forever — no more polls, no more resets, no shutdown response.

---

## Critical Findings (Will Cause Daemon Failure)

### CRIT-01: InfluxDB HTTP Requests Have No Timeout 🔴

**File:** `src/influx.rs:36, 46-55`
**Severity:** CRITICAL — daemon hangs indefinitely

The `reqwest::Client` is constructed with `Client::new()` which has **no connect timeout and no request timeout by default**:

```rust
// influx.rs:36
let client = reqwest::Client::new();  // ← No timeout configured!
```

The `write()` method calls `.send().await` with no timeout wrapper:

```rust
// influx.rs:49-54
let response = self.client
    .post(&url)
    .bearer_auth(&self.token)
    .body(body)
    .send()
    .await  // ← Waits FOREVER if InfluxDB doesn't respond
```

**Impact scenario:** InfluxDB becomes unresponsive (network partition, server overload, firewall drops packets silently). The `.send().await` never resolves. Because the daemon is single-threaded (`current_thread`), **all polling stops**. The select loop is stuck inside the tick arm. The shutdown signal handler also stops working because `select!` never reaches it — the daemon becomes unresponsive to SIGTERM and must be `kill -9`'d.

**How long until this happens?** Days to weeks of continuous operation. Network blips are common on Raspberry Pis with WiFi or shared switches.

**Recommended fix:**
```rust
let client = reqwest::Client::builder()
    .connect_timeout(std::time::Duration::from_secs(5))
    .timeout(std::time::Duration::from_secs(10))
    .build()
    .expect("Failed to build HTTP client");
```

Or wrap the entire `write()` call in `main.rs` with a `tokio::time::timeout()` like the Modbus calls already do.

---

### CRIT-02: Serial Port Failure Is Unrecoverable 🔴

**File:** `src/poller.rs:30-36`
**Severity:** CRITICAL — requires external restart

The serial port is opened once at startup and never reopened:

```rust
// poller.rs:30-36
pub fn new(serial: &SerialConfig) -> anyhow::Result<Self> {
    let port = SerialStream::open(&builder)?;
    let ctx = rtu::attach(port);
    Ok(ModbusPoller { ctx })
}
```

If the USB adapter is unplugged and re-plugged (or the kernel driver reloads), the file descriptor becomes invalid. Subsequent `read_input_registers` calls will return `Err` forever — but the daemon keeps running, logging `"Device poll failed, skipping"` for every device on every tick, producing an infinite stream of warnings with zero useful data.

**Impact scenario:** USB adapter power glitch on the RS485 bus (common with cheap adapters). The daemon continues "running" but is effectively dead. It won't recover until systemd restarts it (which only happens on crash, not on continuous error).

**Partial mitigation already exists:** systemd `Restart=always` will restart the daemon if it crashes. But the daemon doesn't crash — it runs happily logging errors forever.

**Recommended fix (option A — simplest):** Count consecutive poll failures across ALL devices. If all devices fail N consecutive polls (e.g. 10), log an error and exit with a non-zero code. systemd will restart the process, re-opening the serial port.

```rust
// In main loop
let mut consecutive_all_fail = 0u32;
// ... in tick arm:
let mut any_ok = false;
for device in &cfg.devices {
    if poller.poll_device(device).await.is_ok() { any_ok = true; }
}
if any_ok { consecutive_all_fail = 0; } else { consecutive_all_fail += 1; }
if consecutive_all_fail >= 10 {
    tracing::error!("All devices failed {consecutive_all_fail} consecutive polls — exiting for restart");
    break;
}
```

**Recommended fix (option B — more robust):** Add a `reconnect()` method to `ModbusPoller` that re-opens the serial port and re-attaches the RTU context.

---

### CRIT-03: API Token Committed to Git 🔴

**File:** `config.toml:28`
**Severity:** CRITICAL — secret exposure

```toml
token = "apiv3_AwBnUa4uyFJw_b5QZl6cZG1X7XriXLxaSelwPoTt8loiGzHXo256oB1ZCHRjkMML5Ajnv_cnO56flhBWdpH10w"
```

The InfluxDB API token is committed in plain text in `config.toml`. The `.gitignore` does not exclude `config.toml`.

**Recommended fix:**
1. Revoke the exposed token in InfluxDB
2. Add `config.toml` to `.gitignore`
3. Provide `config.toml.example` with a placeholder token
4. Remove the token from git history with `git filter-branch` or `bfg`

---

## High Findings (Likely to Cause Issues in Production)

### HIGH-01: No Timeout on InfluxDB Error Response Body Read

**File:** `src/influx.rs:61`

```rust
let body = response.text().await.unwrap_or_default();
```

When InfluxDB returns a non-204 status, the daemon reads the full response body. If the server sends an enormous error response (or a slow-drip response), this `.text().await` has no size limit and no timeout. While less likely than CRIT-01, it's the same class of hang.

**Recommended fix:** Use `response.text().await` with a timeout, or limit the body size with `response.bytes()` and truncate.

---

### HIGH-02: Device Name Not Sanitized for Line Protocol

**File:** `src/influx.rs:12-25`, `src/config.rs:53-78`

Device names from config are used verbatim as InfluxDB measurement names:

```rust
format!("{} voltage={:.4},...", reading.device_name, ...)
```

InfluxDB line protocol uses spaces as delimiters between measurement, fields, and timestamp. If a device name contains a space (e.g., `"floor 1"`), commas, or newlines, the line protocol is malformed.

The `validate_config()` function checks addresses but **does not validate device names**. There's no check for illegal characters.

**Recommended fix:** Add to `validate_config()`:
```rust
for device in &cfg.devices {
    anyhow::ensure!(
        device.name.chars().all(|c| c.is_alphanumeric() || c == '_'),
        "device name '{}' contains invalid characters (use only alphanumeric and underscore)",
        device.name
    );
}
```

---

### HIGH-03: Database Name Not URL-Encoded

**File:** `src/influx.rs:48`

```rust
let url = format!("{}?db={}&precision=ns", self.url, self.database);
```

The database name is inserted directly into the URL without URL-encoding. A database name containing `&`, `=`, `#`, spaces, or other special characters would break the URL or inject extra parameters.

**Recommended fix:** Use `url::form_urlencoded` or `percent_encoding` to encode the database parameter, or validate at config time that the database name is safe.

---

### HIGH-04: Modbus Context May Enter Broken State After Transport Error

**File:** `src/poller.rs:20-36`

The `tokio-modbus` `Context` wraps a tokio-serial `SerialStream`. After a timeout (the `tokio::time::timeout` fires and cancels the `read_input_registers` future), the Modbus context's internal buffer may contain a partial response frame from the device that responded late.

On the next `poll_device` call, this stale data may be interpreted as the start of a new response, causing a CRC error or misaligned register values.

**How `tokio-modbus` handles this:** The library does have some internal framing, but cancellation of a pending read (via timeout drop) is an edge case that may not flush the receive buffer cleanly.

**Impact:** After a timeout, the next poll for a different device may fail with a transport error, or worse, return corrupted register values.

**Recommended fix (defensive):** After a timeout, add a short delay (50-100ms) before the next device poll to let any late response drain. Or, consider reading and discarding any pending bytes on the serial port after a timeout.

**Note:** This may already be handled correctly by `tokio-modbus` internals. This finding requires hardware testing to confirm severity. Marking as HIGH due to potential for silent data corruption.

---

## Medium Findings (Should Fix for Production Reliability)

### MED-01: Log File Grows Unbounded

**File:** `src/main.rs:102`

```rust
let file_appender = tracing_appender::rolling::never(dir, filename);
```

The file appender uses `rolling::never` — the log file grows without rotation. On a Raspberry Pi with limited SD card space (often 8-16GB), months of continuous operation will fill the disk.

**Impact:** Full disk → potential OS instability, journal corruption, inability to write data.

**Recommended fix:** Use `rolling::daily` instead:
```rust
let file_appender = tracing_appender::rolling::daily(dir, filename);
```

Or configure external `logrotate` (but since the daemon holds the file handle open, `rolling::daily` is more reliable with non-blocking writers).

---

### MED-02: `far_future()` Duration Overflow on 32-bit Platforms

**File:** `src/main.rs:43`

```rust
fn far_future() -> tokio::time::Instant {
    tokio::time::Instant::now() + std::time::Duration::from_secs(365 * 24 * 3600 * 100)
}
```

`365 * 24 * 3600 * 100 = 3_153_600_000` seconds. This fits in `u64` but is near `u32::MAX` (4,294,967,295). On 32-bit ARM (armv7), this should still be fine since `Duration::from_secs` takes `u64`, and `tokio::time::Instant` internally uses a `u64` counter. However, the tokio time wheel has a maximum duration it can handle efficiently.

**Actual risk:** Low — tokio's time wheel can handle this. But a simpler approach:
```rust
fn far_future() -> tokio::time::Instant {
    tokio::time::Instant::now() + std::time::Duration::from_secs(86400 * 365 * 10) // 10 years
}
```

---

### MED-03: Timestamp Uses i64 but Could Overflow in 2038+ on 32-bit

**File:** `src/types.rs:45-48`

```rust
let timestamp_secs = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap_or_default()
    .as_secs() as i64;
```

The `.as_secs()` returns `u64`, cast to `i64`. This is fine for the next ~292 billion years. However, the nanosecond multiplication in `influx.rs`:

```rust
let ts_ns = reading.timestamp_secs * 1_000_000_000_i64;
```

At the current epoch (~1.7 billion seconds), `1_700_000_000 * 1_000_000_000 = 1.7e18`, which is within `i64::MAX` (9.2e18). Safe for the next ~270 years. This is a non-issue in practice but worth documenting.

**Status:** Acceptable. No fix needed.

---

### MED-04: No Backoff on Repeated InfluxDB Failures

**File:** `src/main.rs:260-265`

When InfluxDB is unreachable, the daemon logs a warning every `poll_interval_secs` for every device. With 2 devices at 10-second intervals, that's 12 warnings per minute, 720 per hour, 17,280 per day of useless identical log lines.

**Impact:** Log noise, disk fill (especially with file logging), difficult to find real errors in logs.

**Recommended fix:** Implement exponential backoff or "log first occurrence, suppress repeats" pattern:
```rust
// Simple approach: track InfluxDB health state
static INFLUX_HEALTHY: AtomicBool = AtomicBool::new(true);
// On failure: if was_healthy { warn!(...) } 
// On success: if !was_healthy { info!("InfluxDB connection restored") }
```

---

### MED-05: No Validation of `energy_reset.time` and `energy_reset.timezone` at Startup

**File:** `src/config.rs`, `src/main.rs:181-195`

The `timezone` and `time` fields are validated lazily when `next_reset_instant()` is called. If the config has an invalid timezone like `"NotATimezone"`, the daemon starts successfully, logs a warning, and silently disables energy reset.

This could surprise operators who expect reset to work. The validation should happen at config load time so invalid timezone/time causes an immediate fatal error.

**Recommended fix:** Add to `validate_config()`:
```rust
if let Some(ref er) = cfg.energy_reset {
    if er.enabled {
        er.timezone.parse::<chrono_tz::Tz>()
            .map_err(|_| anyhow::anyhow!("Unknown timezone: {}", er.timezone))?;
        chrono::NaiveTime::parse_from_str(&er.time, "%H:%M")
            .with_context(|| format!("Invalid time '{}', expected HH:MM", er.time))?;
    }
}
```

---

## Low Findings (Minor Improvements)

### LOW-01: `tokio::main` Missing `rt` Feature Nuance

**File:** `Cargo.toml:11`

```toml
tokio = { version = "1.50.0", features = ["rt", "time", "signal", "io-util", "macros"] }
```

The `#[tokio::main(flavor = "current_thread")]` attribute requires `rt`. This is present. However, `reqwest` internally requires a tokio runtime with `net` support. With `default-features = false` on reqwest and the listed tokio features, this works because reqwest pulls in its own tokio dependency.

**Status:** Works correctly. No fix needed.

---

### LOW-02: `unwrap_or_default()` on `SystemTime::now()` Duration

**File:** `src/types.rs:47`

```rust
.unwrap_or_default()
```

If `SystemTime::now()` is before `UNIX_EPOCH` (clock misconfigured), the duration calculation fails and defaults to `Duration::ZERO`, producing `timestamp_secs = 0`. This means InfluxDB would receive a timestamp of 0 nanoseconds (January 1, 1970).

**Impact on Raspberry Pi:** Pi boards without an RTC often start with a clock of January 1, 1970 until NTP syncs. If the daemon starts before NTP, early readings will have epoch-0 timestamps.

**Recommended fix:** Log a warning if timestamp is before a reasonable minimum (e.g. 2024-01-01):
```rust
if timestamp_secs < 1704067200 {
    tracing::warn!("System clock appears incorrect (timestamp={timestamp_secs}), data may have wrong timestamps");
}
```

---

## Verification of Originally Stated Concerns

### 1. Memory Leaks or Unbounded Growth Over Time ✅ GOOD

**Finding:** No unbounded collections, no growing buffers, no leaked allocations detected.

- The main loop allocates `PowerReading` (stack/heap) per poll and drops it after each write
- `reqwest::Client` reuses its connection pool (bounded by default)
- No `Vec::push` in a loop without clearing
- No `Arc` or reference-counted cycles
- `tracing-appender` uses a bounded channel for non-blocking writes

**Minor concern:** The `reqwest::Client` connection pool could grow if InfluxDB reconnects frequently, but reqwest limits this by default. Not a practical issue.

**Verdict:** ✅ Memory-safe for indefinite operation.

---

### 2. Deadlocks, Hangs, or Blocking Operations in Async Context ❌ CRITICAL

**Finding:** CRIT-01 — the InfluxDB HTTP call has no timeout and will hang the single-threaded runtime.

- ✅ No `std::thread::sleep` (would block the runtime)
- ✅ No `Mutex` or `RwLock` (no deadlock possible)
- ✅ No blocking file I/O in the async path (config is loaded before the runtime loop, tracing uses non-blocking appender)
- ❌ `reqwest` `.send().await` and `.text().await` have no timeout

**Verdict:** ❌ CRIT-01 must be fixed.

---

### 3. Resource Exhaustion (File Descriptors, Connections, Buffers) ⚠️ MEDIUM

**Finding:** Generally good, with one concern.

- ✅ One serial port FD, opened once
- ✅ One `reqwest::Client` with connection pooling
- ⚠️ MED-01: Log file grows unbounded
- ✅ No file handles leaked in loops
- ✅ No unbounded buffers

**Verdict:** ⚠️ Fix MED-01 (log rotation) for production.

---

### 4. Crash Scenarios (Panics, unwrap on None/Err in Runtime Paths) ✅ GOOD

**Finding:** Runtime paths are panic-free.

Audit of all `unwrap()`/`expect()` in non-test code:
- `main.rs:20,26` — `expect()` on signal handler installation — **acceptable**: these are one-time, startup-only, and process-fatal if they fail
- `main.rs:182,217` — `unwrap()` on `cfg.energy_reset.as_ref()` — **safe**: guarded by `reset_enabled` which is derived from the same `Option`
- `types.rs:47` — `unwrap_or_default()` — **safe**: gracefully defaults to zero
- `main.rs:293` — `unwrap_or_default()` — **safe**: defaults to zero duration
- `main.rs:294` — `.parse().unwrap_or(chrono_tz::Asia::Bangkok)` — **safe**: falls back to Bangkok timezone

All `unwrap()` calls in test modules (`#[cfg(test)]`) are acceptable.

**Verdict:** ✅ No runtime panics found. Good defensive coding.

---

### 5. Error Handling Gaps That Could Cause Silent Failures ⚠️ MEDIUM

**Findings:**
- ✅ Device poll errors are logged and skipped — good
- ✅ InfluxDB write errors are logged and skipped — good
- ⚠️ MED-04: No backoff on repeated InfluxDB failures (noisy logs, same message forever)
- ⚠️ MED-05: Invalid timezone/time silently disables energy reset instead of failing at startup
- ⚠️ CRIT-02: Serial port death silently degrades to "all polls fail forever"

**Verdict:** ⚠️ Several silent failure modes need attention.

---

### 6. Concurrency Issues ✅ EXCELLENT

**Finding:** No concurrency issues possible.

- Single-threaded runtime (`current_thread`) — no thread races
- No `Arc`, `Mutex`, `RwLock`, channels, or shared state
- Sequential device polling matches RS485 physical constraint
- `&mut poller` enforces exclusive access at compile time
- `biased` select prevents timer/reset race condition

**Verdict:** ✅ Excellent design. The architecture naturally avoids all concurrency bugs.

---

### 7. Signal Handling Correctness ✅ GOOD

**Finding:** Signal handling is correctly implemented.

- ✅ `shutdown_signal()` is called once and pinned outside the loop (avoids re-registering handlers)
- ✅ Both SIGTERM and SIGINT (Ctrl+C) are handled
- ✅ `#[cfg(unix)]` guard for SIGTERM (portability)
- ✅ `biased` select means shutdown can preempt other arms

**One edge case:** If the daemon is stuck in CRIT-01 (hung InfluxDB call), the shutdown signal handler never gets a chance to run because `select!` is not reached. This is a consequence of CRIT-01, not a signal handling bug.

**Verdict:** ✅ Correct, contingent on CRIT-01 fix.

---

### 8. Graceful Degradation When Devices Go Offline/Online ✅ GOOD

**Finding:** Device failure handling is robust.

- ✅ Each device is polled independently — one device timeout doesn't block others
- ✅ 500ms timeout per device (configurable would be better, but fixed is acceptable)
- ✅ Failed devices are logged and skipped, not retried (which would delay the next tick)
- ✅ Devices that come back online are automatically picked up on the next tick
- ⚠️ HIGH-04: Timeout cancellation may leave stale data in the Modbus context buffer

**Verdict:** ✅ Good graceful degradation, with HIGH-04 as a hardware-dependent concern.

---

### 9. InfluxDB Connection Resilience ❌ CRITICAL/MEDIUM

**Finding:** Partially resilient.

- ✅ InfluxDB write failures don't crash the daemon
- ✅ `reqwest::Client` reuses connections (HTTP keep-alive)
- ❌ CRIT-01: No timeout — hung InfluxDB hangs the daemon
- ⚠️ MED-04: No backoff on repeated failures
- ✅ Transient failures (network blips) recover automatically on next tick

**Verdict:** ❌ Fix CRIT-01, MED-04.

---

### 10. Serial Port Recovery After Errors ❌ HIGH

**Finding:** No recovery mechanism.

- ✅ Individual device errors are handled (timeout, exception)
- ❌ CRIT-02: Serial port loss is unrecoverable
- ❌ HIGH-04: Transport errors may corrupt the Modbus context state
- The serial port is opened once and never closed/reopened

**Verdict:** ❌ Fix CRIT-02, HIGH-04.

---

## Summary Table

| ID | Severity | Title | Effort |
|---|---|---|---|
| CRIT-01 | 🔴 Critical | InfluxDB HTTP no timeout — daemon hangs | 5 min |
| CRIT-02 | 🔴 Critical | Serial port failure unrecoverable | 30 min |
| CRIT-03 | 🔴 Critical | API token in git | 5 min |
| HIGH-01 | 🟠 High | No timeout on error response body read | 5 min |
| HIGH-02 | 🟠 High | Device name not sanitized for line protocol | 10 min |
| HIGH-03 | 🟠 High | Database name not URL-encoded | 5 min |
| HIGH-04 | 🟠 High | Modbus context stale data after timeout | 15 min |
| MED-01 | 🟡 Medium | Log file grows unbounded | 5 min |
| MED-02 | 🟡 Medium | far_future() duration unnecessarily large | 2 min |
| MED-03 | 🟡 Medium | Timestamp i64 nanosecond multiplication | N/A |
| MED-04 | 🟡 Medium | No backoff on repeated InfluxDB failures | 20 min |
| MED-05 | 🟡 Medium | Energy reset config validated lazily | 10 min |
| LOW-01 | 🟢 Low | Tokio features adequate | N/A |
| LOW-02 | 🟢 Low | Epoch-0 timestamp if clock wrong at boot | 10 min |

## Priority Fix Order

1. **CRIT-01** (5 minutes) — Add timeout to reqwest client builder. Highest impact-to-effort ratio.
2. **CRIT-03** (5 minutes) — Remove token from git, add `.gitignore` entry.
3. **HIGH-02** (10 minutes) — Validate device names at config load.
4. **CRIT-02** (30 minutes) — Add "exit after N consecutive all-device failures" logic.
5. **MED-01** (5 minutes) — Switch to `rolling::daily`.
6. **MED-05** (10 minutes) — Validate timezone/time at config load.
7. **HIGH-04** (15 minutes) — Add inter-device delay after timeout.
8. **MED-04** (20 minutes) — Add InfluxDB failure state tracking.

## What's Done Well

Despite the findings above, this codebase gets many things right:

1. **Single-threaded async** — perfectly matches RS485 physical constraints, eliminates all concurrency bugs
2. **Error propagation** — consistent use of `anyhow::Context` with descriptive messages
3. **Skip-and-log pattern** — individual device failures don't cascade
4. **`biased` select** — prevents reset/poll race condition
5. **Pinned shutdown future** — avoids re-registering signal handlers
6. **`MissedTickBehavior::Skip`** — prevents poll floods after delays
7. **No `panic!` in runtime paths** — all `unwrap()` calls are either safe or in test code
8. **systemd hardening** — `ProtectSystem=strict`, `NoNewPrivileges`, dedicated user
9. **Clear code structure** — 6 files, each with a single responsibility
10. **Comprehensive tests** — config validation, register decoding, line protocol formatting

---

_Verified: 2026-04-02T17:34:50Z_
_Verifier: Code audit (manual + automated pattern analysis)_
