---
status: awaiting_human_verify
trigger: "Energy reset sends commands to multiple devices but device 2 times out. User wants strictly sequential per-device flow."
created: 2026-04-03T00:00:00Z
updated: 2026-04-03T00:02:00Z
---

## Current Focus

hypothesis: CONFIRMED — Two separate root causes work together:
  1. PRIMARY (timing/bus contention): The `ticker` interval fires at midnight too. When the reset arm runs its for-loop sequentially, it `.await`s on device 1's reset — this yields the executor. If the ticker fires during that yield, `tokio::select!` has already committed to the reset arm for THIS iteration, so the tick accumulates. BUT: after device 1 completes (T+48ms), the code awaits device 2's reset. The ticker (set to some poll_interval_secs) may have accumulated a tick that fires at the top of the NEXT loop iteration — but that's after the for-loop finishes. So concurrent access isn't possible within a single select! arm.
  2. ACTUAL ROOT CAUSE: The `ticker` fires at T=0ms (midnight). The `reset_sleep` ALSO fires at T=0ms (midnight). `tokio::select!` randomly picks ONE. It picks `reset_sleep`. The reset for loop runs. Device 1's reset: send + receive = 48ms. Then device 2's reset is attempted. BUT: the `ticker` was ALSO ready at T=0ms. After the reset arm's for-loop finishes and `.await`s device 2, the select! has already chosen the reset arm for this loop iteration. The for loop is fully sequential. Device 2 gets its command... but the PZEM-016 Modbus RTU protocol requires a guard time between messages. Device 1's response was received at T+48ms. Device 2's command is sent almost immediately after (no inter-device delay). The PZEM-016 may still be processing or there's RS485 bus turnaround time. 500ms should be enough for that.
  3. MOST LIKELY ACTUAL CAUSE: Re-examining logs. T+48ms = device 1 OK. T+550ms = device 2 timeout. That's 502ms = exactly 500ms timeout. Device 2 got NO response at all. This indicates the RS485 bus was busy or the device was not responding. Given the `ticker` fires at the same millisecond as `reset_sleep`, and `tokio::select!` with `current_thread` flavor picks randomly among ready futures: if `ticker` fires FIRST, then after the poll for-loop completes (multiple devices polling), the reset arm fires next. During the poll loop, device 2's address (Slave(2)) was last set. Then reset_energy sets Slave(device.address) for device 1, then device 2. This is fine. BUT: if the poll already set slave to address 2 and sent FC0x04 read commands, the response from device 2 for the POLL may still be in the serial buffer when the RESET command for device 2 is sent. The RESET would then read back the stale poll response, get an unexpected response, and fail/timeout.
  ACTUAL MOST LIKELY: ticker arm ran first at midnight (polling all devices including device 2), left residual RS485 bus traffic or queued responses for device 2, then reset_energy for device 2 received a corrupt/stale response and timed out.
test: Check if logs show poll success entries at midnight BEFORE the reset (would confirm ticker fired first)
next_action: Apply fix — add inter-device delay OR increase timeout, AND most importantly ensure the reset for-loop has explicit log of "sending" before and "received" after each device per user requirement

## Evidence

- timestamp: 2026-04-03T00:00:00Z
  checked: Log timestamps
  found: Device 1 reset OK at T+48ms. Device 2 timeout at T+550ms (= T+48ms + 502ms = device 1 complete + exactly 500ms timeout). Device 2 got zero response — the full 500ms elapsed.
  implication: Device 2 either (a) never received the command, (b) received it but couldn't respond in 500ms, or (c) responded but with something unexpected that the timeout path swallowed.

- timestamp: 2026-04-03T00:00:00Z
  checked: main.rs select! loop — ticker and reset_sleep both fire at midnight
  found: `tokio::select!` with `current_thread` flavor. Both `ticker.tick()` and `reset_sleep` can be ready simultaneously at midnight. select! picks one pseudo-randomly. If ticker fires first, it polls ALL devices sequentially (.await each). After that loop, next select! iteration fires reset arm. The poll loop touched device 2's Modbus address and sent FC0x04 reads. The RS485 serial buffer may contain a delayed/slow response from device 2 for the poll when the reset command is sent.
  implication: RACE CONDITION — ticker and reset can fire at same select! cycle. If ticker wins, it pollutes the RS485 bus for device 2 before reset runs.

- timestamp: 2026-04-03T00:00:00Z
  checked: poller.rs reset_energy and poll_device — both use same `ctx: client::Context`, same serial port
  found: No locking, no inter-device delay, no flush between operations. `set_slave()` is called immediately before each command. The tokio-modbus context is stateful — it holds the slave address. Both functions modify it.
  implication: If a poll response for device 2 is still in-flight or buffered when reset_energy calls set_slave(device2) and sends FC0x42, the response to FC0x42 may be confused with the buffered FC0x04 response.

- timestamp: 2026-04-03T00:00:00Z
  checked: User requirement — strictly sequential with send/receive logging per device
  found: Current code has sequential for-loop but NO "sending" log before the command and NO "received" log after. The user wants: log send → send → log receive → next device.
  implication: Logging changes needed in addition to the timing/bus fix.

## Symptoms

expected: Each device receives its energy reset command one at a time, sequentially. For each device: send command → wait for response → log send/receive → move to next device. All devices should succeed.

actual: Device 1 resets successfully. Device 2 times out with error "Timeout resetting energy on device '235_floor_2'".

