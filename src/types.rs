// PowerReading struct and register decode logic — implemented in Plan 03

#![allow(dead_code)]

use anyhow::anyhow;

#[derive(Debug, Clone)]
pub struct PowerReading {
    pub device_name: String,    // InfluxDB measurement name (from config)
    pub voltage: f64,           // V
    pub current: f64,           // A
    pub power: f64,             // W
    pub energy: f64,            // Wh
    pub frequency: f64,         // Hz
    pub power_factor: f64,      // 0.0–1.0
    pub timestamp_secs: i64,    // Unix epoch seconds (std::time::SystemTime)
}

/// Decode 10 raw PZEM-016 input registers into a PowerReading.
///
/// Register layout (FC 0x04, starting at 0x0000):
///   [0] voltage     16-bit  ÷ 10.0       → V
///   [1] current_lo  32-bit  low-word-first
///   [2] current_hi          (hi<<16|lo) ÷ 1000.0 → A
///   [3] power_lo    32-bit  low-word-first
///   [4] power_hi            (hi<<16|lo) ÷ 10.0  → W
///   [5] energy_lo   32-bit  low-word-first
///   [6] energy_hi           (hi<<16|lo) as f64  → Wh
///   [7] frequency   16-bit  ÷ 10.0       → Hz
///   [8] power_factor 16-bit ÷ 100.0      → dimensionless
///   [9] alarm        16-bit (ignored)
///
/// NOTE: 32-bit word order is LOW-WORD-FIRST (PZEM-016 deviates from Modbus standard).
/// D-08: MEDIUM confidence — verify against physical hardware in Phase 3.
pub fn decode_registers(regs: &[u16], device_name: &str) -> anyhow::Result<PowerReading> {
    if regs.len() < 10 {
        return Err(anyhow!("Expected 10 registers, got {}", regs.len()));
    }

    let voltage = regs[0] as f64 / 10.0;
    let current = ((regs[2] as u32) << 16 | regs[1] as u32) as f64 / 1000.0;
    let power = ((regs[4] as u32) << 16 | regs[3] as u32) as f64 / 10.0;
    let energy = ((regs[6] as u32) << 16 | regs[5] as u32) as f64;
    let frequency = regs[7] as f64 / 10.0;
    let power_factor = regs[8] as f64 / 100.0;

    let timestamp_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    Ok(PowerReading {
        device_name: device_name.to_string(),
        voltage,
        current,
        power,
        energy,
        frequency,
        power_factor,
        timestamp_secs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGS: [u16; 10] = [
        2301,  // [0] voltage: 2301 / 10.0 = 230.1 V
        1234,  // [1] current low word
        0,     // [2] current high word  → (0 << 16 | 1234) / 1000.0 = 1.234 A
        2852,  // [3] power low word
        0,     // [4] power high word    → (0 << 16 | 2852) / 10.0 = 285.2 W
        10240, // [5] energy low word
        0,     // [6] energy high word   → (0 << 16 | 10240) = 10240.0 Wh
        500,   // [7] frequency: 500 / 10.0 = 50.0 Hz
        95,    // [8] power factor: 95 / 100.0 = 0.95
        0,     // [9] alarm (ignored)
    ];

    #[test]
    fn test_basic_decode() {
        let reading = decode_registers(&REGS, "test_device").unwrap();
        assert!((reading.voltage - 230.1).abs() < 0.001, "voltage: {}", reading.voltage);
        assert!((reading.current - 1.234).abs() < 0.001, "current: {}", reading.current);
        assert!((reading.power - 285.2).abs() < 0.01, "power: {}", reading.power);
        assert_eq!(reading.energy, 10240.0, "energy: {}", reading.energy);
        assert!((reading.frequency - 50.0).abs() < 0.001, "frequency: {}", reading.frequency);
        assert!((reading.power_factor - 0.95).abs() < 0.001, "power_factor: {}", reading.power_factor);
        assert_eq!(reading.device_name, "test_device");
    }

    #[test]
    fn test_32bit_rollover() {
        // current_lo=0xFFFF, current_hi=0x0001 → (0x0001 << 16 | 0xFFFF) = 131071
        // 131071 / 1000.0 = 131.071 A
        let mut regs = REGS;
        regs[1] = 0xFFFF; // current low word
        regs[2] = 0x0001; // current high word
        let reading = decode_registers(&regs, "test_device").unwrap();
        assert!(
            (reading.current - 131.071).abs() < 0.001,
            "32-bit rollover current: {}",
            reading.current
        );
    }

    #[test]
    fn test_zero_values() {
        let regs: [u16; 10] = [0; 10];
        let reading = decode_registers(&regs, "zero_device").unwrap();
        assert_eq!(reading.voltage, 0.0);
        assert_eq!(reading.current, 0.0);
        assert_eq!(reading.power, 0.0);
        assert_eq!(reading.energy, 0.0);
        assert_eq!(reading.frequency, 0.0);
        assert_eq!(reading.power_factor, 0.0);
    }

    #[test]
    fn test_insufficient_registers_returns_err() {
        let short_regs = &REGS[..5];
        let result = decode_registers(short_regs, "x");
        assert!(result.is_err(), "Should return Err for fewer than 10 registers");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("5"),
            "Error should mention the actual register count, got: {}",
            msg
        );
    }
}
