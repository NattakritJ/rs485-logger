// Scheduler — pure time-of-day scheduling utilities for daily energy reset.
// Implemented in Plan 06-01.

use anyhow::Context as _;
use chrono::{DateTime, NaiveTime, Utc};
use chrono_tz::Tz;
use std::time::Instant;

/// Compute the next future wall-clock `Instant` for the given time-of-day in the
/// given timezone.
///
/// If the computed time is already in the past (or is exactly NOW), always returns
/// the NEXT occurrence (i.e., advance by 1 day). This ensures a 01:00 startup
/// schedules for tomorrow 00:00, not today's already-passed midnight.
///
/// # Arguments
/// * `now`       — current UTC time (injectable for unit-testing)
/// * `tz_str`    — IANA timezone name, e.g. `"Asia/Bangkok"`
/// * `time_str`  — wall-clock time in `"HH:MM"` format, e.g. `"00:00"`
///
/// # Errors
/// Returns `Err` if the timezone string is unrecognised, the time string is
/// malformed, or the local time is ambiguous (DST gap).
pub fn next_reset_instant(
    now: DateTime<Utc>,
    tz_str: &str,
    time_str: &str,
) -> anyhow::Result<Instant> {
    let tz: Tz = tz_str
        .parse()
        .map_err(|_| anyhow::anyhow!("Unknown timezone: {tz_str}"))?;
    let reset_time = NaiveTime::parse_from_str(time_str, "%H:%M")
        .with_context(|| format!("Invalid time format '{time_str}', expected HH:MM"))?;

    let now_local = now.with_timezone(&tz);

    // Try today's occurrence first
    let candidate = now_local
        .date_naive()
        .and_time(reset_time)
        .and_local_timezone(tz)
        .single()
        .ok_or_else(|| anyhow::anyhow!("Ambiguous or invalid local time for {tz_str}"))?;

    let target = if candidate.with_timezone(&Utc) > now {
        // Today's occurrence is still in the future — use it
        candidate
    } else {
        // Already passed today — schedule tomorrow
        let tomorrow = now_local
            .date_naive()
            .succ_opt()
            .ok_or_else(|| anyhow::anyhow!("Date overflow computing next reset"))?;
        tomorrow
            .and_time(reset_time)
            .and_local_timezone(tz)
            .single()
            .ok_or_else(|| {
                anyhow::anyhow!("Ambiguous local time for tomorrow in {tz_str}")
            })?
    };

    let delay = (target.with_timezone(&Utc) - now)
        .to_std()
        .map_err(|_| anyhow::anyhow!("Negative duration computing next reset"))?;

    Ok(Instant::now() + delay)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;
    use std::time::Duration;

    /// Helper: given a `now` and the function result, compute the UTC DateTime the
    /// returned Instant corresponds to (within 1s tolerance).
    fn instant_to_approx_utc(
        now: DateTime<Utc>,
        result_instant: Instant,
        now_std: Instant,
    ) -> DateTime<Utc> {
        let delay = result_instant
            .duration_since(now_std)
            .max(Duration::ZERO);
        now + chrono::Duration::from_std(delay).unwrap()
    }

    // Snap an Instant captured just before calling next_reset_instant — this
    // lets us map the returned Instant back to an approximate UTC time.
    fn now_std() -> Instant {
        Instant::now()
    }

    /// Test 1: Given 15:00 Bangkok (= 08:00 UTC on Apr 2), next reset is midnight Bangkok
    /// = 00:00+07:00 on Apr 3 Bangkok = 2026-04-02T17:00:00Z (UTC midnight Apr 2 at 17:00).
    #[test]
    fn test_next_reset_midnight_bangkok() {
        let now: DateTime<Utc> = "2026-04-02T08:00:00Z".parse().unwrap();
        let std_now = now_std();
        let result = next_reset_instant(now, "Asia/Bangkok", "00:00").unwrap();

        // Midnight Apr 3 in Bangkok (UTC+7) = Apr 2 17:00:00 UTC
        let expected_utc: DateTime<Utc> = "2026-04-02T17:00:00Z".parse().unwrap();
        let approx_utc = instant_to_approx_utc(now, result, std_now);

        let diff = (approx_utc - expected_utc).num_seconds().abs();
        assert!(
            diff <= 1,
            "Expected ~{expected_utc}, got ~{approx_utc} (diff={diff}s)"
        );
    }

    /// Test 2: Given 01:00 Bangkok (= 18:00 UTC on Apr 2), midnight Apr 2 Bangkok
    /// already passed → next reset at 2026-04-03T17:00:00Z.
    #[test]
    fn test_next_reset_already_passed() {
        // 18:00 UTC = 01:00 Bangkok on Apr 3 (midnight already passed)
        let now: DateTime<Utc> = "2026-04-02T18:00:00Z".parse().unwrap();
        let std_now = now_std();
        let result = next_reset_instant(now, "Asia/Bangkok", "00:00").unwrap();

        // Bangkok midnight Apr 3 = 2026-04-02T17:00:00Z — but that's in the past
        // (now is 18:00 UTC). So next is Apr 4 midnight Bangkok = 2026-04-03T17:00:00Z.
        let expected_utc: DateTime<Utc> = "2026-04-03T17:00:00Z".parse().unwrap();
        let approx_utc = instant_to_approx_utc(now, result, std_now);

        let diff = (approx_utc - expected_utc).num_seconds().abs();
        assert!(
            diff <= 1,
            "Expected ~{expected_utc}, got ~{approx_utc} (diff={diff}s)"
        );
    }

    /// Test 3: UTC timezone, time "00:00", now=10:00 UTC → next reset at
    /// 2026-04-03T00:00:00Z (today's midnight already passed).
    #[test]
    fn test_next_reset_custom_timezone_utc() {
        let now: DateTime<Utc> = "2026-04-02T10:00:00Z".parse().unwrap();
        let std_now = now_std();
        let result = next_reset_instant(now, "UTC", "00:00").unwrap();

        let expected_utc: DateTime<Utc> = "2026-04-03T00:00:00Z".parse().unwrap();
        let approx_utc = instant_to_approx_utc(now, result, std_now);

        let diff = (approx_utc - expected_utc).num_seconds().abs();
        assert!(
            diff <= 1,
            "Expected ~{expected_utc}, got ~{approx_utc} (diff={diff}s)"
        );
    }

    /// Test 4: EnergyResetConfig deserializes from TOML [energy_reset] section.
    #[test]
    fn test_energy_reset_config_deserializes() {
        use crate::config::AppConfig;

        let toml_str = r#"
poll_interval_secs = 10

[serial]
port = "/dev/ttyUSB0"
baud_rate = 9600

[influxdb]
url = "http://localhost:8086"
token = "my-token"
database = "power"

[energy_reset]
enabled = true
timezone = "Asia/Bangkok"
time = "00:00"

[[devices]]
address = 1
name = "solar_panel"
"#;
        let cfg: AppConfig = toml::from_str(toml_str).unwrap();
        let er = cfg.energy_reset.expect("energy_reset should be Some");
        assert!(er.enabled);
        assert_eq!(er.timezone, "Asia/Bangkok");
        assert_eq!(er.time, "00:00");
    }

    /// Test 5: AppConfig without [energy_reset] section → energy_reset == None.
    #[test]
    fn test_energy_reset_absent_is_none() {
        use crate::config::AppConfig;

        let toml_str = r#"
poll_interval_secs = 10

[serial]
port = "/dev/ttyUSB0"
baud_rate = 9600

[influxdb]
url = "http://localhost:8086"
token = "my-token"
database = "power"

[[devices]]
address = 1
name = "solar_panel"
"#;
        let cfg: AppConfig = toml::from_str(toml_str).unwrap();
        assert!(
            cfg.energy_reset.is_none(),
            "energy_reset should be None when section absent"
        );
    }
}
