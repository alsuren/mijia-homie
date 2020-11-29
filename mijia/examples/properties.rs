use chrono::{DateTime, Utc};
use eyre::Report;
use mijia::{MijiaSession, SensorProps};
use std::process::exit;
use std::time::Duration;
use tokio::time;

const SCAN_DURATION: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> Result<(), Report> {
    pretty_env_logger::init();

    // If at least one command-line argument is given, we will only try to connect to sensors whose
    // MAC address containts one of them as a sub-string.
    let mut args = std::env::args();
    let binary_name = args
        .next()
        .ok_or_else(|| eyre::eyre!("Binary name missing"))?;
    let filters: Vec<_> = args.collect();

    if filters
        .iter()
        .any(|f| f.contains(|c: char| !(c.is_ascii_hexdigit() || c == ':')))
    {
        eprintln!("Invalid MAC addresses {:?}", filters);
        eprintln!("Usage:");
        eprintln!("  {} [MAC address]...", binary_name);
        exit(1);
    }

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
            let history_range = session.get_history_range(&sensor.id).await?;
            let last_record = session.get_last_history_record(&sensor.id).await?;
            println!(
                "Time: {}, Unit: {}, Comfort level: {}, Range: {:?} Last value: {}",
                sensor_time, temperature_unit, comfort_level, history_range, last_record
            );
            let history = session.get_all_history(&sensor.id).await?;
            println!("History: {:?}", history);
        }
    }

    Ok(())
}

fn should_include_sensor(sensor: &SensorProps, filters: &Vec<String>) -> bool {
    let mac = sensor.mac_address.to_string();
    filters.is_empty() || filters.iter().any(|filter| mac.contains(filter))
}
