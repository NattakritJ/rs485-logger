# Phase 2: Revisit Polling Slowlyness - Context

**Gathered:** 2026-04-09
**Status:** Ready for planning

<domain>
## Phase Boundary

Fix the true root cause of the ~5s poll cycle: sequential InfluxDB HTTP writes blocking the Modbus poll loop. Each write waits ~630ms for HTTP response before the next device is polled — 5 devices × ~990ms (Modbus + InfluxDB + IFD) ≈ 5000ms per cycle.

The fix is to decouple InfluxDB writes from the poll loop via `tokio::spawn` (fire-and-forget). After a successful Modbus read, the reading is handed off to a spawned task and the poll loop immediately moves to the next device.

Also fold in two small but blocking active requirements: CFG-02 (parity field) and RISK-1 (udev rule driver mismatch).

This phase is software-only with one deploy artifact change (udev rule). No hardware changes. Sequential Modbus polling remains unchanged.

</domain>

<decisions>
## Implementation Decisions

### Write Decoupling Strategy
- **D-01:** Decouple InfluxDB writes from the poll loop using `tokio::spawn` (fire-and-forget). After `poll_device` returns `Ok(reading)`, immediately spawn a write task and continue to the next device — do NOT await the write.
- **D-02:** Write tasks are unbounded — no semaphore, no channel backlog cap. Pi has 4GB RAM with 288MB used (daemon-only). Even prolonged InfluxDB outages won't cause memory pressure from queued write tasks (each task holds ~6 f64 values + a name string).
- **D-03:** Out-of-order HTTP delivery is acceptable. Timestamps are already set at Modbus read time inside the `PowerReading` struct. InfluxDB receives correct timestamps regardless of HTTP arrival order.

### InfluxWriter Sharing
- **D-04:** Add `#[derive(Clone)]` to `InfluxWriter`. `reqwest::Client` is `Arc`-based internally — clone is `O(1)` (increments a reference count), not a deep copy. Each spawned write task receives a cloned `InfluxWriter` by value.
- **D-05:** The spawned write task owns: `writer_clone: InfluxWriter`, `reading: PowerReading`, `influx_healthy: Arc<AtomicBool>`.

### InfluxDB Health Tracking
- **D-06:** Replace the `influx_healthy: bool` local variable in `main()` with `Arc<AtomicBool>`. The spawned write tasks share this flag with the main loop.
- **D-07:** Health state transitions in the spawned task:
  - Healthy → unhealthy (first failure): log WARN, set flag to false.
  - Unhealthy → healthy (first success after failure): log INFO restore message, set flag to true.
  - While unhealthy: silently drop per-write errors (no repeated WARN spam).
- **D-08:** Use `Arc::clone` to pass the health flag into each spawned task. Atomic operations (`load`/`compare_exchange` or `swap`) are appropriate — no `Mutex` needed.

### CFG-02: Parity Field
- **D-09:** Add `parity: Option<String>` to `SerialConfig` in `src/config.rs`. Default: `"N"` (no parity — 8N1, which is what PZEM-016 uses). Accepted values: `"N"` (none), `"E"` (even), `"O"` (odd).
- **D-10:** Apply parity in `ModbusPoller::new` when building the `tokio_serial::new()` builder. Map string → `tokio_serial::Parity` enum. Invalid string → config validation error (fail-fast at startup).
- **D-11:** Backwards compatible: if `parity` is absent from config, default to `"N"` via `unwrap_or`. Existing configs continue to work without changes.

### RISK-1: Udev Rule Driver Mismatch
- **D-12:** Fix `deploy/99-rs485.rules`. The comment says "cp210x (most common)" but the rule uses `DRIVERS=="ch341"` — mismatched. The rule currently works only on CH341 adapters.
- **D-13:** Fix approach: remove the `DRIVERS=="..."` clause. Instead, instruct users to match by VID/PID (`ATTRS{idVendor}`, `ATTRS{idProduct}`) via the usage note already in the file comments. Update the rule to be driver-agnostic: `SUBSYSTEM=="tty", SUBSYSTEMS=="usb", SYMLINK+="ttyRS485", MODE="0660", GROUP="dialout"` with a prominent comment that VID/PID should be added for precision.
- **D-14:** Update README.md to align the udev rule documentation with the actual `deploy/99-rs485.rules` file.

