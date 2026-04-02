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
    // Phase 06: optional daily energy reset
    pub energy_reset: Option<EnergyResetConfig>,
}

#[derive(Debug, serde::Deserialize)]
pub struct EnergyResetConfig {
    pub enabled: bool,
    pub timezone: String,   // IANA name, e.g. "Asia/Bangkok"
    pub time: String,       // "HH:MM" format, e.g. "00:00"
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
        cfg.influxdb.database.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-'),
        "influxdb.database '{}' contains invalid characters (use only alphanumeric, underscore, dash)",
        cfg.influxdb.database
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
        anyhow::ensure!(
            !device.name.is_empty(),
            "device at address {} has empty name",
            device.address
        );
        anyhow::ensure!(
            device.name.chars().all(|c| c.is_alphanumeric() || c == '_'),
            "device name '{}' contains invalid characters — use only alphanumeric and underscore (spaces, commas, newlines break InfluxDB line protocol)",
            device.name
        );
    }
    if let Some(ref er) = cfg.energy_reset {
        if er.enabled {
            er.timezone.parse::<chrono_tz::Tz>()
                .map_err(|_| anyhow::anyhow!("Unknown timezone '{}' in energy_reset", er.timezone))?;
            chrono::NaiveTime::parse_from_str(&er.time, "%H:%M")
                .with_context(|| format!("Invalid time '{}' in energy_reset — expected HH:MM", er.time))?;
        }
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
            energy_reset: None,
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

    // --- Database name validation (HIGH-03) ---

    fn make_cfg_with_database(database: &str) -> AppConfig {
        AppConfig {
            poll_interval_secs: 10,
            serial: SerialConfig {
                port: "/dev/ttyUSB0".to_string(),
                baud_rate: 9600,
            },
            influxdb: InfluxConfig {
                url: "http://localhost:8086".to_string(),
                token: "my-token".to_string(),
                database: database.to_string(),
            },
            devices: vec![DeviceConfig { address: 1, name: "meter".to_string() }],
            log_file: None,
            log_level: None,
            energy_reset: None,
        }
    }

    #[test]
    fn test_database_name_with_slash_rejected() {
        let cfg = make_cfg_with_database("power/test");
        let err = validate_config(&cfg).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("invalid characters"),
            "Error should mention 'invalid characters', got: {}",
            msg
        );
    }

    #[test]
    fn test_database_name_with_space_rejected() {
        let cfg = make_cfg_with_database("my database");
        let err = validate_config(&cfg).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("invalid characters"),
            "Error should mention 'invalid characters', got: {}",
            msg
        );
    }

    #[test]
    fn test_database_name_with_question_mark_rejected() {
        let cfg = make_cfg_with_database("power?db=x");
        let err = validate_config(&cfg).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("invalid characters"),
            "Error should mention 'invalid characters', got: {}",
            msg
        );
    }

    #[test]
    fn test_database_name_alphanumeric_underscore_dash_passes() {
        for name in &["power", "power_test", "my-db", "solar_2026", "my-db_01"] {
            let cfg = make_cfg_with_database(name);
            assert!(
                validate_config(&cfg).is_ok(),
                "Database name '{}' should pass validation",
                name
            );
        }
    }

    // --- T1: Device name validation (HIGH-02) ---

    fn make_cfg_with_device(address: u8, name: &str) -> AppConfig {
        AppConfig {
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
            devices: vec![DeviceConfig { address, name: name.to_string() }],
            log_file: None,
            log_level: None,
            energy_reset: None,
        }
    }

    #[test]
    fn test_device_name_with_space_rejected() {
        let cfg = make_cfg_with_device(1, "solar panel");
        let err = validate_config(&cfg).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("invalid characters"),
            "Error should mention 'invalid characters', got: {}",
            msg
        );
    }

    #[test]
    fn test_device_name_with_comma_rejected() {
        let cfg = make_cfg_with_device(1, "solar,panel");
        let err = validate_config(&cfg).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("invalid characters"),
            "Error should mention 'invalid characters', got: {}",
            msg
        );
    }

    #[test]
    fn test_device_name_with_newline_rejected() {
        let cfg = make_cfg_with_device(1, "solar\npanel");
        let err = validate_config(&cfg).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("invalid characters"),
            "Error should mention 'invalid characters', got: {}",
            msg
        );
    }

    #[test]
    fn test_device_name_empty_rejected() {
        let cfg = make_cfg_with_device(1, "");
        let err = validate_config(&cfg).unwrap_err();
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("empty"),
            "Error should mention 'empty', got: {}",
            msg
        );
    }

    #[test]
    fn test_device_name_valid_alphanumeric_and_underscore_passes() {
        let cfg = make_cfg_with_device(1, "solar_panel_01");
        assert!(
            validate_config(&cfg).is_ok(),
            "Alphanumeric + underscore device name should pass"
        );
    }

    // --- T2: energy_reset validation (MED-05) ---

    fn make_cfg_with_energy_reset(timezone: &str, time: &str, enabled: bool) -> AppConfig {
        AppConfig {
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
            devices: vec![DeviceConfig { address: 1, name: "meter".to_string() }],
            log_file: None,
            log_level: None,
            energy_reset: Some(EnergyResetConfig {
                enabled,
                timezone: timezone.to_string(),
                time: time.to_string(),
            }),
        }
    }

    #[test]
    fn test_invalid_timezone_rejected_when_enabled() {
        let cfg = make_cfg_with_energy_reset("Not/ATimezone", "00:00", true);
        let err = validate_config(&cfg).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("timezone") || msg.contains("Unknown"),
            "Error should mention 'timezone' or 'Unknown', got: {}",
            msg
        );
    }

    #[test]
    fn test_invalid_time_format_rejected_when_enabled() {
        let cfg = make_cfg_with_energy_reset("Asia/Bangkok", "25:99", true);
        let err = validate_config(&cfg).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("time") || msg.contains("Invalid"),
            "Error should mention 'time' or 'Invalid', got: {}",
            msg
        );
    }

    #[test]
    fn test_invalid_timezone_not_checked_when_disabled() {
        // When energy_reset.enabled = false, bad timezone/time should be silently ignored
        let cfg = make_cfg_with_energy_reset("Not/ATimezone", "99:99", false);
        assert!(
            validate_config(&cfg).is_ok(),
            "Disabled energy_reset should not validate timezone/time"
        );
    }

    #[test]
    fn test_valid_energy_reset_passes() {
        let cfg = make_cfg_with_energy_reset("Asia/Bangkok", "00:00", true);
        assert!(
            validate_config(&cfg).is_ok(),
            "Valid timezone and time should pass validation"
        );
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

