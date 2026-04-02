mod config;
mod influx;
mod poller;
mod types;

use config::load_config;
use influx::InfluxWriter;
use poller::ModbusPoller;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let cfg = load_config("config.toml")?;

    let mut poller = ModbusPoller::new(&cfg.serial)?;
    let writer = InfluxWriter::new(&cfg.influxdb);

    let mut ticker = tokio::time::interval(
        std::time::Duration::from_secs(cfg.poll_interval_secs)
    );

    loop {
        ticker.tick().await;
        for device in &cfg.devices {
            match poller.poll_device(device).await {
                Ok(reading) => {
                    if let Err(e) = writer.write(&reading).await {
                        tracing::warn!(
                            device = %device.name,
                            error = %e,
                            "InfluxDB write failed"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        device = %device.name,
                        error = %e,
                        "Device poll failed, skipping"
                    );
                }
            }
        }
    }
}
