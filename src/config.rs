// Config structs and TOML parsing — implemented in Plan 02

use anyhow::Context;

#[derive(Debug, serde::Deserialize)]
pub struct AppConfig {
    pub poll_interval_secs: u64,
    pub serial: SerialConfig,
    pub influxdb: InfluxConfig,
    pub devices: Vec<DeviceConfig>,
    // OPS-03: optional logging config
    pub log_file: Option<String>,   // e.g. "/var/log/rs485-logger/rs485.log"
    pub log_level: Option<String>,  // e.g. "debug", "info", "warn" — default "info"
}

#[derive(Debug, serde::Deserialize)]
pub struct SerialConfig {
    pub port: String,
    pub baud_rate: u32,
}

#[derive(Debug, serde::Deserialize)]
pub struct InfluxConfig {
    pub url: String,
    pub token: String,
    pub database: String,
}

#[derive(Debug, serde::Deserialize, Clone)]
pub struct DeviceConfig {
    pub address: u8,
    pub name: String,
}

pub fn load_config(path: &str) -> anyhow::Result<AppConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path))?;
    let cfg: AppConfig = toml::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {}", path))?;
    validate_config(&cfg)?;
    Ok(cfg)
}

pub fn validate_config(cfg: &AppConfig) -> anyhow::Result<()> {
    anyhow::ensure!(
        cfg.poll_interval_secs > 0,
        "poll_interval_secs must be > 0"
    );
    anyhow::ensure!(
        !cfg.influxdb.url.is_empty(),
        "influxdb.url is empty"
    );
    anyhow::ensure!(
        !cfg.influxdb.token.is_empty(),
        "influxdb.token is empty — set a Bearer token"
    );
    anyhow::ensure!(
        !cfg.devices.is_empty(),
        "device list is empty — add at least one [[devices]] entry"
    );
    for device in &cfg.devices {
        anyhow::ensure!(
            device.address >= 1 && device.address <= 247,
            "device '{}' has invalid Modbus address {} (must be 1–247)",
            device.name,
            device.address
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_CONFIG: &str = r#"
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

[[devices]]
address = 2
name = "grid_meter"
"#;

    #[test]
    fn test_happy_path_deserializes_correctly() {
        let cfg: AppConfig = toml::from_str(VALID_CONFIG).unwrap();
        assert_eq!(cfg.poll_interval_secs, 10);
        assert_eq!(cfg.serial.port, "/dev/ttyUSB0");
        assert_eq!(cfg.serial.baud_rate, 9600);
        assert_eq!(cfg.influxdb.url, "http://localhost:8086");
        assert_eq!(cfg.influxdb.token, "my-token");
        assert_eq!(cfg.influxdb.database, "power");
        assert_eq!(cfg.devices.len(), 2);
        assert_eq!(cfg.devices[0].address, 1);
        assert_eq!(cfg.devices[0].name, "solar_panel");
        assert_eq!(cfg.devices[1].address, 2);
        assert_eq!(cfg.devices[1].name, "grid_meter");
        assert!(validate_config(&cfg).is_ok());
    }

    #[test]
    fn test_empty_device_list_rejected() {
        // Note: In TOML, inline arrays (devices = []) at root level must appear
        // before any [section] headers. We build AppConfig directly to test
        // validate_config with an empty device list.
        let cfg = AppConfig {
            poll_interval_secs: 10,
            serial: SerialConfig {
                port: "/dev/ttyUSB0".to_string(),
                baud_rate: 9600,
            },
            influxdb: InfluxConfig {
                url: "http://localhost:8086".to_string(),
                token: "my-token".to_string(),
                database: "power".to_string(),
            },
            devices: vec![],
            log_file: None,
            log_level: None,
        };
        let err = validate_config(&cfg).unwrap_err();
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("device") || msg.contains("empty"),
            "Error should mention 'device' or 'empty', got: {}",
            err
        );
    }

    #[test]
    fn test_invalid_address_zero_rejected() {
        let config_str = r#"
poll_interval_secs = 10

[serial]
port = "/dev/ttyUSB0"
baud_rate = 9600

[influxdb]
url = "http://localhost:8086"
token = "my-token"
database = "power"

[[devices]]
address = 0
name = "bad_device"
"#;
        let cfg: AppConfig = toml::from_str(config_str).unwrap();
        let err = validate_config(&cfg).unwrap_err();
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("address"),
            "Error should mention 'address', got: {}",
            err
        );
    }

    #[test]
    fn test_invalid_address_248_rejected() {
        let config_str = r#"
poll_interval_secs = 10

[serial]
port = "/dev/ttyUSB0"
baud_rate = 9600

[influxdb]
url = "http://localhost:8086"
token = "my-token"
database = "power"

[[devices]]
address = 248
name = "bad_device"
"#;
        let cfg: AppConfig = toml::from_str(config_str).unwrap();
        let err = validate_config(&cfg).unwrap_err();
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("address"),
            "Error should mention 'address', got: {}",
            err
        );
    }

    #[test]
    fn test_empty_token_rejected() {
        let config_str = r#"
poll_interval_secs = 10

[serial]
port = "/dev/ttyUSB0"
baud_rate = 9600

[influxdb]
url = "http://localhost:8086"
token = ""
database = "power"

[[devices]]
address = 1
name = "solar_panel"
"#;
        let cfg: AppConfig = toml::from_str(config_str).unwrap();
        let err = validate_config(&cfg).unwrap_err();
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("token"),
            "Error should mention 'token', got: {}",
            err
        );
    }

    #[test]
    fn test_poll_interval_zero_rejected() {
        let config_str = r#"
poll_interval_secs = 0

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
        let cfg: AppConfig = toml::from_str(config_str).unwrap();
        let err = validate_config(&cfg).unwrap_err();
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("poll_interval"),
            "Error should mention 'poll_interval', got: {}",
            err
        );
    }

    #[test]
    fn test_load_config_file_not_found() {
        let result = load_config("nonexistent.toml");
        assert!(result.is_err(), "load_config should return Err for missing file");
    }
}

