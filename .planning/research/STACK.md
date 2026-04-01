# Stack Research

**Domain:** Rust embedded/IoT daemon — RS485/Modbus RTU → InfluxDB 3 time-series logger
**Researched:** 2026-04-02
**Confidence:** HIGH (all versions verified against crates.io; InfluxDB 3 write API verified against official docs)

---

## Recommended Stack

### Core Technologies

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| `tokio` | 1.50.0 | Async runtime | The de-facto Rust async runtime; `tokio-modbus` and `reqwest` both depend on it — no alternative viable. Feature `full` or targeted `rt-multi-thread,time,signal,io-util` |
| `tokio-modbus` | 0.17.0 | Modbus RTU client | Only async Modbus RTU crate that integrates directly with tokio-serial; maintained (Dec 2024 release), uses tokio ^1.35, has RTU + sync variants. The `rtu` feature (default) is exactly what's needed |
| `tokio-serial` | 5.4.5 | Async serial port (tokio I/O) | tokio-modbus depends on `tokio-serial ^5.4.4`; wraps `mio-serial` and exposes a `tokio::io::AsyncRead/Write` stream; the only tokio-native serial crate |
| `reqwest` | 0.13.2 | HTTP client for InfluxDB writes | Async HTTP client, built on hyper/tokio; InfluxDB 3 writes are simple `POST /api/v3/write_lp` calls with line protocol body — reqwest handles auth headers, retries cleanly |
| `serde` | 1.0.228 | Config deserialization framework | Essential for `#[derive(Deserialize)]` on config structs; used by `toml` crate |
| `toml` | 1.1.1 | TOML config parsing | Project requirement; `toml::from_str::<T>()` with serde is the standard idiomatic pattern |
| `tracing` | 0.1.44 | Structured logging facade | Superior to `log`: structured fields, async-aware, spans for per-device context; integrates with `tracing-subscriber` for output routing |
| `tracing-subscriber` | 0.3.23 | Log output routing (console + file) | Provides `EnvFilter` (runtime log level control), `fmt` layer for human-readable output, and composes with `tracing-appender` for file output |
| `tracing-appender` | 0.2.4 | Non-blocking file log writer | Part of the tokio-rs/tracing family; provides `rolling::daily`/`rolling::never` file appender + non-blocking writer so file I/O doesn't block the async runtime |
| `anyhow` | 1.0.102 | Error handling | Ergonomic error propagation with context chaining; correct for a binary daemon (not a library) — avoids boilerplate `Box<dyn Error>` or custom error enums for glue code |

### Supporting Libraries

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `serde_derive` | (bundled in `serde` with `features = ["derive"]`) | `#[derive(Deserialize, Serialize)]` macros | Always — enable via `serde = { features = ["derive"] }` |
| `tokio-util` | 0.7.x | Codec framing utilities | Only if you drop `tokio-modbus` and implement the RTU framer manually (not recommended) |
| `thiserror` | 2.x | Typed error definitions | Use if you add a library layer or want typed errors at module boundaries; not needed for the top-level binary glue |

### Development Tools

| Tool | Purpose | Notes |
|------|---------|-------|
| `cargo cross` | Cross-compilation for ARM targets | Essential for building on x86 Mac/Linux and deploying to Raspberry Pi; `cross build --target aarch64-unknown-linux-gnu --release` or `armv7-unknown-linux-gnueabihf` for Pi 2/3 32-bit |
| `cargo clippy` | Lint enforcement | Run with `-- -D warnings` in CI to catch common Rust mistakes |
| `cargo-audit` | Dependency vulnerability scanning | Run before releases; tokio ecosystem is well-maintained but good hygiene |
| `systemd` service unit | Process supervision | No crate needed; write a `.service` file with `Restart=always`, `RestartSec=5`, logging via journald + tracing-appender file |

---

## Installation (`Cargo.toml` dependencies)

```toml
[dependencies]
# Async runtime
tokio = { version = "1", features = ["rt-multi-thread", "time", "signal", "macros"] }

# Modbus RTU over RS485
tokio-modbus = { version = "0.17", default-features = false, features = ["rtu"] }
tokio-serial = "5.4"

# InfluxDB 3 HTTP writes
reqwest = { version = "0.13", default-features = false, features = ["rustls-tls", "json"] }

# Config
serde = { version = "1", features = ["derive"] }
toml = "1"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
tracing-appender = "0.2"

# Error handling
anyhow = "1"

[profile.release]
opt-level = "z"   # Minimize binary size on Pi
strip = true
lto = true
```

