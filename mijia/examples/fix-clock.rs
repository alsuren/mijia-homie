//! Example to scan for sensors, connect to each one in turn, and correct its clock if necessary.

use backoff::future::retry;
use backoff::ExponentialBackoff;
use chrono::{DateTime, Utc};
use eyre::Report;
use fmt::Write;
use futures::TryFutureExt;
use mijia::{MijiaSession, SensorProps};
use std::fmt::{self, Debug, Formatter};
use std::time::{Duration, SystemTimeError};
use std::{process::exit, time::SystemTime};
use tokio::time;
use tokio_compat_02::FutureExt;

const SCAN_DURATION: Duration = Duration::from_secs(5);
/// Only correct clocks which are wrong by more than this amount.
const MINIMUM_OFFSET: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// A duration which may be negative.
#[derive(Clone, Eq, PartialEq)]
struct SignedDuration {
    positive: bool,
    duration: Duration,
}

impl From<Duration> for SignedDuration {
    fn from(duration: Duration) -> Self {
        SignedDuration {
            positive: true,
            duration,
        }
    }
}

impl From<Result<Duration, SystemTimeError>> for SignedDuration {
    fn from(result: Result<Duration, SystemTimeError>) -> Self {
        match result {
            Ok(duration) => duration.into(),
            Err(err) => SignedDuration {
                positive: false,
                duration: err.duration(),
            },
        }
    }
}

impl Debug for SignedDuration {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if !self.positive {
            f.write_char('-')?;
        }
        self.duration.fmt(f)
    }
}

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
        .compat()
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
            "Time: {} (should be {}, offset {:?}), Last stored value: {} ({:?} ago)",
            sensor_time_utc, now_utc, offset, last_record_utc, last_record_offset
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
