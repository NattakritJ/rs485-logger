# Phase 6: Energy Reset at 00:00 Thailand Time - Context

**Gathered:** 2026-04-02
**Status:** Ready for planning

<domain>
## Phase Boundary

Send a Modbus `0x42` (Reset Energy) command to each configured PZEM-016 device once per
day at midnight Bangkok time (Asia/Bangkok, UTC+7). The reset clears the accumulated energy
counter (kWh) on every device so each day starts from zero.

This phase does NOT include: changing the poll loop data reads, adding new InfluxDB
measurements, or any reporting of the reset event to InfluxDB. It is a Modbus control
operation only.

</domain>

<decisions>
## Implementation Decisions

### Reset Scope

- **D-01:** Reset every configured device **individually** (per-device), not via broadcast
  address 0x00. Each device is addressed by its `DeviceConfig.address` using the existing
  device list in `AppConfig.devices`.
- **D-02:** For each device, **wait for and validate the echo reply** from the device. A
  correct reply mirrors the command (`slave_addr + 0x42 + CRC`). An error reply uses FC
  `0xC2` with an abnormal code — log `WARN` per device and continue (same skip-and-log
  pattern as the poll loop).

### Sending the 0x42 Command

- **D-03:** Use `tokio-modbus` `call()` method to send the raw PDU for function code
  `0x42`. The RTU framer handles CRC automatically. No need to bypass `tokio-modbus` or
  write raw bytes to the serial stream.
- **D-04:** The PDU for reset energy has no data bytes — it is just the function code
  `0x42` with no payload. The full RTU frame (slave address + PDU + CRC) is assembled by
  `tokio-modbus`.
- **D-05:** Validate the response FC byte: expect `0x42` (success echo). If `0xC2` is
  received, extract abnormal code and log a `WARN`. Use a 500 ms timeout (consistent with
  the existing poll timeout in `poller.rs`).

### Scheduling

- **D-06:** Use the `chrono` and `chrono-tz` crates. Compute "next midnight" in
  `Asia/Bangkok` timezone using `chrono_tz::Asia::Bangkok`. Convert the `DateTime<Tz>`
  to a `tokio::time::Instant` and use `tokio::time::sleep_until` to wait.
- **D-07:** On startup, compute the **next future midnight** from the current wall-clock
  time. If the daemon starts at 01:00, it schedules for **tomorrow's** 00:00 — it does
  not attempt to catch up on the already-passed midnight. This is intentional.
- **D-08:** After each midnight reset fires, **recompute** the next midnight from the
  current time (do not rely on `sleep_until + 86400s` drift). This keeps the schedule
  accurate across long uptime.
- **D-09:** The reset runs **inside the existing `tokio::select!` loop** in `main.rs`.
  A second future (`reset_sleep`) sits alongside `ticker.tick()` and `shutdown`. When
  `reset_sleep` completes, run the per-device reset sequence, then recompute and reset
  the sleep timer. No separate spawned task.

### Config

- **D-10:** Add an optional `[energy_reset]` section to `AppConfig`. Fields:
  - `enabled: bool` — controls whether daily reset is active
  - `timezone: String` — IANA timezone name (default `"Asia/Bangkok"`)
  - `time: String` — time-of-day in `"HH:MM"` format (default `"00:00"`)
  
  All three fields are optional at the TOML level using `Option<EnergyResetConfig>`.
  If the section is absent, energy reset is disabled. Example:
  ```toml
  [energy_reset]
  enabled = true
  timezone = "Asia/Bangkok"
  time = "00:00"
  ```

### Observability

