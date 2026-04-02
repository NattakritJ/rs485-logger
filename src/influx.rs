use anyhow::Context;

use crate::config::InfluxConfig;
use crate::types::PowerReading;

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

pub struct InfluxWriter {
    client: reqwest::Client,
    url: String,      // full endpoint: "{base_url}/api/v3/write_lp"
    token: String,
    database: String,
}

impl InfluxWriter {
    pub fn new(config: &InfluxConfig) -> Self {
        let client = reqwest::Client::new();
        let url = format!("{}/api/v3/write_lp", config.url.trim_end_matches('/'));
        InfluxWriter {
            client,
            url,
            token: config.token.clone(),
            database: config.database.clone(),
        }
    }

    pub async fn write(&self, reading: &PowerReading) -> anyhow::Result<()> {
        let body = to_line_protocol(reading);
        let url = format!("{}?db={}&precision=ns", self.url, self.database);
        let response = self.client
            .post(&url)
            .bearer_auth(&self.token)
            .body(body)
            .send()
            .await
            .with_context(|| format!("Failed to connect to InfluxDB at {}", self.url))?;

        let status = response.status();
        if status == reqwest::StatusCode::NO_CONTENT {
            Ok(())
        } else {
            let body = response.text().await.unwrap_or_default();
            Err(anyhow::anyhow!(
                "InfluxDB write failed: HTTP {} — {}",
                status,
                body
            ))
        }
    }
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

    #[test]
    fn test_influx_writer_constructs() {
        let config = crate::config::InfluxConfig {
            url: "http://localhost:8086".to_string(),
            token: "test-token".to_string(),
            database: "power".to_string(),
        };
        let writer = InfluxWriter::new(&config);
        assert_eq!(writer.url, "http://localhost:8086/api/v3/write_lp");
        assert_eq!(writer.database, "power");
    }

    #[test]
    fn test_influx_writer_trims_trailing_slash() {
        let config = crate::config::InfluxConfig {
            url: "http://localhost:8086/".to_string(),
            token: "test-token".to_string(),
            database: "power".to_string(),
        };
        let writer = InfluxWriter::new(&config);
        assert_eq!(writer.url, "http://localhost:8086/api/v3/write_lp");
    }

    #[tokio::test]
    #[ignore = "requires local InfluxDB 3 instance on localhost:8086"]
    async fn test_influx_write_integration() {
        // Setup: requires InfluxDB 3 running locally with:
        //   docker run -p 8086:8086 influxdb:3-core (or equivalent)
        //   Database "power_test" created, or auto-created on write
        //   Token: set INFLUX_TOKEN env var (default "test-token" for local dev)
        let token = std::env::var("INFLUX_TOKEN").unwrap_or_else(|_| "test-token".to_string());
        let config = crate::config::InfluxConfig {
            url: "http://localhost:8086".to_string(),
            token,
            database: "power_test".to_string(),
        };
        let writer = InfluxWriter::new(&config);
        let reading = PowerReading {
            device_name: "integration_test_device".to_string(),
            voltage: 230.0,
            current: 1.0,
            power: 230.0,
            energy: 100.0,
            frequency: 50.0,
            power_factor: 1.0,
            timestamp_secs: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
        };
        let result = writer.write(&reading).await;
        assert!(result.is_ok(), "InfluxDB write failed: {:?}", result);
    }

    #[tokio::test]
    #[ignore = "requires local InfluxDB 3 instance on localhost:8086"]
    async fn test_influx_write_connection_refused_returns_err() {
        // Verifies STOR-04: write errors do not panic, return Err with context
        let config = crate::config::InfluxConfig {
            url: "http://localhost:19999".to_string(), // nothing on this port
            token: "test-token".to_string(),
            database: "power_test".to_string(),
        };
        let writer = InfluxWriter::new(&config);
        let reading = PowerReading {
            device_name: "test".to_string(),
            voltage: 0.0,
            current: 0.0,
            power: 0.0,
            energy: 0.0,
            frequency: 0.0,
            power_factor: 0.0,
            timestamp_secs: 1700000000,
        };
        let result = writer.write(&reading).await;
        assert!(result.is_err(), "Connection refused should return Err, not panic");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("localhost:19999") || msg.contains("connect"),
            "Error should contain connection info: {}",
            msg
        );
    }
}