#[cfg(test)]
mod log_level_tests {
    use super::*;

    #[test]
    fn test_log_level_parsed_as_warn() {
        // IMPORTANT: log_level must appear BEFORE [[devices]] blocks.
        // In TOML, keys after a [[array-of-tables]] header belong to that
        // array entry, not the root — placing log_level after [[devices]]
        // silently puts it inside the device table and leaves AppConfig.log_level = None.
        let cfg_str = r#"
poll_interval_secs = 10
log_level = "warn"

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
        let cfg: AppConfig = toml::from_str(cfg_str).unwrap();
        assert_eq!(cfg.log_level, Some("warn".to_string()),
            "log_level should be Some(\"warn\") but got {:?}", cfg.log_level);
    }

    #[test]
    fn test_log_level_absent_is_none() {
        let cfg_str = r#"
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
        let cfg: AppConfig = toml::from_str(cfg_str).unwrap();
        assert_eq!(cfg.log_level, None,
            "log_level should be None when absent but got {:?}", cfg.log_level);
    }

    #[test]
    fn test_log_level_after_devices_is_not_parsed_as_root() {
        // Regression guard: log_level placed AFTER [[devices]] in TOML is silently
        // swallowed into the device table. It must appear BEFORE [[devices]] to reach
        // AppConfig.log_level. This test documents the TOML scoping trap.
        let cfg_str = r#"
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

log_level = "warn"
"#;
        // log_level ends up inside devices[0], not at root — AppConfig.log_level stays None
        let cfg: AppConfig = toml::from_str(cfg_str).unwrap();
        assert_eq!(
            cfg.log_level, None,
            "log_level after [[devices]] must NOT reach AppConfig.log_level (TOML scoping)"
        );
    }

    #[test]
    fn test_env_filter_from_warn_string() {
        // Verify EnvFilter::try_new("warn") parses without error
        let filter = tracing_subscriber::EnvFilter::try_new("warn")
            .expect("'warn' should be a valid filter string");
        let filter_str = format!("{}", filter);
        assert!(
            filter_str.contains("warn"),
            "EnvFilter display should contain 'warn', got: {}", filter_str
        );
    }
}