### the agent's Discretion
- Whether to use `AtomicBool::compare_exchange` or `swap` for the health state transitions
- Exact module placement for the spawned write helper (inline closure vs extracted `async fn`)
- Whether `InfluxWriter` needs `Clone` derived or can be wrapped in `Arc<InfluxWriter>` (both are equivalent — prefer `derive(Clone)` for ergonomics)
- reqwest feature rename (`"rustls"` → `"rustls-tls"`) — minor Cargo.toml cleanup, can fold in or leave

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

No external specs for this phase — requirements are fully captured in decisions above.

### Existing Implementation
- `src/main.rs` — poll loop tick arm (line ~295), `influx_healthy` bool, `any_ok` tracking, `InfluxWriter::write` call
- `src/influx.rs` — `InfluxWriter` struct and `write` method (add `#[derive(Clone)]` here)
- `src/config.rs` — `SerialConfig` struct (add `parity: Option<String>` field), `validate_config` (add parity validation)
- `src/poller.rs` — `ModbusPoller::new` (apply parity from `SerialConfig`)
- `deploy/99-rs485.rules` — udev rule to fix (RISK-1)

### Hardware Reference
- PZEM-016 serial config: 9600 baud, 8N1 (parity = None/N)
- Typical Modbus read latency: 60–65ms (confirmed from production logs)
- Typical InfluxDB write latency: ~630ms on current setup (observed bottleneck)

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `InfluxWriter` (`src/influx.rs`) — add `#[derive(Clone)]`; reqwest::Client inside is Arc-based, clone is O(1)
- `ModbusPoller::new` (`src/poller.rs:39`) — already reads `serial.read_timeout_ms`; add parity read here
- `SerialConfig` (`src/config.rs:26`) — already uses `Option<u64>` pattern for `read_timeout_ms`; follow same pattern for `parity: Option<String>`
- `influx_healthy` bool (`src/main.rs:175`) — replace with `Arc<AtomicBool>`, clone into spawned tasks

### Established Patterns
- `Option<T>` with `unwrap_or(default)` — used for `read_timeout_ms`; follow for `parity`
- Skip-and-log on device error: `tracing::warn!` + continue — do NOT change; only InfluxDB write error handling changes
- `tracing::warn!` / `tracing::info!` for health state transitions — keep the existing log messages
- Config defaults via `Option<T>` with `unwrap_or` — same pattern for new fields

### Integration Points
- Poll loop tick arm (`src/main.rs:302–337`) — replace `writer.write(&reading).await` with `tokio::spawn(async move { ... })`
- `InfluxWriter::write` call is the only place where the health flag update currently happens — must move this logic into the spawned task
- `tokio_serial::new()` builder in `ModbusPoller::new` — add `.parity(...)` call after `new(port, baud_rate)`

</code_context>

<specifics>
## Specific Ideas

- The InfluxDB write latency (~630ms) is a network round-trip to 192.168.2.10:8181 — not inherently slow, just blocking the wrong thread. Fire-and-forget eliminates this entirely from the Modbus cycle path.
- Expected cycle time after fix: 5 devices × (60ms Modbus + 30ms IFD) = ~450ms per cycle — comfortably under 1s poll interval. The InfluxDB write still takes ~630ms but happens concurrently, not sequentially.
- The `arc_health_flag` approach preserves the Phase 7 MED-04 health suppression feature exactly — no regression in behavior, just different ownership model.

</specifics>

<deferred>
## Deferred Ideas

- reqwest feature rename (`"rustls"` → `"rustls-tls"` in Cargo.toml) — minor naming correctness fix; can be folded in as a trivial change or done as a quick task post-phase
- Channel-based write worker (bounded queue, ordered delivery) — not needed; fire-and-forget is sufficient and simpler
- Per-device write timeout config — not needed; current behavior (relying on reqwest's built-in timeout) is sufficient

</deferred>

---

*Phase: 02-revisit-polling-slowlyness*
*Context gathered: 2026-04-09*
