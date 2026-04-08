# S01: Foundation

**Status:** ✅ Completed 2026-04-02
**Goal:** Parse and validate TOML config; define `PowerReading` struct with correct PZEM-016 register decode logic; all logic unit-testable with no external dependencies.
**Demo:** `cargo test` passes with a sample `config.toml`; `decode_registers()` unit test produces correct voltage/current/power/energy/frequency/power_factor values with correct scaling and low-word-first 32-bit reconstruction.

## Must-Haves

- Project skeleton: Cargo.toml with all dependencies + src stubs that compile
- Config structs + TOML parsing + startup validation (TDD)
- PowerReading struct + `decode_registers()` with correct PZEM-016 word order (TDD)

## Tasks

- [x] T01: Project skeleton — Cargo.toml + src stubs that compile
- [x] T02: Config structs + TOML parsing + startup validation TDD
- [x] T03: PowerReading struct + decode_registers() TDD

## Files Likely Touched

- `Cargo.toml`
- `src/main.rs`
- `src/config.rs`
- `src/types.rs`
