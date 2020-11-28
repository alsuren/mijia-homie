use chrono::{DateTime, Utc};
use mijia::{MijiaSession, SensorProps};
use std::time::Duration;
use tokio::time;

const SCAN_DURATION: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> Result<(), eyre::Error> {
    pretty_env_logger::init();

    // If at least one command-line argument is given, we will only try to connect to sensors whose
    // MAC address containts one of them as a sub-string.
    let filters: Vec<_> = std::env::args().collect();

    let (_, session) = MijiaSession::new().await?;

    // Start scanning for Bluetooth devices, and wait a while for some to be discovered.
    session.bt_session.start_discovery().await?;
    time::delay_for(SCAN_DURATION).await;

    // Get the list of sensors which are currently known, connect to them and print their properties.
    let sensors = session.get_sensors().await?;
    println!("Sensors:");
    for sensor in sensors {
        if !should_include_sensor(&sensor, &filters) {
            println!("Skipping {}", sensor.mac_address);
            continue;
        }
        println!("Connecting to {} ({:?})", sensor.mac_address, sensor.id);
        if let Err(e) = session.bt_session.connect(&sensor.id).await {
            println!("Failed to connect to {}: {:?}", sensor.mac_address, e);
        } else {
            let sensor_time: DateTime<Utc> = session.get_time(&sensor.id).await?.into();
            let temperature_unit = session.get_temperature_unit(&sensor.id).await?;
            let comfort_level = session.get_comfort_level(&sensor.id).await?;
            println!(
                "Time: {}, Unit: {}, Comfort level: {}",
                sensor_time, temperature_unit, comfort_level
            );
        }
    }

    Ok(())
}

fn should_include_sensor(sensor: &SensorProps, filters: &Vec<String>) -> bool {
    let mac = sensor.mac_address.to_string();
    filters.is_empty() || filters.iter().any(|filter| mac.contains(filter))
}
