# Phase 7: Daemon Reliability Hardening

## Goal

Fix all 14 findings from the daemon reliability verification report (`.planning/DAEMON-RELIABILITY-VERIFICATION.md`) so the rs485-logger runs reliably on a Raspberry Pi indefinitely without human intervention.

## Source

Verification report found 3 critical, 4 high, 5 medium, 2 low issues. Priority: eliminate all daemon-hang and unrecoverable-failure modes, then harden config validation, then improve operational hygiene.

## Findings to Address

| ID | Severity | Title | Plan |
|---|---|---|---|
| CRIT-01 | Critical | InfluxDB HTTP no timeout — daemon hangs | 07-01 |
| CRIT-03 | Critical | API token in git | 07-01 |
| HIGH-01 | High | No timeout on error response body read | 07-01 |
| HIGH-03 | High | Database name not URL-encoded | 07-01 |
| MED-01 | Medium | Log file grows unbounded | 07-01 |
| MED-02 | Medium | far_future() duration unnecessarily large | 07-01 |
| HIGH-02 | High | Device name not sanitized for line protocol | 07-02 |
| MED-05 | Medium | Energy reset config validated lazily | 07-02 |
| LOW-02 | Low | Epoch-0 timestamp if clock wrong at boot | 07-02 |
| CRIT-02 | Critical | Serial port failure unrecoverable | 07-03 |
| HIGH-04 | High | Modbus context stale data after timeout | 07-03 |
| MED-04 | Medium | No backoff on repeated InfluxDB failures | 07-03 |

### Not Addressed (acceptable as-is per report)

| ID | Severity | Reason |
|---|---|---|
| MED-03 | Medium | Timestamp i64 nanosecond multiplication — safe for ~270 years, no fix needed |
| LOW-01 | Low | Tokio features adequate — no fix needed |

## Depends On

Phase 6 (complete)

## Success Criteria

1. `cargo test` passes — all existing tests green plus new validation tests
2. `cargo clippy -- -D warnings` clean
3. InfluxDB client has connect + request timeouts (CRIT-01, HIGH-01 resolved)
4. `config.toml` in `.gitignore`, `config.toml.example` provided (CRIT-03)
5. Device names validated at config load — reject spaces/commas/newlines (HIGH-02)
6. Database name validated or URL-encoded at config load (HIGH-03)
7. All-device-failure counter causes process exit after N consecutive failures (CRIT-02)
8. Post-timeout delay between device polls prevents stale Modbus frames (HIGH-04)
9. Log rotation enabled via `rolling::daily` (MED-01)
10. InfluxDB failure state tracking — log first occurrence, suppress repeats (MED-04)
11. Energy reset timezone/time validated eagerly at config load (MED-05)
12. System clock warning when timestamp < 2024-01-01 (LOW-02)
13. `far_future()` reduced to 10 years (MED-02)