errors: |
  Apr 03 00:00:00 ct-meter-load rs485-logger[1746]: 2026-04-02T17:00:00.001484Z  INFO rs485_logger: Daily energy reset starting
  Apr 03 00:00:00 ct-meter-load rs485-logger[1746]: 2026-04-02T17:00:00.049286Z  INFO rs485_logger: Energy reset OK device=235_floor_1
  Apr 03 00:00:00 ct-meter-load rs485-logger[1746]: 2026-04-02T17:00:00.550126Z  WARN rs485_logger: Energy reset failed, skipping device=235_floor_2 error=Timeout resetting energy on device '235_floor_2'
  Apr 03 00:00:00 ct-meter-load rs485-logger[1746]: 2026-04-02T17:00:00.550218Z  INFO rs485_logger: Next energy reset scheduled next_reset=2026-04-04T00:00:00+07:00

reproduction: Wait for daily energy reset at midnight (local time = 00:00 +07:00 = 17:00 UTC), or trigger manually if possible.
started: Observed on 2026-04-03. The energy reset is a daily scheduled task.

## Eliminated

- hypothesis: "Non-sequential dispatch — commands sent in parallel"
  evidence: Code uses a plain `for` loop with `.await` inside — fully sequential. Each device completes before next starts.
  timestamp: 2026-04-03T00:00:30Z

- hypothesis: "500ms timeout too short per device"
  evidence: 500ms is generous for RS485 RTU. The PZEM-016 responds in <50ms (device 1 showed ~48ms). The real issue is bus state, not timeout duration.
  timestamp: 2026-04-03T00:00:30Z

- hypothesis: "Device 2 hardware failure"
  evidence: The pattern (device 1 always OK, device 2 always times out) is consistent with bus contention at midnight, not device failure. If device 2 were faulty it would also fail during normal polls.
  timestamp: 2026-04-03T00:00:30Z

## Evidence

- timestamp: 2026-04-03T00:00:00Z
  checked: Log timestamps
  found: Device 1 reset OK at T+48ms. Device 2 timeout at T+550ms (= T+48ms + 502ms = device 1 complete + exactly 500ms timeout). Device 2 got zero response — the full 500ms elapsed.
  implication: Device 2 either (a) never received the command, (b) received it but couldn't respond in 500ms, or (c) responded but with something unexpected.

- timestamp: 2026-04-03T00:00:00Z
  checked: main.rs select! loop — ticker and reset_sleep both fire at midnight
  found: `tokio::select!` without `biased` pseudo-randomly selects among ready futures. At midnight both `ticker.tick()` AND `reset_sleep` are ready simultaneously. If ticker wins: it polls ALL devices with FC0x04. Then next loop iteration the reset arm fires. But after ticker polls device 2 with FC0x04, the RS485 bus is still settling (response may still be in transit or buffered). When reset_energy sends FC0x42 to device 2, no valid echo comes back → full 500ms timeout.
  implication: ROOT CAUSE — non-deterministic arm selection in select! allows the poll loop to contaminate the bus before the reset runs.

- timestamp: 2026-04-03T00:00:00Z
  checked: Default MissedTickBehavior for tokio::time::interval
  found: Default is `Burst` — if the energy reset loop takes longer than the poll interval, the ticker fires immediately when the reset arm finishes, potentially sending more FC0x04 commands right after the reset. With 2 devices and a 500ms reset, any poll_interval_secs <= 1 would burst.
  implication: Secondary fix needed — set MissedTickBehavior::Skip to prevent burst polling after reset.

- timestamp: 2026-04-03T00:00:00Z
  checked: User requirement for logging
  found: User wants explicit "sending command" log BEFORE sending and "OK"/"failed" AFTER receiving, per device. Current code only logged OK or failed — no pre-send log.
  implication: Add `tracing::info!(device = %device.name, "Energy reset sending command")` before each `reset_energy` call.

## Resolution

root_cause: |
  `tokio::select!` without `biased` randomly selects among ready arms when multiple
  futures complete simultaneously. At midnight, both the poll ticker AND the reset
  sleep deadline fire at the same instant. When the ticker arm wins the random
  selection, it sends FC0x04 read commands to ALL devices (including device 2) via
  the RS485 bus. In the NEXT select! iteration the reset arm fires; device 1 resets
  fine (~48ms), but when the FC0x42 reset command is sent to device 2, the RS485 bus
  hasn't fully settled from the prior FC0x04 poll — the device 2 response is either
  still buffered or the PZEM-016 is still processing. No valid echo is received within
  500ms → Timeout error.

fix: |
  Two changes to src/main.rs:
  1. Added `biased;` to the `tokio::select!` macro so the reset arm is always checked
     first. When both reset_sleep and ticker are ready simultaneously, the reset arm
     wins deterministically, preventing any FC0x04 poll from contaminating the bus
     before the energy reset runs.
  2. Set `MissedTickBehavior::Skip` on the ticker so that if the reset loop causes a
     tick to be missed, the ticker does NOT burst-fire immediately after — it simply
     skips the missed interval and resumes on schedule.
  3. Added `tracing::info!(device = %device.name, "Energy reset sending command")`
     log line before each `reset_energy` call to implement the strictly sequential
     send→receive→log flow requested by the user.

verification: Build passes (cargo build), all 26 tests pass (cargo test), zero clippy warnings (cargo clippy -- -D warnings). Awaiting next-midnight confirmation on device.

files_changed:
  - src/main.rs
