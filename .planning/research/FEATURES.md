# Feature Research

**Domain:** Modbus RTU RS485 data logger daemon (embedded Linux / Raspberry Pi → InfluxDB)
**Researched:** 2026-04-02
**Confidence:** HIGH

---

## Feature Landscape

### Table Stakes (Users Expect These)

Features that a production-quality Modbus data logger _must_ have. Missing any of these means the tool is broken or unreliable.

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Configurable serial port path + baud + parity | The device path (`/dev/ttyUSB0`) changes between hosts; 9600/8N1 is PZEM-016 default but must be overridable | LOW | TOML `[serial]` section; standard `tokio-serial` / `serialport` crate params |
| TOML config with device list (address + name) | Operators define how many devices exist and name them; not hardcoded | LOW | Serde deserialize into `Vec<DeviceConfig>` |
| Poll all PZEM-016 registers (voltage, current, power, energy, frequency, power factor) via FC 0x04 | All 6 fields are the product's entire value proposition | MEDIUM | Single FC04 call per device reads all 10 registers at once (0x0000–0x0009) |
| Sequential polling across all devices per interval | RS485 is a shared bus — only one master transaction at a time; concurrent reads corrupt responses | LOW | `tokio::time::interval` tick → sequential `for device in devices` loop |
| Global polling interval (seconds) in TOML | Operators must be able to change poll cadence without recompiling | LOW | `interval_secs: u64` in TOML |
| Skip failed device, log error, continue poll cycle | A failed PZEM-016 (powered off, wrong address) must not stall the entire loop | LOW | `Result::err` path → `tracing::warn!` → continue; this is the stated core reliability requirement |
| Write to InfluxDB 3 via HTTP line protocol (`/api/v3/write_lp`) | InfluxDB 3 write endpoint; `Authorization: Bearer <token>` header | MEDIUM | `reqwest` async HTTP client; line format: `<device_name> voltage=229.6,current=0.10,...` |
| Per-device InfluxDB measurement name (= device name from config) | Allows per-device queries without tag filtering | LOW | Measurement name = device name string; one write call per device per poll cycle |
| Structured logging to stderr/stdout (INFO by default) | systemd captures stdout/stderr via `journald`; operators need to see what's happening | LOW | `tracing` + `tracing-subscriber` with `EnvFilter`; level from config or `RUST_LOG` |
| Optional log to file (configurable path) | Pi operators often want a persistent log file for post-mortem; journald may be large | LOW | `tracing-appender` rolling file or simple file writer; configurable path + level |
| Graceful shutdown on SIGTERM / SIGINT | systemd sends SIGTERM before killing; clean shutdown avoids corrupted line-protocol mid-write | LOW | `tokio::signal::unix::signal(SIGTERM)` + `select!`; complete current poll cycle, flush, exit |
| Systemd service unit file | Operators use `systemctl start/stop/enable`; auto-restart on crash is mandatory for 24/7 operation | LOW | `[Service] Type=simple Restart=always RestartSec=5`; included in repo |
| Config file validation at startup with clear error messages | Mistyped device address or bad URL should fail fast with a human-readable message, not a cryptic panic | LOW | `serde` deserialization errors surfaced as `anyhow`/`thiserror` with context |
| InfluxDB write failure: log error and continue polling | Network blip or InfluxDB restart must not crash the daemon or stall device polling | LOW | HTTP error → `tracing::error!` → continue; data from the failed write is discarded (no gap in future polls) |
| USB device path stability after reboot | `/dev/ttyUSB0` numbering is arbitrary; production setups must use stable paths | LOW | Document using `udev` rule to create `/dev/rs485-pzem` symlink; not a code feature, but a deployment requirement to document |

---

### Differentiators (Competitive Advantage)

