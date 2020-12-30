//! Example of how to subscribe to readings from one or more sensors.

use eyre::Report;
use mijia::{MijiaEvent, MijiaSession, SensorProps};
use std::process::exit;
use std::time::Duration;
use tokio::stream::StreamExt;
use tokio::time;

const SCAN_DURATION: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> Result<(), Report> {
    pretty_env_logger::init();

    let filters = parse_args()?;

    let (_, session) = MijiaSession::new().await?;
    let mut events = session.event_stream().await?;

    // Start scanning for Bluetooth devices, and wait a while for some to be discovered.
    session.bt_session.start_discovery().await?;
    time::sleep(SCAN_DURATION).await;

    // Get the list of sensors which are currently known, connect those which match the filter and
    // subscribe to readings.
    let sensors = session.get_sensors().await?;
    println!("Sensors:");
    for sensor in sensors
        .iter()
        .filter(|sensor| should_include_sensor(sensor, &filters))
    {
        println!("Connecting to {} ({:?})", sensor.mac_address, sensor.id);
        if let Err(e) = session.bt_session.connect(&sensor.id).await {
            println!("Failed to connect to {}: {:?}", sensor.mac_address, e);
            continue;
        }
        session.start_notify_sensor(&sensor.id).await?;
    }

    println!("Readings:");
    while let Some(event) = events.next().await {
        match event {
            MijiaEvent::Readings { id, readings } => {
                println!("{:?}: {}", id, readings);
            }
            _ => println!("Event: {:?}", event),
        }
    }

    Ok(())
}

fn parse_args() -> Result<Vec<String>, Report> {
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

    Ok(filters)
}

fn should_include_sensor(sensor: &SensorProps, filters: &Vec<String>) -> bool {
    let mac = sensor.mac_address.to_string();
    filters.is_empty() || filters.iter().any(|filter| mac.contains(filter))
}
