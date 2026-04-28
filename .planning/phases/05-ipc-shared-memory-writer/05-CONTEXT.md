# Phase 5: IPC Shared Memory Writer - Context

**Gathered:** 2026-04-28
**Status:** Ready for planning

<domain>
## Phase Boundary

Implement the writer side of the PZEM-to-Orchestrator shared memory interface. The PZEM daemon writes per-inverter power data to a 96-byte fixed-size binary region in `/dev/shm/pzem-power.bin` using a seqlock protocol, enabling the Solar Orchestrator to read power data with near-zero latency (<1μs) instead of querying InfluxDB (~1s).

InfluxDB writes continue unchanged — shared memory is an additional output path, not a replacement. The reader (orchestrator) is implemented in a separate repo.

</domain>

<decisions>
## Implementation Decisions

### Device-to-Inverter Mapping
- **D-01:** Add `shm_name: Option<String>` to `DeviceConfig` in `src/config.rs`. Only devices with a `shm_name` value are written to the shared memory file.
- **D-02:** The `shm_name` value is written verbatim into the 16-byte `name` field in the binary layout (null-terminated, zero-padded). It must match the orchestrator's config keys exactly (e.g., `"east"`, `"west"`, `"west2"`).
- **D-03:** Devices without `shm_name` are ignored for shared memory — they continue to write to InfluxDB as usual. This allows the daemon to poll 5+ devices but only expose the 3 inverters to the orchestrator.

### Config Schema
- **D-04:** Add an optional `[shm]` section to `AppConfig`, following the `energy_reset` pattern:
  ```toml
  [shm]
  enabled = true
  path = "/dev/shm/pzem-power.bin"
  ```
- **D-05:** `ShmConfig` struct: `enabled: bool`, `path: String`. If the `[shm]` section is absent from config, shared memory is disabled.
- **D-06:** Validate in `validate_config`:
  - If `shm.enabled`, at least one device must have `shm_name` set (otherwise shm is useless).
  - No more than 3 devices may have `shm_name` set (contract defines 3 inverter slots).
  - `shm_name` values must be unique across devices.
  - `shm_name` must be ≤15 bytes (16-byte field minus null terminator).

### Write Integration Point
- **D-07:** Write to shared memory **synchronously** in the poll loop, after all devices have been polled in the current tick. The seqlock write is a memcpy (<1μs) — no need for async/fire-and-forget.
- **D-08:** New `src/shm.rs` module with `ShmWriter` struct. Responsibilities:
  - File creation/truncation on startup (`OpenOptions::create(true).write(true)`, `set_len(96)`)
  - Zero-initialization of the 96-byte region
  - Seqlock write protocol (increment sequence odd → write data → increment sequence even)
  - File permissions set to `0o644`
- **D-09:** `ShmWriter::new(path: &str) -> anyhow::Result<Self>` — creates file, maps memory via `memmap2`, zero-initializes.
- **D-10:** `ShmWriter::write(&mut self, readings: &[PowerReading]) -> anyhow::Result<()>` — accepts the subset of readings that have `shm_name`, performs the seqlock write. Only `power_watts` and `shm_name` are written per the contract layout.

### Seqlock Protocol
- **D-11:** Sequence counter at offset 0 as `AtomicU64`. Writer uses `Ordering::Release` for both increments. Starts at 0, first write makes it 1 (odd) then 2 (even).
- **D-12:** Data fields (timestamp_ns, inverter_count, readings) use regular memory writes between the two atomic sequence stores — the seqlock provides the memory barrier.

### Dependency
- **D-13:** Add `memmap2 = "0.9"` to `Cargo.toml` for memory-mapped file I/O.

### Lifecycle
- **D-14:** On daemon shutdown, do NOT delete the shm file — the contract specifies the file persists and the orchestrator detects staleness via timestamp.
- **D-15:** On daemon restart, reinitialize the file (truncate to 96 bytes, zero-fill, resume writing).
- **D-16:** Log at startup: `tracing::info!(path = shm_path, "Shared memory writer enabled")` if shm is enabled. If disabled, log only if `[shm]` section is present but `enabled = false` (same pattern as `energy_reset`).

