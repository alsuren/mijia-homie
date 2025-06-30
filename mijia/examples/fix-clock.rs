//! Example to scan for sensors, connect to each one in turn, and correct its clock if necessary.

use backoff::ExponentialBackoff;
use backoff::future::retry;
use chrono::{DateTime, Utc};
use eyre::Report;
use futures::TryFutureExt;
use mijia::{MijiaSession, SensorProps, SignedDuration};
use std::process::exit;
use std::time::{Duration, SystemTime};
use tokio::time;

const SCAN_DURATION: Duration = Duration::from_secs(5);
/// Only correct clocks which are wrong by more than this amount.
const MINIMUM_OFFSET: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::main]
async fn main() -> Result<(), Report> {
    pretty_env_logger::init();

    let filters = parse_args()?;

    let (_, session) = MijiaSession::new().await?;

    // Start scanning for Bluetooth devices, and wait a while for some to be discovered.
    session.bt_session.start_discovery().await?;
    time::sleep(SCAN_DURATION).await;

    // Get the list of sensors which are currently known, connect to them and print their properties.
    let sensors = session.get_sensors().await?;
    println!("Sensors:");
    for sensor in sensors {
        if !should_include_sensor(&sensor, &filters) {
            println!("Skipping {}", sensor.mac_address);
            continue;
        }
        println!("Connecting to {} ({})", sensor.mac_address, sensor.id);
        if let Err(e) = retry(
            ExponentialBackoff {
                max_elapsed_time: Some(CONNECT_TIMEOUT),
                ..Default::default()
            },
            || session.bt_session.connect(&sensor.id).map_err(Into::into),
        )
        .await
        {
            println!("Failed to connect to {}: {:?}", sensor.mac_address, e);
            continue;
        }
        let last_record_time = session.get_last_history_record(&sensor.id).await?.time;
        let sensor_time = session.get_time(&sensor.id).await?;
        let now = SystemTime::now();
        let last_record_utc: DateTime<Utc> = last_record_time.into();
        let sensor_time_utc: DateTime<Utc> = sensor_time.into();
        let now_utc: DateTime<Utc> = now.into();
        let offset: SignedDuration = now.duration_since(sensor_time).into();
        let last_record_offset: SignedDuration = now.duration_since(last_record_time).into();
        println!(
            "Time: {sensor_time_utc} (should be {now_utc}, offset {offset:?}), Last stored value: {last_record_utc} ({last_record_offset:?} ago)"
        );
        if offset.duration > MINIMUM_OFFSET {
            println!("Correcting clock.");
            session.set_time(&sensor.id, now).await?;
        }

        if let Err(e) = session.bt_session.disconnect(&sensor.id).await {
            println!("Failed to disconnect from {}: {:?}", sensor.mac_address, e);
            continue;
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
        eprintln!("Invalid MAC addresses {filters:?}");
        eprintln!("Usage:");
        eprintln!("  {binary_name} [MAC address]...");
        exit(1);
    }

    Ok(filters)
}

fn should_include_sensor(sensor: &SensorProps, filters: &[String]) -> bool {
    let mac = sensor.mac_address.to_string();
    filters.is_empty() || filters.iter().any(|filter| mac.contains(filter))
}