Features not expected in minimal Modbus loggers but that make this daemon production-quality.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| In-memory write-failure buffer with bounded retry | InfluxDB briefly unavailable (restart, network) → buffer N failed batches in memory → retry on next successful cycle; prevents gaps in data | MEDIUM | Ring buffer (`VecDeque`) of `(timestamp, line_protocol_batch)` with configurable max size; flush oldest first on recovery. **Do not use disk** — adds complexity and SD card wear |
| InfluxDB connectivity check at startup | Fail fast with clear error if InfluxDB is unreachable at launch rather than silently losing data | LOW | One test write or `HEAD /ping` on startup before entering poll loop |
| Per-device consecutive error counter in logs | Distinguish "device flapped once" from "device has been offline for 50 cycles" without querying InfluxDB | LOW | `HashMap<DeviceId, u32>` error count; log escalation at thresholds (e.g., warn at 3, error at 10) |
| Configurable Modbus RTU timeout per-device-poll | PZEM-016 needs ~100ms for response; too-short timeout causes false errors, too-long stalls the bus | LOW | `timeout_ms: u64` in TOML `[serial]`; default 500ms; passed to `tokio_modbus` read call |
| Startup summary log line | On boot, emit one INFO log showing: serial port, baud rate, device count, InfluxDB URL, poll interval | LOW | Confirms config was parsed correctly at a glance via `journalctl` |
| Config file path as CLI argument | Allows running multiple instances (unusual but possible with multiple USB adapters) or non-default config location | LOW | `clap` with `--config <path>`; default `/etc/rs485-logger/config.toml` |
| Log rotation for file appender | On a Pi running 24/7 for months, unbounded log files fill the SD card | LOW | `tracing-appender` rolling daily; or document `logrotate` config |
| Explicit InfluxDB 3 database/table model in docs | InfluxDB 3 uses "database + table" not "org + bucket + measurement" as in v1/v2; this trips up users | LOW | In README and example config, explain the v3 API's `?db=` parameter mapping |

---

### Anti-Features (Commonly Requested, Often Problematic)

Features to deliberately _not_ build in v1.

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| Per-device polling intervals | "Device X is slow, Device Y needs faster data" | Turns a simple `for` loop into a scheduler; adds concurrency to a serial bus that is fundamentally single-threaded; all PZEM-016 respond in <200ms anyway | Single global interval is sufficient; a 5-second interval is fine for power monitoring |
| Disk-persistent write buffer (SQLite/CSV fallback) | "Don't lose data when InfluxDB is down for hours" | Adds significant complexity; SD card write amplification is a real failure mode on Pi; InfluxDB on the same LAN almost never goes down for >minutes | Bounded in-memory ring buffer handles brief outages; long outages mean accepting data gaps |
| Hot-reload of config file (SIGHUP) | "Change device list without restart" | Config affects serial port setup and device state; partial reload is risky; PZEM-016 setups rarely change | Just restart the daemon (`systemctl restart rs485-logger`); it starts in <1 second |
| Modbus TCP support | "I want to use this with a TCP gateway" | Different framing, different library code path, doubles test surface; out of stated scope | Fork or use a Modbus TCP tool; this is RTU-only |
| Web UI / REST API for status | "I want to see device status in a browser" | Grafana connected to InfluxDB already does this; adding a web server increases binary size and attack surface | Use InfluxDB + Grafana; expose device status via a periodic health measurement in InfluxDB itself |
| Alerting / threshold notifications | "Alert me when voltage drops below 200V" | Complex to do well (hysteresis, dedup, notification channels); InfluxDB 3 processing engine or Grafana alerting already provides this | Use InfluxDB 3's built-in threshold alerting or Grafana alerting rules |
| OAuth / env-var / secret-store credential sourcing | "Don't put the token in the config file" | Over-engineering for a single-device Pi setup; token in a file with `chmod 600` is fine | Document `chmod 600 /etc/rs485-logger/config.toml`; v2 can add env-var override |
| One-shot / cron mode | "Run on demand, not continuously" | Adds a second code path and startup/shutdown overhead; daemon with systemd timer would be redundant | Run as daemon; for ad-hoc reads, use `mbpoll` or `modpoll` CLI tool directly |
| Auto-discovery of PZEM-016 devices | "Scan the bus and find all devices automatically" | RS485 bus scan requires iterating all 247 addresses; takes minutes; PZEM-016 needs a connected load to respond; fragile on production systems | Explicit device list in config; provide docs on how to find/set PZEM addresses with `mbpoll` |

---

## Feature Dependencies