### Agent's Discretion
- Whether `ShmWriter` holds the `MmapMut` directly or wraps it in a helper struct
- Exact error handling if mmap fails mid-operation (log + continue polling, or escalate)
- Whether to add a `write_shm` convenience method on the poll loop or call `shm_writer.write()` directly
- Whether the config example shows `shm_name` on all devices or just one as a hint

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Shared Memory Contract
- `.planning/phases/05-ipc-shared-memory-writer/15-PZEM-DAEMON-CONTRACT.md` — Full binary layout (96 bytes), seqlock protocol, write timing, graceful degradation scenarios, Rust implementation hints. **This is the primary requirements source.**

### Existing Implementation
- `src/config.rs` — `DeviceConfig` struct (add `shm_name: Option<String>`); `AppConfig` struct (add `shm: Option<ShmConfig>`); `validate_config` (add shm validation rules)
- `src/main.rs` — poll loop (lines ~243–400); startup initialization (lines ~60–180); fire-and-forget InfluxDB write pattern (reference, but shm is synchronous)
- `src/types.rs` — `PowerReading` struct (data source for shm writes — only `power` field and device name used)
- `config.toml` / `config.toml.example` — add `[shm]` section and `shm_name` examples to `[[devices]]`

### Prior Phase Context
- Phase 4 CONTEXT.md (`04-CONTEXT.md`) — `DeviceConfig` may gain `poll_interval_ms: Option<u64>` (D-01); `Option<T>` config pattern established
- Phase 3 CONTEXT.md (`03-CONTEXT.md`) — `Option<u64>` + `unwrap_or` config pattern; `validate_config` validation rules pattern

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `Option<T>` config pattern — used for `read_timeout_ms`, `parity`, `poll_interval_ms`; follow same for `shm_name` and `ShmConfig`
- `validate_config` — centralized validation function; add shm rules here
- `PowerReading.power` field (f64, watts) — the only data field needed for the shm binary layout
- `PowerReading.device_name` — maps to `DeviceConfig.name`; `shm_name` is the separate field for the binary name

### Established Patterns
- Optional config sections: `Option<SectionConfig>` on `AppConfig` (e.g., `energy_reset: Option<EnergyResetConfig>`)
- Startup log pattern: `tracing::info!(field = value, "message")`
- Module pattern: one struct per concern (`InfluxWriter` in `influx.rs`, `ModbusPoller` in `poller.rs`) — follow with `ShmWriter` in `shm.rs`
- `DeviceConfig` is `#[derive(Clone)]` — must remain Clone after adding `shm_name`

### Integration Points
- After the `for device in &cfg.devices` poll loop in `main.rs` — collect readings with `shm_name`, pass to `ShmWriter::write()`
- `mod shm;` declaration in `main.rs` alongside existing `mod influx;` and `mod poller;`
- `Cargo.toml` — add `memmap2 = "0.9"` dependency

</code_context>

<specifics>
## Specific Ideas

- The `shm_name` approach cleanly separates "what gets polled on Modbus" from "what gets exposed to the orchestrator" — a device can be polled for InfluxDB dashboards without appearing in shared memory.
- Config example in `config.toml.example`:
  ```toml
  [shm]
  enabled = true
  path = "/dev/shm/pzem-power.bin"

  [[devices]]
  address = 1
  name = "inverter_east"
  shm_name = "east"       # Written to shared memory for orchestrator

  [[devices]]
  address = 2
  name = "inverter_west"
  shm_name = "west"

  [[devices]]
  address = 3
  name = "grid_meter"
  # No shm_name — not exposed to orchestrator, InfluxDB only
  ```
- The seqlock write is trivially fast — no performance concern about doing it synchronously in the poll loop.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---

*Phase: 05-ipc-shared-memory-writer*
*Context gathered: 2026-04-28*
