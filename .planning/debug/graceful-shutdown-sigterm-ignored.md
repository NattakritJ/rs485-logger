---
status: resolved
trigger: "Program ignores SIGTERM from systemd and gets SIGKILL after timeout"
created: 2026-04-08T00:00:00Z
updated: 2026-04-09T00:00:00Z
---

## Current Focus

hypothesis: CONFIRMED and VERIFIED on device
test: cargo check — passed clean; deployed to Pi and tested with `systemctl stop rs485-logger`
result: SIGTERM received within 4 seconds, clean exit, systemd reported "Deactivated successfully" with no timeout/SIGKILL
next_action: none — resolved

## Symptoms
<!-- Written during gathering, then IMMUTABLE -->

expected: When `systemctl stop rs485-logger` is run, the process should catch SIGTERM, finish current operations cleanly, and exit gracefully within a few seconds.
actual: Process completely ignores SIGTERM. It continues polling devices normally for the full systemd timeout period (~90 seconds), then gets force-killed by systemd with `State 'stop-sigterm' timed out. Killing.` and `status=9/KILL`.
errors: |
  Apr 08 21:37:58 ct-meter-load systemd[1]: Stopping rs485-logger.service - RS485 Logger — PZEM-016 power meter polling daemon...
  Apr 08 21:39:28 ct-meter-load systemd[1]: rs485-logger.service: State 'stop-sigterm' timed out. Killing.
  Apr 08 21:39:28 ct-meter-load systemd[1]: rs485-logger.service: Killing process 1602 (rs485-logger) with signal SIGKILL.
  Apr 08 21:39:28 ct-meter-load systemd[1]: rs485-logger.service: Killing process 1603 (tokio-rt-worker) with signal SIGKILL.
  Apr 08 21:39:28 ct-meter-load systemd[1]: rs485-logger.service: Main process exited, code=killed, status=9/KILL
  Apr 08 21:39:28 ct-meter-load systemd[1]: rs485-logger.service: Failed with result 'timeout'.
reproduction: Run the service and execute `systemctl stop rs485-logger`
started: Observed now; unclear if graceful shutdown was ever implemented

## Eliminated

- hypothesis: SIGTERM handler not installed (no tokio::signal::unix usage)
  evidence: src/main.rs lines 27-36 clearly installs SIGTERM handler via tokio::signal::unix::signal(SignalKind::terminate())
  timestamp: 2026-04-08T00:00:00Z

- hypothesis: shutdown future not polled (not included in select!)
  evidence: src/main.rs line 349 — `_ = &mut shutdown => { ... break; }` is present in the select! block
  timestamp: 2026-04-08T00:00:00Z

## Evidence

- timestamp: 2026-04-08T00:00:00Z
  checked: main.rs select! block structure (lines 236-353)
  found: select! uses `biased;` — arms are evaluated in declaration order: (1) reset_sleep, (2) ticker.tick(), (3) shutdown. shutdown arm is LAST.
  implication: With biased select!, when multiple arms are ready simultaneously, the FIRST ready arm is always chosen. If ticker.tick() fires before shutdown is re-polled, shutdown never gets selected.

- timestamp: 2026-04-08T00:00:00Z
  checked: tokio biased select! semantics (tokio docs)
  found: With `biased;`, tokio::select! checks arms in declaration order and picks the FIRST arm that is ready — it does NOT randomly select among ready arms. Critically, the signal future must be polled to register interest with the OS signal subsystem.
  implication: Once the poll loop is running and ticker keeps firing, the shutdown arm is the lowest-priority arm. If the ticker is immediately ready on each loop iteration (which it is — it uses MissedTickBehavior::Skip so it fires immediately each interval), then shutdown is never polled and the signal is never delivered to the Rust future.

- timestamp: 2026-04-08T00:00:00Z
  checked: ticker.set_missed_tick_behavior (main.rs line 184)
  found: MissedTickBehavior::Skip is set — ticks that would have fired during a blocked period are skipped. BUT the first tick fires immediately when the loop starts, and subsequent ticks fire at each interval.
  implication: Between poll cycles, when the select! restarts, both ticker AND shutdown may be ready. With biased ordering ticker wins every time.

- timestamp: 2026-04-08T00:00:00Z
  checked: tokio signal future registration
  found: The signal future (sig.recv()) must be polled at least once to register the waker with the OS signal handler. With biased select! always choosing ticker first, shutdown future is never polled on the first few iterations, and the SIGTERM signal registered by the OS may be missed entirely (signal arrives before the waker is registered).
  implication: This is a race condition in the worst case, and a starvation bug in the common case. The OS delivers SIGTERM to the process, but tokio's signal infrastructure never sees it because the future isn't polled.

## Resolution

root_cause: `biased;` select! with shutdown arm declared LAST causes SIGTERM starvation. With biased ordering, tokio checks arms in declaration order and selects the first ready one. The ticker arm (declared second) fires on every interval; the shutdown arm (declared third/last) is never polled when the ticker is ready. Additionally, on the very first iteration the signal future may not have been polled yet (waker not registered), so even a SIGTERM arriving after the loop starts can be lost. The fix is to move the shutdown arm to FIRST position in the biased select! — since shutdown fires at most once and is otherwise pending, it adds zero overhead while guaranteeing it is always polled before the ticker.
fix: Moved the shutdown arm to FIRST position in the biased select! block in main.rs (was last). Updated comments to reflect new priority order: shutdown > reset > tick. Reset arm remains second (before ticker) to preserve the midnight-collision fix.
verification: |
  `cargo check` passes clean (0 warnings, 0 errors).
  Deployed to Pi and verified 2026-04-09:
    03:26:31  SIGTERM received, initiating graceful shutdown
    03:26:31  Shutdown signal received, exiting cleanly
    03:26:31  rs485-logger stopped
    03:26:31  systemd: Deactivated successfully.
  Service exited ~4 seconds after `systemctl stop`, completing in-flight poll cycle then exiting cleanly.
  No timeout, no SIGKILL.
files_changed: [src/main.rs]
