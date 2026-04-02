---
type: quick
description: Create ARCHITECTURE.md explaining how the program works with Rust language explanations for developers unfamiliar with Rust
autonomous: true
files_modified:
  - ARCHITECTURE.md
---

<objective>
Create a comprehensive ARCHITECTURE.md document that explains how rs485-logger works, with particular attention to Rust-specific concepts that developers from other languages (Python, JS, Go) might not immediately understand.

Purpose: Help developers unfamiliar with Rust understand the codebase — both the architecture/data flow and the Rust idioms used throughout.
Output: ARCHITECTURE.md at project root
</objective>

<context>
@src/main.rs
@src/config.rs
@src/types.rs
@src/influx.rs
@src/poller.rs
@Cargo.toml
@config.toml
@deploy/rs485-logger.service
</context>

<tasks>

<task type="auto">
  <name>Task 1: Write ARCHITECTURE.md</name>
  <files>ARCHITECTURE.md</files>
  <action>
Create ARCHITECTURE.md at the project root. The document should cover:

**1. High-Level Overview**
- ASCII diagram showing: PZEM-016 devices → RS485 bus → USB adapter → Raspberry Pi → rs485-logger daemon → InfluxDB 3
- The daemon's single-threaded async loop: open serial port once → tick every N seconds → poll each device sequentially → write each reading to InfluxDB → repeat until SIGTERM/SIGINT
- Why sequential polling matters (single RS485 bus = shared wire, only one device can talk at a time)

**2. Source File Map**
A table mapping each file to its responsibility:
| File | Purpose |
|------|---------|
| `src/main.rs` | Entry point: CLI arg parsing, config loading, tracing init, poll loop with graceful shutdown |
| `src/config.rs` | TOML config structs (`AppConfig`, `SerialConfig`, `InfluxConfig`, `DeviceConfig`) + validation |
| `src/types.rs` | `PowerReading` struct and `decode_registers()` — decodes raw Modbus registers into typed readings |
| `src/influx.rs` | `InfluxWriter` and `to_line_protocol()` — formats readings as InfluxDB line protocol, sends via HTTP |
| `src/poller.rs` | `ModbusPoller` — opens serial port, switches Modbus slave address per device, reads input registers |
| `config.toml` | Runtime configuration (serial port, InfluxDB connection, device list) |
| `deploy/` | systemd service unit, udev rule, install/build scripts for Raspberry Pi |

**3. Data Flow (step-by-step)**
Walk through one complete poll cycle with code references:
1. `main.rs:117-119` — `tokio::time::interval` creates a ticker
2. `main.rs:129` — ticker fires → iterate over `cfg.devices`
3. `poller.rs:48` — `set_slave()` switches to device's Modbus address
4. `poller.rs:56-63` — `read_input_registers(0x0000, 10)` with 500ms timeout, triple error unwrap
5. `types.rs:33-59` — `decode_registers()` converts 10 raw u16 registers into `PowerReading`
6. `influx.rs:12-25` — `to_line_protocol()` formats as `measurement field=val,... timestamp_ns`
7. `influx.rs:46-68` — `InfluxWriter::write()` POSTs to `/api/v3/write_lp?db=...&precision=ns`

**4. Rust Concepts for Non-Rust Developers**
For each concept, give a 2-3 sentence explanation plus the equivalent in familiar languages:

