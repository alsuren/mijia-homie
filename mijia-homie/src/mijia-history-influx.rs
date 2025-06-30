//! Utility program to dump historical data from sensors to InfluxDB.

#[allow(dead_code)]
mod config;
mod mijia_history_config;

use crate::config::read_sensor_names;
use crate::mijia_history_config::{Config, get_influxdb_client};
use eyre::Report;
use influx_db_client::{Client, Point, Precision};
use mijia::{HistoryRecord, MijiaSession, SignedDuration, bluetooth::MacAddress};
use std::time::{Duration, SystemTime};
use tokio::time;

const SCAN_DURATION: Duration = Duration::from_secs(5);
const INFLUXDB_PRECISION: Option<Precision> = Some(Precision::Milliseconds);

#[tokio::main]
async fn main() -> Result<(), Report> {
    stable_eyre::install()?;
    pretty_env_logger::init();
    color_backtrace::install();

    let config = Config::from_file()?;
    let names = read_sensor_names(&config.sensor_names_filename)?;

    let influxdb_client = get_influxdb_client(&config.influxdb)?;
    let (_, session) = MijiaSession::new().await?;

    // Start scanning for Bluetooth devices, and wait a while for some to be discovered.
    println!("Scanning...");
    session.bt_session.start_discovery().await?;
    time::sleep(SCAN_DURATION).await;

    // Get the list of sensors which are currently visible and connect those for which we have
    // names.
    let sensors = session.get_sensors().await?;
    for sensor in sensors.iter() {
        if let Some(name) = names.get(&sensor.mac_address) {
            println!("Connecting to {} ({})...", name, sensor.mac_address);
            if let Err(e) = session.bt_session.connect(&sensor.id).await {
                log::error!("Failed to connect to {name}: {e:?}");
                continue;
            }

            // Check that the clock isn't too badly wrong.
            let sensor_time = session.get_time(&sensor.id).await?;
            let now = SystemTime::now();
            let offset: SignedDuration = now.duration_since(sensor_time).into();
            if offset.duration > config.max_clock_offset {
                println!(
                    "Clock offset {:?} is more than {:?}, skipping.",
                    offset, config.max_clock_offset
                );
            } else {
                println!("Sensor time offset {offset:?}, reading history...");
                let history = session.get_all_history(&sensor.id).await?;
                write_history(
                    &influxdb_client,
                    &config.influxdb.measurement,
                    &sensor.mac_address,
                    name,
                    history,
                )
                .await?;
                println!("Written to InfluxDB.");
            }

            if let Err(e) = session.bt_session.disconnect(&sensor.id).await {
                log::error!("Disconnecting failed: {e:?}");
            }
        }
    }

    Ok(())
}

async fn write_history(
    influxdb_client: &Client,
    measurement: &str,
    mac_address: &MacAddress,
    name: &str,
    history: Vec<Option<HistoryRecord>>,
) -> Result<(), Report> {
    let points = history
        .into_iter()
        .flatten()
        .map(|record| point_for_record(measurement, mac_address, name, &record));
    influxdb_client
        .write_points(points, INFLUXDB_PRECISION, None)
        .await?;
    Ok(())
}

fn point_for_record<'a>(
    measurement: &str,
    mac_address: &MacAddress,
    name: &'a str,
    record: &HistoryRecord,
) -> Point<'a> {
    Point::new(measurement)
        .add_timestamp(
            record
                .time
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
        )
        .add_tag("node_id", mac_address.to_string().replace(':', ""))
        .add_tag("node_name", name)
        .add_field("temperature_min", record.temperature_min as f64)
        .add_field("temperature_max", record.temperature_max as f64)
        .add_field("humidity_min", record.humidity_min as i64)
        .add_field("humidity_max", record.humidity_max as i64)
}
