// ModbusPoller — RTU client that opens the serial port once and polls PZEM-016 devices.
// Implemented in Plan 03-01 (GREEN phase replaces stubs).

#![allow(dead_code)]

use crate::config::{DeviceConfig, SerialConfig};
use crate::types::PowerReading;

/// Holds the Modbus RTU context for the shared RS485 bus.
/// Opened once at construction; slave address is switched per `poll_device()` call.
pub struct ModbusPoller {
    _placeholder: (),
}

impl ModbusPoller {
    /// Open the serial port and initialise the Modbus RTU context.
    pub fn new(_serial: &SerialConfig) -> anyhow::Result<ModbusPoller> {
        unimplemented!("ModbusPoller::new — stub for RED phase")
    }

    /// Switch slave address, issue FC 0x04 for 10 input registers starting at 0x0000,
    /// decode into a `PowerReading`, and return it.
    pub async fn poll_device(
        &mut self,
        _device: &DeviceConfig,
    ) -> anyhow::Result<PowerReading> {
        unimplemented!("ModbusPoller::poll_device — stub for RED phase")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DeviceConfig;

    /// Compile-time check: `DeviceConfig` can be constructed and passed to `poll_device`.
    /// Marked `#[ignore]` because running it without hardware would either panic (stub)
    /// or require an actual RS485 serial port.
    #[tokio::test]
    #[ignore = "requires physical RS485 hardware"]
    async fn test_poll_device_signature_compiles() {
        let serial = SerialConfig {
            port: "/dev/ttyUSB0".to_string(),
            baud_rate: 9600,
        };
        let device = DeviceConfig {
            address: 1,
            name: "test_device".to_string(),
        };
        let mut poller = ModbusPoller::new(&serial).unwrap();
        let _reading = poller.poll_device(&device).await.unwrap();
    }
}
