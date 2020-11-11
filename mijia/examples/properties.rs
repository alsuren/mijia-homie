use mijia::MijiaSession;
use std::time::Duration;
use tokio::time;

const SCAN_DURATION: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> Result<(), eyre::Error> {
    pretty_env_logger::init();

    let (_, session) = MijiaSession::new().await?;

    // Start scanning for Bluetooth devices, and wait a while for some to be discovered.
    session.bt_session.start_discovery().await?;
    time::delay_for(SCAN_DURATION).await;

    // Get the list of sensors which are currently known, connect to them and print their properties.
    let sensors = session.get_sensors().await?;
    println!("Sensors:");
    for sensor in sensors {
        println!("Connecting to {} ({:?})", sensor.mac_address, sensor.id);
        if let Err(e) = session.bt_session.connect(&sensor.id).await {
            println!("Failed to connect to {}: {:?}", sensor.mac_address, e);
        } else {
            let sensor_time = session.get_time(&sensor.id).await?;
            let temperature_unit = session.get_temperature_unit(&sensor.id).await?;
            println!("Time: {:?}, Unit: {:?}", sensor_time, temperature_unit);
        }
    }

    Ok(())
}