```
[Serial port config (path, baud, parity)]
    └──required by──> [Modbus RTU client initialization]
                          └──required by──> [Per-device polling (FC04 read)]
                                                └──required by──> [InfluxDB write]

[TOML config (device list, interval, InfluxDB URL/token/db)]
    └──required by──> [All runtime behavior]

[Per-device polling]
    └──required by──> [Skip-and-continue error handling]
    └──required by──> [Per-device error counter] (differentiator)

[InfluxDB write]
    └──required by──> [Write-failure in-memory buffer] (differentiator)
    └──required by──> [Startup connectivity check] (differentiator)

[Graceful shutdown (SIGTERM)]
    └──enhances──> [InfluxDB write] (flush in-flight batch before exit)

[Systemd service unit]
    └──enhances──> [Graceful shutdown] (systemd sends SIGTERM before SIGKILL)

[Structured logging to stdout/stderr]
    └──enhances──> [Systemd service unit] (journald captures stdout)

[Log to file] ──optional──> [Log rotation]
```

### Dependency Notes

- **Serial port config requires Modbus client:** `tokio-modbus` RTU client takes a `tokio-serial` port built from the config's path + baud settings.
- **TOML config required by all runtime behavior:** Everything derives from the parsed config; startup validation failure must prevent the poll loop from starting.
- **Graceful shutdown enhances InfluxDB write:** On SIGTERM, complete the current in-progress poll cycle and flush any buffered writes before exiting — avoids partial line-protocol payloads mid-HTTP-request.
- **Write-failure buffer requires InfluxDB write:** The buffer only makes sense if there is a write target; it holds `(timestamp, batch)` tuples for retry.
- **Log-to-file is independent of stdout logging:** Both can operate simultaneously; `tracing-subscriber` supports multiple layers.

---

## MVP Definition

### Launch With (v1)

Minimum viable product that fulfills the stated core value: *"Reliable, continuous power data from every PZEM-016 flowing into InfluxDB without data gaps."*

- [x] **TOML config** — device list (address + name), serial port, poll interval, InfluxDB endpoint + token + db — because nothing works without it
- [x] **Modbus RTU serial initialization** — open port at configured baud/parity — foundation for all reads
- [x] **FC04 register read for all 6 PZEM-016 fields** per device — the entire point of the daemon
- [x] **Sequential polling loop** — tick on interval, read each device in order — core collection behavior
- [x] **Skip-and-log on device error** — error counted, message logged, loop continues — directly required by PROJECT.md
- [x] **InfluxDB 3 write via `/api/v3/write_lp`** with `Authorization: Bearer` — per-device measurement name = device name
- [x] **InfluxDB write failure: log error and continue** — network blip must not crash the daemon
- [x] **Graceful shutdown on SIGTERM/SIGINT** — systemd needs this; completes current cycle before exiting
- [x] **Structured log to stdout** + optional **log to file** — configurable path and level; systemd/journald integration
- [x] **Config validation at startup** — clear error messages before entering poll loop
- [x] **Systemd `.service` unit file** — in repo; `Restart=always`

### Add After Validation (v1.x)

Features to add once the core loop is proven stable over days/weeks of real operation.

- [ ] **In-memory write-failure buffer with retry** — triggered when InfluxDB goes offline for minutes; first validate that the core loop is reliable before adding buffering complexity
- [ ] **Startup InfluxDB connectivity check** — good DX addition; add after verifying write path is solid
- [ ] **Per-device consecutive error counter** — useful for dashboards once live; adds observability after basic data flow is confirmed
- [ ] **CLI `--config` argument** — default path is fine for v1; add when second deployment is needed

### Future Consideration (v2+)

Features to defer until the daemon has been in production and real needs are validated.

- [ ] **Log rotation config** — document `logrotate` approach first; build-in only if that proves insufficient
- [ ] **Configurable Modbus read timeout per-device** — PZEM-016 always responds within 200ms; only needed if devices with different timing are added
- [ ] **Multiple serial port support** — only relevant when hardware scales beyond one USB adapter
- [ ] **Cross-compilation / CI release pipeline** — useful for distribution; not needed for single-Pi deployment