---

## InfluxDB 3 Write API — Critical Details

**This differs significantly from v1/v2 — read carefully.**

| Aspect | InfluxDB v2 | InfluxDB v3 (use this) |
|--------|-------------|------------------------|
| Endpoint | `POST /api/v2/write?bucket=…&org=…` | `POST /api/v3/write_lp?db=<DATABASE>` |
| Auth header | `Authorization: Token <token>` | `Authorization: Bearer <token>` |
| Org parameter | Required (`org=…`) | **Not used** in v3 endpoint |
| Precision parameter | `?precision=ns` (required) | `?precision=<unit>` (optional; default `auto` detects from timestamp magnitude) |
| Success response | `HTTP 204` | `HTTP 204` |
| Content type | `text/plain` | Line protocol plaintext (same format) |

**Minimal write request (reqwest example):**
```rust
client
    .post(format!("{}/api/v3/write_lp", config.influxdb.url))
    .bearer_auth(&config.influxdb.token)
    .query(&[("db", &config.influxdb.database)])
    .body(line_protocol_string)
    .send()
    .await?;
```

**Line protocol format for PZEM-016:**
```
# measurement=device_name, no tags needed (already one measurement per device)
kitchen_panel voltage=230.5,current=12.3,power=2839.65,energy=1234.5,frequency=50.0,power_factor=0.98 1712000000000000000
```

---

## Alternatives Considered

| Recommended | Alternative | Why Not |
|-------------|-------------|---------|
| `tokio-modbus 0.17` | `rmodbus 0.12.2` | `rmodbus` is `no_std`/embedded-focused with manual frame handling — requires you to implement the serial framing loop yourself. Extra code for no benefit on Linux |
| `tokio-modbus 0.17` | Custom RTU over `tokio-serial` | Re-inventing what `tokio-modbus` already does correctly (CRC, frame timeouts, retry logic) |
| `reqwest 0.13` | `influxdb3 0.2.0` crate | 210 total downloads, GitLab-hosted, community crate with no InfluxData backing. InfluxData's official Rust client library **does not exist yet** (only Go, Python, Java, JS, C# listed in official docs). Raw `reqwest` with 3 lines of code is safer and has zero dependency risk |
| `reqwest 0.13` | `influxdb2` crate (v2 client) | Targets v2 API — wrong endpoint, wrong auth format, wrong org/bucket semantics |
| `tracing` | `log` + `env_logger` | `log` has no structured fields and no async spans. `tracing` is the 2025 standard for async Rust daemons |
| `toml` direct | `config` crate | `config` crate adds complexity (layered configs, env overrides) that the project explicitly out-of-scopes; `toml::from_str` + serde is 10 lines and zero magic |
| `anyhow` | `thiserror` | `thiserror` is for libraries that expose typed errors to callers. A daemon binary benefits from `anyhow` context chains in logs, not typed error variants |
| `tokio-serial 5.4.5` | `serialport 4.9.0` (sync) | `serialport` is synchronous; wrapping it in `spawn_blocking` inside tokio works but is clunky. `tokio-serial` is the tokio-native wrapper that `tokio-modbus` expects |

---

## What NOT to Use

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| `influxdb` crate (v1 client) | Targets InfluxDB 1.x line protocol endpoint `/write` — incompatible with v3 API routes and auth | `reqwest` with `POST /api/v3/write_lp` |
| `influxdb2` crate | Targets `/api/v2/write` with `Token` header and `org=` param — both wrong for v3 | `reqwest` with `Bearer` header and `db=` param |
| `rmodbus` for Linux daemon | Designed for `no_std` embedded; requires manual byte buffer management on Linux | `tokio-modbus` |
| `async-std` runtime | Incompatible with tokio-modbus and reqwest (both require tokio); mixing runtimes causes panics | `tokio` |
| `serialport 4.x` (sync) | Forces `spawn_blocking` wrappers; `tokio-modbus` does not accept it as a transport | `tokio-serial 5.4.5` |
| `log` + `env_logger` | No structured fields, no spans, no async context — insufficient for a multi-device polling daemon where per-device error context matters | `tracing` + `tracing-subscriber` |
| `config` crate | Over-engineered for TOML-only config; adds implicit env-var behavior that project explicitly excludes | `toml::from_str` + `serde` |

