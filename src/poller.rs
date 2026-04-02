// ModbusPoller — RTU client that opens the serial port once and polls PZEM-016 devices.
// Implemented in Plan 03-01.

use std::time::Duration;

use anyhow::Context;
use tokio_modbus::prelude::*;
use tokio_serial::SerialStream;

use crate::config::{DeviceConfig, SerialConfig};
use crate::types::{decode_registers, PowerReading};

/// Holds the Modbus RTU context for the shared RS485 bus.
///
/// The serial port is opened once at construction via [`ModbusPoller::new`].
/// The Modbus slave address is switched before each device read via `set_slave`,
/// so the single `Context` serves all devices on the daisy chain without
/// reopening the port.
pub struct ModbusPoller {
    ctx: client::Context,
}

impl ModbusPoller {
    /// Open the serial port and initialise the Modbus RTU context.
    ///
    /// # Errors
    /// Returns `Err` if the serial port cannot be opened (e.g. device missing,
    /// permission denied).
    pub fn new(serial: &SerialConfig) -> anyhow::Result<Self> {
        let builder = tokio_serial::new(&serial.port, serial.baud_rate);
        let port = SerialStream::open(&builder)
            .with_context(|| format!("Failed to open serial port '{}'", serial.port))?;
        let ctx = rtu::attach(port);
        Ok(ModbusPoller { ctx })
    }

    /// Switch to `device`'s Modbus slave address, issue FC 0x04 for 10 input
    /// registers starting at 0x0000 with a 500 ms timeout, decode them into a
    /// [`PowerReading`], and return it.
    ///
    /// # Errors
    /// Returns `Err` on timeout, Modbus transport error, Modbus exception
    /// response, or register decode failure.  Does **not** panic.
    pub async fn poll_device(
        &mut self,
        device: &DeviceConfig,
    ) -> anyhow::Result<PowerReading> {
        self.ctx.set_slave(Slave(device.address));

        // `read_input_registers` returns `tokio_modbus::Result<Vec<Word>>`
        // which is `Result<Result<Vec<Word>, ExceptionCode>, TransportError>`.
        // Three `.with_context()?` steps handle:
        //   1. timeout::elapsed (outer-outer — tokio timeout)
        //   2. TransportError    (outer Result — IO / protocol)
        //   3. ExceptionCode     (inner Result — Modbus exception from device)
        let regs = tokio::time::timeout(
            Duration::from_millis(500),
            self.ctx.read_input_registers(0x0000, 10),
        )
        .await
        .with_context(|| format!("Timeout polling device '{}'", device.name))? // timeout elapsed
        .with_context(|| format!("Modbus transport error from device '{}'", device.name))? // outer Err
        .with_context(|| format!("Modbus exception from device '{}'", device.name))?; // inner Err

        decode_registers(&regs, &device.name)
            .with_context(|| format!("Failed to decode registers from device '{}'", device.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DeviceConfig;

    /// Compile-time check: `DeviceConfig` can be constructed and passed to `poll_device`.
    /// Marked `#[ignore]` because running it without hardware would either fail to
    /// open the serial port or time out waiting for a Modbus response.
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
