use mijia::MijiaSession;
use std::time::Duration;
use tokio::time;

const SCAN_DURATION: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    pretty_env_logger::init();

    let (_, session) = MijiaSession::new().await?;

    // Start scanning for Bluetooth devices, and wait a while for some to be discovered.
    session.bt_session.start_discovery().await?;
    time::sleep(SCAN_DURATION).await;

    // Get the list of sensors which are currently known and print them.
    let sensors = session.get_sensors().await?;
    println!("Sensors:");
    for sensor in sensors {
        println!("{}: {:?}", sensor.mac_address, sensor.id);
    }

    Ok(())
}