---

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| TOML config (devices, serial, InfluxDB) | HIGH | LOW | P1 |
| FC04 register read (all 6 PZEM-016 fields) | HIGH | MEDIUM | P1 |
| Sequential polling loop with global interval | HIGH | LOW | P1 |
| Skip-and-log on device error | HIGH | LOW | P1 |
| InfluxDB 3 write (line protocol, per-device measurement) | HIGH | MEDIUM | P1 |
| InfluxDB write failure: log + continue | HIGH | LOW | P1 |
| Graceful SIGTERM/SIGINT shutdown | HIGH | LOW | P1 |
| Structured logging to stdout + optional file | HIGH | LOW | P1 |
| Startup config validation | HIGH | LOW | P1 |
| Systemd service unit | HIGH | LOW | P1 |
| In-memory write-failure buffer + retry | MEDIUM | MEDIUM | P2 |
| Startup InfluxDB connectivity check | MEDIUM | LOW | P2 |
| Per-device consecutive error counter | MEDIUM | LOW | P2 |
| CLI `--config` argument | LOW | LOW | P2 |
| Log rotation | LOW | LOW | P3 |
| Configurable per-device Modbus timeout | LOW | LOW | P3 |

**Priority key:**
- P1: Must have for launch
- P2: Should have, add when possible
- P3: Nice to have, future consideration

---

## Competitor Feature Analysis

*Note: This is a narrow embedded Rust daemon, not a commercial product. The relevant "competitors" are existing open-source Modbus-to-InfluxDB loggers and Python/Node.js scripts.*

| Feature | Typical Python/Node script | PyScada (full SCADA) | Our Approach |
|---------|--------------------------|----------------------|--------------|
| Config format | Hardcoded or `.env` | Django admin UI + DB | TOML file — human-editable, version-controllable |
| Error handling | Script crashes on first error | Daemon with retry | Skip-and-continue: daemon never dies due to one device |
| Resource usage on Pi | ~50MB RAM (Python runtime) | ~200MB+ (Django + DB) | ~2–5MB RAM (Rust static binary) |
| InfluxDB 3 support | Rare (most target v1/v2 API) | No v3 native support | Native v3 `/api/v3/write_lp` endpoint |
| Systemd integration | `ExecStart=python3 ...` with no restart | Complex multi-process | Single binary, `Restart=always`, `Type=simple` |
| Deployment | Copy script + pip install | Full SCADA install | Single cross-compiled binary + one config file |
| Bus safety | Often uses threads, risks collision | Worker process per device | Single async task, sequential per-device reads |

---

## Sources

- PZEM-004T v3.0 register map and multidevice usage: [github.com/mandulaj/PZEM-004T-v30](https://github.com/mandulaj/PZEM-004T-v30) — HIGH confidence
- InfluxDB 3 `/api/v3/write_lp` endpoint, auth, line protocol: [docs.influxdata.com/influxdb3/core/write-data/http-api/v3-write-lp/](https://docs.influxdata.com/influxdb3/core/write-data/http-api/v3-write-lp/) — HIGH confidence
- InfluxDB 3 write best practices (batch, precision, gzip): [docs.influxdata.com/influxdb3/core/write-data/best-practices/optimize-writes/](https://docs.influxdata.com/influxdb3/core/write-data/best-practices/optimize-writes/) — HIGH confidence
- tokio-modbus 0.17.0 RTU client API: [docs.rs/tokio-modbus](https://docs.rs/tokio-modbus/latest/tokio_modbus/) — HIGH confidence
- tracing + tracing-subscriber logging ecosystem: [docs.rs/tracing](https://docs.rs/tracing/latest/tracing/) — HIGH confidence
- PyScada SCADA (competitor feature comparison): [github.com/pyscada/PyScada](https://github.com/pyscada/PyScada) — MEDIUM confidence
- Modbus RTU RS485 logger ecosystem survey: GitHub Topics modbus-rtu (379 repos), modbus-logger (2 repos) — MEDIUM confidence
- PZEM-016 register layout and Modbus addressing: training data (matches PZEM-004T v3.0 datasheet patterns) — MEDIUM confidence (should be verified against physical hardware in Phase 1)

---

*Feature research for: RS485 Modbus RTU PZEM-016 data logger daemon (Rust, Raspberry Pi, InfluxDB 3)*
*Researched: 2026-04-02*
