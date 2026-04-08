# Decisions

<!-- Append-only register of architectural and pattern decisions -->

| ID | Decision | Rationale | Date |
|----|----------|-----------|------|
| D001 | Use Rust for implementation | Low memory footprint on Pi, strong serial/async ecosystem, reliability | 2026-04-02 |
| D002 | TOML config format | Idiomatic in Rust ecosystem (serde + toml crate), human-friendly, project constraint | 2026-04-02 |
| D003 | Per-device InfluxDB measurement (named) | Allows per-device dashboards and queries without tag filtering | 2026-04-02 |
| D004 | Skip-and-log on device error (continue polling) | Partial data is better than no data; daemon must stay alive when individual devices fail | 2026-04-02 |
| D005 | Global polling interval (not per-device) | Simplifies scheduling; PZEM-016 response time makes per-device intervals unnecessary | 2026-04-02 |
| D006 | tokio-modbus 0.17 + `rtu::attach(port)` with `set_slave()` | Only async RTU crate integrating with tokio-serial; slave switched per call — no port reopen | 2026-04-02 |
| D007 | tracing-subscriber with EnvFilter | journald-compatible structured logging; RUST_LOG + log_level config + file appender | 2026-04-02 |
| D008 | reqwest with `rustls-tls` feature | Avoids OpenSSL system library dependency during cross-compilation to ARM | 2026-04-02 |
| D009 | `{:.4}` float formatting for all PZEM fields | Prevents InfluxDB 3 integer type lock-in (STOR-03) | 2026-04-02 |
| D010 | chrono-tz for energy reset timezone | Bundles IANA tz database at compile time; avoids runtime system tz dependency | 2026-04-03 |
| D011 | `far_future()` parks disabled select! arm | No conditional select! needed; clean biased select! structure when energy reset is disabled | 2026-04-03 |
| D012 | CRIT-02: exit + systemd restart (not in-process serial reconnect) | Simpler, more reliable; systemd handles restart correctly; no hardware to test reconnect logic | 2026-04-03 |
| D013 | InfluxDB health tracking per-daemon (not per-device) | All devices write to same InfluxDB instance — one health flag is correct; avoids per-device state bloat | 2026-04-03 |
| D014 | `tokio current_thread` runtime | Single RS485 bus needs sequential polling; eliminates Send bounds on serial handles; lower Pi memory | 2026-04-02 |
| D015 | Raw reqwest for InfluxDB writes (no client library) | No official Rust InfluxDB 3 client exists; community crate has 210 downloads; 3 lines of reqwest is safer | 2026-04-02 |