- **D-11:** Log `tracing::info!` at the start of the daily reset batch (e.g. "Daily
  energy reset starting") and per-device on success (e.g. device name + "energy reset
  OK").
- **D-12:** Log `tracing::warn!` per device on reset failure (mirrors poll loop
  convention). Include device name and error detail. Continue to next device.
- **D-13:** Log `tracing::info!` on startup showing next reset scheduled time (e.g.
  "Next energy reset scheduled at 2026-04-03T00:00:00+07:00").

### Agent's Discretion

- Exact struct/function naming for the `0x42` command logic — e.g. `reset_energy()` on
  `ModbusPoller`, or a standalone function. Researcher/planner should decide based on
  how it fits alongside `poll_device()`.
- Whether `EnergyResetConfig` lives in `config.rs` or a new `reset.rs` — small daemon,
  flat layout preferred (Phase 1 D-05 pattern).
- Error message wording — use clear operator-friendly phrasing consistent with the rest
  of the codebase.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### PZEM-016 Protocol (critical for 0x42 command)
- `ct_datasheet.txt` — **Primary source** for the Reset Energy command (§2.5):
  frame format, correct reply (mirror echo), error reply (0xC2 + abnormal code), and CRC
  scheme. All implementation decisions about the 0x42 frame are derived from this file.
  Also confirms FC 0x04 register map (§2.3) and communication parameters (§2.1).

### Project Foundation
- `.planning/PROJECT.md` — Project vision, constraints, key decisions, hardware context
- `.planning/REQUIREMENTS.md` — v1 requirements (TBD for Phase 6 — no REQ IDs assigned yet)
- `.planning/ROADMAP.md` — Phase 6 goal and dependency on Phase 5

### Stack
- `.planning/research/STACK.md` — Full stack rationale; all dependency versions locked here

### Prior Context
- `.planning/phases/01-foundation/01-CONTEXT.md` — D-05 flat module layout pattern,
  D-06 anyhow error strategy, D-07 f64 fields

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/poller.rs` — `ModbusPoller` struct with `ctx: client::Context`. The `0x42` reset
  command can be added as a new method `reset_energy()` on this struct, reusing `set_slave()`
  and the existing `ctx.call()` pattern. The 500 ms timeout from `poll_device()` is reused.
- `src/main.rs` — `tokio::select!` loop with `ticker.tick()` and `shutdown` arms. Phase 6
  adds a third arm for the daily reset sleep future alongside the existing two.
- `src/config.rs` — `AppConfig` struct. Add optional `energy_reset: Option<EnergyResetConfig>`
  field. Follows the `log_file: Option<String>` / `log_level: Option<String>` pattern
  (already optional fields).

### Established Patterns
- Skip-and-log: `tracing::warn!` per device on error, continue loop — used in poll loop
  (`main.rs:148-153`). Energy reset failures follow the same pattern.
- 500 ms timeout with `tokio::time::timeout()` wrapping Modbus calls — `poller.rs:56-63`.
  Reuse for the `call()` timeout on the 0x42 command.
- `tokio::pin!(shutdown)` outside the loop in `main.rs:127-128` — the reset sleep future
  should similarly be pinned or handled via `Box::pin`.

### Integration Points
- New dependency: `chrono` + `chrono-tz` for timezone-aware next-midnight computation.
  These must be added to `Cargo.toml`. `chrono-tz` bundles the IANA tz database at compile
  time — no runtime tz files needed on the Pi.
- `ModbusPoller.reset_energy(&device)` → called in `main.rs` reset arm, same as
  `poller.poll_device(&device)` is called in the ticker arm.

</code_context>

<specifics>
## Specific Ideas

- On startup: compute `next_midnight_bangkok(now)`, `tokio::time::sleep_until(next_midnight)`
  as the initial reset timer. After reset fires: recompute `next_midnight_bangkok(now)`,
  reset the timer. The function `next_midnight_bangkok(now: DateTime<Utc>) -> Instant`
  is a pure function — easy to unit test.
- `call()` in tokio-modbus takes a `Request::Custom(fc, data)` — for 0x42, `fc = 0x42`
  and `data = Bytes::new()` (empty). The response `Response::Custom(fc, data)` should
  have `fc == 0x42` for success.
- `chrono_tz::Asia::Bangkok` is the IANA timezone constant. `NaiveDate::succ_opt()` +
  `and_hms_opt(0, 0, 0)` + `.and_local_timezone(Bangkok)` gives the next midnight DateTime.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---

*Phase: 06-send-command-to-reset-energy-at-the-beginning-of-the-day*
*Context gathered: 2026-04-02*
