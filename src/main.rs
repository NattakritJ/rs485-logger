mod config;
mod influx;
mod poller;
mod types;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    Ok(())
}