- **Ownership & Borrowing (`&`, `&mut`)** — Explain that `&cfg.serial` is a read-only borrow (like a pointer that can't modify), `&mut self` means exclusive mutable access. Relate to: const references in C++, readonly in TS.

- **`Result<T, E>` and the `?` operator** — Rust has no exceptions. Every function that can fail returns `Result<Ok, Err>`. The `?` operator is shorthand for "if Err, return early with the error". Show the triple `?` chain in `poller.rs:56-63` and explain each layer.

- **`anyhow::Result` and `.with_context()`** — `anyhow` is an error library that lets you chain context strings onto errors (like wrapping exceptions with messages). `with_context(|| "msg")?` adds "msg" to the error if it fails. Compare to: try/except with re-raise in Python, wrapping errors in Go.

- **`#[derive(Debug, Deserialize)]`** — Rust's compile-time code generation. `Deserialize` auto-generates TOML/JSON parsing code for a struct at compile time — no reflection, no runtime cost. Compare to: decorators in Python, but runs at compile time.

- **`async/await` and `tokio`** — Rust async is zero-cost (no garbage collector, no green threads). `tokio` is the runtime that drives async futures. `#[tokio::main(flavor = "current_thread")]` means single-threaded — appropriate here because RS485 is inherently sequential.

- **`tokio::select!`** — Waits on multiple async futures simultaneously, runs the branch of whichever completes first. Used in `main.rs:128-160` to race the poll ticker against the shutdown signal. Compare to: `select` in Go, `Promise.race` in JS.

- **`mod` and `use` (modules)** — Each `.rs` file is a module. `mod config;` in `main.rs` tells the compiler to include `src/config.rs`. `use config::load_config;` imports a specific function. Compare to: `import` in Python/JS.

- **`pub` visibility** — Everything is private by default. `pub` makes it accessible from other modules. `pub struct`, `pub fn`, `pub field` each need explicit opt-in. Compare to: default private in Java/Kotlin, opposite of Python/JS.

- **`Option<T>` (nullable types)** — Rust has no null. `Option<String>` means "might be a String, might be None". The `log_file: Option<String>` in config means this field is optional in the TOML. Compare to: `Optional` in Java, `T | None` in Python, `T | undefined` in TS.

- **Lifetimes (brief mention)** — Explain that the `'_` or `&str` vs `String` distinction is Rust's way of tracking how long references are valid. For this codebase, it mostly means "we use `String` (owned) in structs and `&str` (borrowed) in function parameters".

- **`#[cfg(test)]` and `mod tests`** — Test modules live inside the same file as the code they test. `#[cfg(test)]` means this code is only compiled when running tests. `cargo test` runs them.

**5. Error Handling Strategy**
- Device poll failure → `warn!` + skip device, continue to next (no crash)
- InfluxDB write failure → `warn!` + continue polling (data gap, but daemon stays alive)
- Config error → `eprintln!` + `exit(1)` (fatal, before logging is initialized)
- Serial port open failure → bubble up via `?` → daemon exits (can't recover without hardware)

**6. Deployment Architecture**
- systemd service runs as dedicated `rs485logger` user
- `SupplementaryGroups=dialout` grants serial port access without root
- `After=network-online.target` ensures InfluxDB is reachable on boot
- `Restart=always` + `RestartSec=5` for auto-recovery
- Logs go to journald (stderr) + optional file via `tracing-appender`

**Formatting guidelines:**
- Use clear section headers with `##`
- Use code blocks with Rust syntax highlighting for code snippets
- Keep explanations conversational, not academic
- Use ASCII art for diagrams (not mermaid — works everywhere)
- Target length: 200-350 lines
  </action>
  <verify>
    <automated>test -f ARCHITECTURE.md && wc -l ARCHITECTURE.md | awk '{if ($1 >= 100) print "OK:",$1,"lines"; else print "FAIL: too short",$1,"lines"}'</automated>
  </verify>
  <done>ARCHITECTURE.md exists at project root, covers all 6 sections, includes Rust concept explanations with cross-language comparisons, and is readable by a developer unfamiliar with Rust</done>
</task>

</tasks>

<verification>
- ARCHITECTURE.md exists at project root
- Document covers: overview, file map, data flow, Rust concepts, error handling, deployment
- Rust concepts include cross-language comparisons (Python, JS, Go equivalents)
- Code references point to actual line numbers and file paths
</verification>

<success_criteria>
A developer who knows Python/JS/Go but not Rust can read ARCHITECTURE.md and understand:
1. What the program does and how data flows through it
2. What each source file is responsible for
3. The key Rust-specific patterns used (Result, ownership, async, derive macros)
4. How errors are handled and why the daemon stays alive when individual devices fail
</success_criteria>
