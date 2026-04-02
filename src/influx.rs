use crate::types::PowerReading;

#[allow(dead_code)]
/// Convert a PowerReading to InfluxDB 3 line protocol.
///
/// Format: `measurement field=value,... timestamp_ns`
/// - No tags (device name is the measurement name per STOR-01)
/// - All numeric values formatted as f64 floats (STOR-03: prevents type lock-in)
/// - Timestamp in nanoseconds (timestamp_secs * 1_000_000_000)
pub fn to_line_protocol(reading: &PowerReading) -> String {
    let ts_ns = reading.timestamp_secs * 1_000_000_000_i64;
    format!(
        "{} voltage={:.4},current={:.4},power={:.4},energy={:.4},frequency={:.4},power_factor={:.4} {}",
        reading.device_name,
        reading.voltage,
        reading.current,
        reading.power,
        reading.energy,
        reading.frequency,
        reading.power_factor,
        ts_ns,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_reading(device_name: &str, power: f64) -> PowerReading {
        PowerReading {
            device_name: device_name.to_string(),
            voltage: 230.1,
            current: 1.234,
            power,
            energy: 10240.0,
            frequency: 50.0,
            power_factor: 0.95,
            timestamp_secs: 1700000000,
        }
    }

    #[test]
    fn test_basic_line_protocol() {
        let r = make_reading("solar_panel", 285.2);
        let lp = to_line_protocol(&r);
        assert!(lp.starts_with("solar_panel "), "measurement: {}", lp);
        assert!(lp.contains("voltage="), "voltage field: {}", lp);
        assert!(lp.contains("current="), "current field: {}", lp);
        assert!(lp.contains("power="), "power field: {}", lp);
        assert!(lp.contains("energy="), "energy field: {}", lp);
        assert!(lp.contains("frequency="), "frequency field: {}", lp);
        assert!(lp.contains("power_factor="), "power_factor field: {}", lp);
    }

    #[test]
    fn test_zero_power_is_float() {
        // STOR-03: zero must write as float — InfluxDB 3 locks field type on first write
        let r = make_reading("zero_meter", 0.0);
        let lp = to_line_protocol(&r);
        // Must contain "power=0." or "power=0.0" — never bare "power=0 " or "power=0,"
        assert!(
            lp.contains("power=0."),
            "power=0.0 must be formatted as float, got: {}",
            lp
        );
    }

    #[test]
    fn test_timestamp_is_nanoseconds() {
        let r = make_reading("ts_test", 100.0);
        let lp = to_line_protocol(&r);
        assert!(
            lp.ends_with("1700000000000000000"),
            "Timestamp must be epoch nanoseconds, got: {}",
            lp
        );
    }

    #[test]
    fn test_device_name_verbatim() {
        let r = make_reading("grid_meter", 50.0);
        let lp = to_line_protocol(&r);
        assert!(
            lp.starts_with("grid_meter "),
            "Measurement name must be device_name verbatim, got: {}",
            lp
        );
    }
}