---

## Stack Patterns by Variant

**If deploying to 32-bit Pi (Pi 2/3 without 64-bit OS):**
- Cross-compile target: `armv7-unknown-linux-gnueabihf`
- Add `reqwest` with `rustls-tls` (not `native-tls`) — avoids OpenSSL system library dependency during cross-compilation

**If deploying to 64-bit Pi OS (Pi 3B+/4/5 with arm64):**
- Cross-compile target: `aarch64-unknown-linux-gnu`
- Both `rustls-tls` and `native-tls` work; prefer `rustls-tls` for static linking simplicity

**If compiling directly on the Pi (slow but zero cross-compile setup):**
- Install Rust via `rustup` on the Pi
- `cargo build --release` works natively; expect 5-15 min compile time for first build

**If InfluxDB instance uses self-signed TLS:**
- Add `.danger_accept_invalid_certs(true)` to `reqwest::ClientBuilder` OR
- Bundle the CA cert and use `.add_root_certificate(cert)` — prefer the latter for production

---

## Version Compatibility

| Package | Compatible With | Notes |
|---------|-----------------|-------|
| `tokio-modbus 0.17` | `tokio ^1.35`, `tokio-serial ^5.4.4` | Verified from docs.rs dependency list |
| `tokio-serial 5.4.5` | `tokio 1.x`, `mio-serial` | Last updated Dec 2024; 5.4.5 is latest stable |
| `reqwest 0.13` | `tokio 1.x`, `hyper 1.x` | Reqwest 0.13 moved to hyper 1.x — major internal change from 0.11/0.12; do not mix with reqwest 0.11 middleware |
| `tracing-subscriber 0.3` | `tracing 0.1` | Same major version family; all 0.1.x tracing crates are compatible |
| `tracing-appender 0.2.4` | `tracing-subscriber 0.3` | Compose via `tracing_subscriber::registry().with(layer)` |
| `toml 1.x` | `serde 1.x` | `toml` 1.x (spec 1.1.0) is a major rewrite from 0.7; use `1` not `0.8` |
| `serde 1.0.228` | Universal | Stable semver; no breaking changes since 1.0 |

---

## Sources

- crates.io API — `tokio-serial` max_stable_version: `5.4.5` (updated 2024-12-31)
- crates.io API — `tokio-modbus` default_version: `0.17.0`
- docs.rs tokio-modbus 0.17.0 — feature flags (`rtu`, `tcp`), dependency list (tokio-serial ^5.4.4 confirmed)
- crates.io API — `serialport` max_stable_version: `4.9.0` (updated 2026-03-16)
- crates.io API — `reqwest` default_version: `0.13.2`
- crates.io API — `tokio` default_version: `1.50.0`
- crates.io API — `serde` default_version: `1.0.228`
- crates.io API — `toml` default_version: `1.1.1+spec-1.1.0`
- crates.io API — `tracing` default_version: `0.1.44`
- crates.io API — `tracing-subscriber` default_version: `0.3.23`
- crates.io API — `tracing-appender` default_version: `0.2.4`
- crates.io API — `anyhow` default_version: `1.0.102`
- crates.io API — `rmodbus` default_version: `0.12.2`
- crates.io API — `influxdb3` default_version: `0.2.0` (community crate, 210 total downloads — LOW confidence for production use)
- **Official InfluxDB 3 docs** — `POST /api/v3/write_lp`, `Authorization: Bearer`, `?db=` parameter, `HTTP 204` success — HIGH confidence — https://docs.influxdata.com/influxdb3/core/write-data/http-api/v3-write-lp/
- **Official InfluxDB 3 client library list** — No official Rust v3 client exists; Go/Python/Java/JS/C# only — https://docs.influxdata.com/influxdb3/core/reference/client-libraries/v3/

---
*Stack research for: Rust RS485/Modbus RTU → InfluxDB 3 daemon on Raspberry Pi*
*Researched: 2026-04-02*
