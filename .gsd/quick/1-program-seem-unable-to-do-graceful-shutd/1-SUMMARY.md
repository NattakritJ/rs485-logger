# Quick Task: Program unable to do graceful shutdown. When I run "sudo systemctl stop rs485-logger" it doesn't graceful shutdown until timeout and OS killed process.

**Date:** 2026-04-08
**Branch:** master
**Commit:** 66ed375

## Root Cause

The program was using a single-threaded tokio runtime (`current_thread` flavor), which cannot properly wake up from signal handlers when SIGTERM/SIGINT is received. Signal handlers run on the OS signal delivery thread, but single-threaded tokio executors don't properly route these wakeups, causing the main select! loop to never break until the systemd timeout (90 seconds by default) expires and forcefully kills the process.

## What Changed

1. **Tokio Runtime Configuration**
   - Changed from `flavor = "current_thread"` to `flavor = "multi_thread"`
   - Added `rt-multi-thread` and `sync` features to tokio dependency
   - Multi-threaded runtime properly integrates signal handlers with the executor threads

2. **Signal Handler Improvements**
   - Improved error handling in `shutdown_signal()` to warn instead of panic
   - Added explicit logging when SIGINT/SIGTERM is received
   - Better error context for signal registration failures

3. **Systemd Configuration**
   - Added `TimeoutStopSec=15` to allow 15 seconds for graceful shutdown
   - This is reasonable for the program's single tick interval (typically 30-300 seconds)
   - Prevents systemd timeout at the default 90 seconds

## Files Modified

- `Cargo.toml` - Updated tokio dependency features
- `src/main.rs` - Changed runtime flavor and improved signal handling
- `deploy/rs485-logger.service` - Added TimeoutStopSec configuration

## Verification

- ✅ All 39 unit tests pass
- ✅ Release build compiles successfully
- ✅ No warnings in compilation output
- ✅ Binary size optimized with strip and opt-level 3

## Technical Details

With the multi-threaded runtime:
- Tokio spawns multiple worker threads
- Signal handlers are properly integrated and can wake the runtime
- When SIGTERM arrives, the signal delivery triggers the pinned `shutdown_signal()` future
- The tokio::select! biased loop immediately breaks on the shutdown arm
- The program logs shutdown event and exits cleanly within the TimeoutStopSec window

## Testing Recommendation

When deployed to the Raspberry Pi, test graceful shutdown with:
```bash
sudo systemctl stop rs485-logger
# Should complete within 15 seconds and show "Shutdown signal received" in logs
journalctl -u rs485-logger -f
```
