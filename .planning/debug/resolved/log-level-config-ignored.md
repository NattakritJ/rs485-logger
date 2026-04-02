---
status: resolved
trigger: "log_level field in config.toml is parsed but never applied — the program always logs at a fixed default level regardless of configuration"
created: 2026-04-02T00:00:00Z
updated: 2026-04-02T00:20:00Z
symptoms_prefilled: true
---

## Current Focus

hypothesis: CONFIRMED (second, true root cause) — log_level = "warn" placed after [[devices]] in TOML is silently absorbed into the last device table entry, not parsed as a root-level AppConfig field. AppConfig.log_level stays None, so the "info" default is always used.
test: Verified via TOML Value probe: parsed tree shows log_level inside devices[0], not at root.
expecting: N/A
next_action: All fixes applied — awaiting human verification.

## Symptoms

expected: The program should respect the log_level value from config.toml and only log at or above that level.
actual: Always uses the default log level regardless of what log_level is set to in config.toml.
errors: No errors — program starts fine, just uses wrong log level.
reproduction: Set log_level = "warn" or "error" in config.toml and run the program — lower-level logs still appear.
started: Unknown — may never have worked.

## Eliminated

- hypothesis: "EnvFilter precedence inverted — RUST_LOG wins over cfg.log_level"
  evidence: |
    This was a real secondary bug (also fixed), but NOT the cause of the user's symptom.
    Probe confirmed: registry().with(EnvFilter::new("warn")).with(fmt::Layer) correctly
    suppresses info events. The subscriber wiring was sound.
    The precedence fix was still correct and worth keeping (it implements the documented
    priority: cfg.log_level > RUST_LOG > "info"), but it didn't fix the observable symptom
    because cfg.log_level was always None regardless.
  timestamp: 2026-04-02T00:10:00Z

## Evidence

- timestamp: 2026-04-02T00:07:00Z
  checked: src/config.rs log_level_tests::test_log_level_parsed_as_warn — does toml parse log_level = "warn" after [[devices]]?
  found: Test FAILED — cfg.log_level was None despite log_level = "warn" being in the TOML string.
  implication: TOML parsing is not putting log_level where we expect. Structural TOML issue.

- timestamp: 2026-04-02T00:08:00Z
  checked: TOML Value probe — parsed the raw TOML with log_level after [[devices]] as toml::Value
  found: |
    log_level ended up INSIDE devices[0]:
      devices: [{ address: 1, name: "solar_panel", log_level: "warn" }]
    root log_level = None
    
    TOML spec: keys after a [[array-of-tables]] header belong to that table entry
    until the next header. There is no way to "return" to root scope after entering
    [[devices]]. So log_level = "warn" placed after [[devices]] is a field of 
    DeviceConfig, not AppConfig. Since DeviceConfig has no log_level field, serde 
    ignores it silently. AppConfig.log_level stays None → "info" default always used.
  implication: |
    True root cause. The fix is to move log_level (and log_file) before any [[devices]]
    blocks in config.toml. The Rust struct and main.rs logic are correct — the config
    file layout is wrong.
  checked: src/main.rs lines 66-72 — EnvFilter construction logic
  found: |
    Line 67: `let log_level = cfg.log_level.as_deref().unwrap_or("info");`
    Line 68: `let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()`
    Line 69-71: `.unwrap_or_else(|_| EnvFilter::try_new(log_level)...)`
    
    try_from_default_env() reads RUST_LOG. If RUST_LOG is set (even to "info"),
    it succeeds and the unwrap_or_else closure never runs — log_level from config
    is silently ignored. If RUST_LOG is NOT set at all, then cfg.log_level is used.
  implication: |
    Most environments will have RUST_LOG unset, so the fallback kicks in.
    BUT the comment says priority is "cfg.log_level > RUST_LOG > info", while 
    the actual code implements "RUST_LOG > cfg.log_level > info".
    The user's symptom (always uses default) likely means RUST_LOG is unset,
    so cfg.log_level IS being read, but the real bug is that if RUST_LOG=info
    is set anywhere in the environment, config is bypassed.
    
    WAIT — re-reading the symptom: "always uses default log level regardless of 
    what log_level is set to in config.toml". This means it behaves like "info" 
    always. If cfg.log_level IS being used (RUST_LOG absent), setting log_level="warn"
    should produce warn-level logging. So either:
    1. RUST_LOG IS set in the environment (overriding config), OR
    2. There is an additional bug

- timestamp: 2026-04-02T00:02:00Z
  checked: src/main.rs lines 88-105 — how env_filter is applied to subscribers
  found: |
    File logging branch (lines 89-99): env_filter is passed to .with(env_filter) on
    the registry. This applies EnvFilter as a global layer — correct.
    
    Stderr-only branch (lines 102-105): env_filter is passed to .with_env_filter(env_filter) — also correct.
    
    BUT in the file-logging branch (lines 89-99), the registry has:
      .with(env_filter)       ← global filter layer
      .with(fmt::Layer file)  ← file layer (no per-layer filter)
      .with(fmt::Layer stderr) ← stderr layer (no per-layer filter)
    
    This is the correct pattern — env_filter controls the global level.
  implication: The apply path is correct. Root cause is purely the precedence inversion.

- timestamp: 2026-04-02T00:03:00Z
  checked: The comment on line 66 vs actual logic
  found: Comment says "cfg.log_level > RUST_LOG env var > info fallback" but
         code evaluates RUST_LOG first (try_from_default_env), using cfg.log_level
         only in the unwrap_or_else fallback.
  implication: The intended behavior was documented correctly but implemented backwards.
               The fix is to check cfg.log_level first, fall back to RUST_LOG, then "info".

## Resolution

root_cause: |
  TOML scoping trap: log_level = "warn" was placed in config.toml AFTER the [[devices]]
  array-of-tables blocks. Per the TOML spec, once you open a [[array]] entry, all
  subsequent key-value pairs until the next header belong to that table entry — there is
  no way to "return" to root scope. So log_level ended up inside devices[0] as an unknown
  field that serde silently discards. AppConfig.log_level stayed None, causing the "info"
  default to always be used regardless of what log_level was set to.
  
  A secondary (also real) bug was also present: even if log_level had been parsed
  correctly, EnvFilter::try_from_default_env() (RUST_LOG) was evaluated before
  cfg.log_level, meaning any set RUST_LOG env var would silently override the config.
  This was also fixed.

fix: |
  1. config.toml: Moved log_level and log_file to the top of the file, before any
     [section] or [[array]] headers. Added prominent comments warning about the TOML
     ordering requirement.
  2. src/main.rs: Fixed EnvFilter precedence — cfg.log_level is now checked first
     (Some branch), with RUST_LOG as a fallback (None branch), then "info" as final
     default.
  3. src/config.rs: Fixed the test TOML string in test_log_level_parsed_as_warn to
     place log_level before [[devices]]. Added test_log_level_after_devices_is_not_parsed_as_root
     as a permanent regression guard documenting the TOML scoping trap.

verification: |
  cargo check: clean (0 errors, 0 warnings)
  cargo test: 21 passed, 0 failed, 3 ignored
  New tests confirm: log_level before [[devices]] → Some("warn") ✓
                     log_level after [[devices]] → None (documented trap) ✓
                     EnvFilter::try_new("warn") parses correctly ✓

files_changed:
  - config.toml
  - src/main.rs
  - src/config.rs
