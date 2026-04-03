---
status: awaiting_human_verify
trigger: "Energy reset (--clear) works for first device but always times out on second device"
created: 2026-04-03T00:00:00Z
updated: 2026-04-03T00:00:10Z
---

## Current Focus

hypothesis: CONFIRMED — Missing inter-frame delay between sequential energy reset commands causes bus collision/timeout
test: Fix applied and compilation verified. Awaiting human verification on physical hardware.
expecting: Both devices should now succeed when running --clear with multiple devices configured
next_action: User verifies on physical Raspberry Pi with real PZEM-016 devices

## Symptoms

expected: Energy reset should succeed for ALL devices when running --clear with multiple devices configured
actual: First device always succeeds, second device always times out (~500ms). Swapping device order confirms the second device always fails regardless of which device it is.
errors: "Timeout resetting energy on device '<second_device_name>'"
reproduction:
  - Run with 2 devices: `./rs485-logger --config /etc/rs485-logger/config.toml --clear` -> first OK, second timeout
  - Run with only second device: `./rs485-logger --config config.toml --clear` -> succeeds
  - Run with reversed order: `./rs485-logger --config config_2.toml --clear` -> first OK, second timeout
started: Affects all multi-device energy clear operations. Normal polling works fine.

## Eliminated

## Evidence

- timestamp: 2026-04-03T00:00:00Z
  checked: Knowledge base for matching patterns
  found: No matching patterns (previous entry was about log_level config)
  implication: Novel issue, proceed with fresh investigation

- timestamp: 2026-04-03T00:00:01Z
  checked: Clear loop vs poll loop code structure in main.rs
  found: Clear loop (lines 136-143) iterates devices with NO delay between reset_energy calls. Poll loop (lines 253-276) has implicit delay because writer.write(&reading).await (HTTP POST to InfluxDB) executes between poll_device calls for successive devices. This HTTP roundtrip adds 5-50ms of natural inter-frame gap.
  implication: The clear loop sends the next Modbus command immediately after receiving the first device's response, potentially violating RS-485 bus turnaround and Modbus RTU inter-frame timing.

- timestamp: 2026-04-03T00:00:02Z
  checked: tokio-modbus 0.17.0 service/rtu.rs Client::call() method
  found: Line 65 clears read buffer before each request, Line 66 sends immediately. No built-in inter-frame delay in tokio-modbus. The library relies on the application to manage bus timing for sequential commands.
  implication: tokio-modbus does not add any Modbus RTU inter-frame gap (3.5 character times) between consecutive transactions. The application must provide this.

- timestamp: 2026-04-03T00:00:03Z
  checked: Response frame sizes — poll vs reset
  found: FC 0x04 poll response is 25 bytes (~29ms at 9600 baud). FC 0x42 reset response is only 4 bytes (~4.6ms at 9600 baud). The reset transaction completes much faster, leaving less natural gap before the next command.
  implication: The short reset response contributes to the problem — the entire reset transaction for device 1 completes in ~10ms total wire time (4-byte request + 4-byte response), so device 2's command arrives almost instantly after device 1's response.

- timestamp: 2026-04-03T00:00:04Z
  checked: tokio-modbus codec for custom function code (0x42) response framing
  found: get_response_pdu_len default branch uses buf.len()-3 as PDU length — works correctly when all bytes are buffered, but is a potential issue if partial data arrives. However, this equally affects both first and second device, so it's not the differential cause.
  implication: Frame decoding works, bus timing is the differential factor.

- timestamp: 2026-04-03T00:00:05Z
  checked: Daily energy reset loop in main.rs (lines 221-234)
  found: Same pattern as --clear mode — no inter-device delay. The daily scheduled reset would also fail for the second device.
  implication: Fix must be applied to BOTH code paths: --clear mode AND the daily reset select! arm.

- timestamp: 2026-04-03T00:00:06Z
  checked: Poll loop error path (device poll fails, no InfluxDB write)
  found: When poll_device fails for device N, the loop immediately moves to device N+1 with NO delay. The InfluxDB write (which provided the accidental delay) only runs on the success path.
  implication: Even the poll loop has a latent version of this bug on the error path. Fix should add explicit delay to all three loops.

- timestamp: 2026-04-03T00:00:07Z
  checked: Build and test after fix applied
  found: cargo check passes, cargo test passes (26 passed, 4 ignored for hardware tests)
  implication: Fix compiles and doesn't break existing functionality

## Resolution

root_cause: No inter-frame delay between sequential energy reset (FC 0x42) commands to different Modbus devices. The --clear loop and daily reset loop both iterate devices without any pause. Unlike the poll loop (which has an implicit delay from the InfluxDB HTTP write between successful device polls), the reset loops send the next command immediately after receiving the previous response. At 9600 baud, this violates the Modbus RTU inter-frame gap requirement (3.5 char times = ~4ms) and doesn't allow enough RS-485 bus turnaround time for the previous device's transceiver to release the line. The second device either doesn't see the command or sees a corrupted frame and doesn't respond, causing the 500ms timeout.
fix: Added a 100ms inter-frame delay (INTER_FRAME_DELAY constant in poller.rs, exposed via bus_delay() method) between ALL sequential Modbus transactions in all three device iteration loops — --clear mode, daily energy reset, and the polling loop. The polling loop delay also fixes a latent bug where poll failures (no InfluxDB write = no accidental delay) could cause the same issue.
verification: cargo check passes, cargo test passes (26/26 + 4 ignored). Hardware verification pending.
files_changed: [src/poller.rs, src/main.rs]
