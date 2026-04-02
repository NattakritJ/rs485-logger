# GSD Debug Knowledge Base

Resolved debug sessions. Used by `gsd-debugger` to surface known-pattern hypotheses at the start of new investigations.

---

## log-level-config-ignored — log_level in config.toml always ignored; program always logs at info
- **Date:** 2026-04-02
- **Error patterns:** log_level, config.toml, tracing, EnvFilter, always info, log level ignored, TOML, [[devices]], Option None
- **Root cause:** TOML scoping trap — log_level and log_file keys were placed after [[devices]] array-of-tables blocks in config.toml. Per TOML spec, keys after [[array]] headers belong to that table entry and cannot return to root scope. Both keys were silently absorbed into devices[0] and discarded by serde, leaving AppConfig.log_level = None and always using the "info" default. A secondary bug also existed: EnvFilter::try_from_default_env() (RUST_LOG) was evaluated before cfg.log_level, meaning any set RUST_LOG would silently override the config value.
- **Fix:** (1) Moved log_level and log_file before all [section]/[[array]] headers in config.toml with ordering warning comments. (2) Fixed EnvFilter precedence in main.rs to if/else on cfg.log_level so config wins over RUST_LOG. (3) Added regression guard test test_log_level_after_devices_is_not_parsed_as_root in config.rs.
- **Files changed:** config.toml, src/main.rs, src/config.rs
---
