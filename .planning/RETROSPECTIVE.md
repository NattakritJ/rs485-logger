# Project Retrospective

*A living document updated after each milestone. Lessons feed forward into future planning.*

## Milestone: v1.0 — MVP

**Shipped:** 2026-04-03
**Phases:** 7 | **Plans:** 16 | **Commits:** 69

### What Was Built

- Rust daemon that polls PZEM-016 power meters over Modbus RTU RS485 and writes to InfluxDB 3 (voltage, current, power, energy, frequency, power factor per device)
- Systemd service unit with udev stable `/dev/ttyRS485` symlink, cross-compilation config (aarch64/armv7), and idempotent `install.sh`
- Daily energy reset scheduler via Modbus FC 0x42 with IANA timezone support (chrono-tz), select! biased integration
- Daemon reliability hardening: HTTP timeouts, serial recovery via exit+systemd-restart, InfluxDB health tracking, config validation (device name sanitization, eager timezone validation), daily log rotation
- Comprehensive E2E README.md (hardware wiring → Raspberry Pi setup → deployment → InfluxDB verification)

### What Worked

- **Phase sequencing was correct**: no hardware required until Phase 3 — all of Phase 1 (config/types) and Phase 2 (InfluxDB) TDD'd offline. Clear testability gates per phase.
- **TDD red/green cycle**: writing tests first for `decode_registers()`, `to_line_protocol()`, and `next_reset_instant()` caught integration issues early without hardware.
- **Tech stack research upfront**: STACK.md locked all crate versions before Phase 1 started — zero dependency surprises mid-build.
- **tokio current_thread**: choosing single-threaded runtime from the start eliminated `Send` bound complexity that multi-threaded polling would have required.
- **reqwest rustls feature**: avoiding OpenSSL made cross-compilation to ARM completely dependency-free — no `Cross.toml` OpenSSL workarounds needed.
- **Phase 7 reliability hardening**: adding a dedicated hardening phase before shipping paid off — 14 concrete findings all addressed systematically.

### What Was Inefficient

- **STOR-01–STOR-04 checkboxes left stale**: requirements in REQUIREMENTS.md were never updated after Phase 2 implementation — created audit confusion; a quick checkbox update after each phase would prevent this.
- **5 phases without VERIFICATION.md**: phases 1, 2, 4, 5, 6 were executed without running `/gsd-verify-phase`. Phase 3 and 7 had formal verification. A lighter "did tests pass + success criteria met?" check per phase would improve traceability without much overhead.
- **REL-01 orphaned REQ-ID**: Phase 7 was added as a scope expansion but REL-01 was never formally added to REQUIREMENTS.md. New requirement IDs should be added to REQUIREMENTS.md at phase creation time.
- **RISK-1 doc mismatch (udev driver)**: `ch341` vs `cp210x` inconsistency slipped through — caught by audit but not during development. More careful cross-referencing deploy artifacts with README would catch this earlier.

### Patterns Established

- **far_future() for optional scheduled tasks**: parking a `sleep_until` at 100-years-hence avoids conditional `select!` guards — clean biased select! structure
- **exit+systemd-restart for unrecoverable failures**: simpler and more reliable than in-process reconnect for a daemon without local hardware test environment
- **`#[ignore]` gate integration tests**: integration tests that need live InfluxDB are `#[ignore]`-gated with `INFLUX_TOKEN` env var guard — safe for CI, runnable on real hardware
- **config-first init**: load config before tracing init (use `eprintln!` for errors) — enables file appender path from config without double-init
- **AtomicBool for one-shot warnings**: system clock warning fires only once across all poll iterations without a mutable flag in the main loop

### Key Lessons

1. **Close requirement checkboxes at phase completion, not at milestone end.** Four STOR requirements were implemented in Phase 2 but checkboxes were not updated — caused audit confusion and extra work at milestone close.
2. **Add new REQ-IDs to REQUIREMENTS.md when the phase is created.** DOC-01, SCHED-01, and REL-01 were added as phases but never formally tracked in REQUIREMENTS.md traceability table.
3. **Hardware-gated success criteria need explicit marking.** Several Phase 3 and Phase 7 success criteria require physical RS485 hardware — these should be clearly marked `[hardware-required]` in plans to set correct expectations.
4. **Parity belongs in config from day one.** CFG-02 explicitly required configurable parity but it was not implemented. Even if PZEM-016 uses 8N1 by default, the requirement stated it — ship it or explicitly change the requirement.
5. **Cross-referencing deploy artifacts with docs prevents mismatches.** `deploy/99-rs485.rules` used `ch341` while README said `cp210x` — a simple review of deploy/ + README together at Phase 4 completion would have caught this.

### Cost Observations

- Model: claude-sonnet-4.6 (OpenCode)
- Sessions: multi-session (2026-04-02 → 2026-04-03)
- Notable: average ~10 min/plan across 16 plans; Phase 3 plan 01 (ModbusPoller) took ~12 min (most complex); Phase 4 plans were fastest at ~7 min each

---

## Cross-Milestone Trends

### Process Evolution

| Milestone | Phases | Plans | Key Change |
|-----------|--------|-------|------------|
| v1.0 | 7 | 16 | Initial project — baseline established |

### Cumulative Quality

| Milestone | Tests | Notes |
|-----------|-------|-------|
| v1.0 | 39 passing | Unit + integration tests; hardware-gated tests #[ignore]-gated |

### Top Lessons (Verified Across Milestones)

1. Close requirement checkboxes at phase completion to prevent stale docs at milestone close
2. New requirements added mid-milestone must be formally recorded in REQUIREMENTS.md at phase creation
