//! Example to log sensor measurements of [RuuviTag]s.
//!
//! RuuviTags broadcast their sensors' measurements via the manufacturer-specific data
//! advertisements. Accordingly, duplicate data must be accepted.
//!
//! In detail this example looks for RuuviTags in reach and writes temperature, humidity and air
//! pressure to `stdout` for each measurement.
//!
//! [RuuviTag]: https://ruuvi.com/ruuvitag-specs/

use std::collections::HashMap;

use bluez_async::{BluetoothEvent, BluetoothSession, DeviceEvent, DiscoveryFilter};
use futures::stream::StreamExt;

/// The [Bluetooth company identifier](https://www.bluetooth.com/specifications/assigned-numbers/company-identifiers/)
/// of Ruuvi Innovations Ltd.
const RUUVI_ID: u16 = 0x0499;

/// Protocol version of RuuviTags' data format
/// [RAWv2](https://github.com/ruuvi/ruuvi-sensor-protocols/blob/master/dataformat_05.md)
const PROTOCOL_VERSION: u8 = 0x05;

/// Search for manufacturer data from a Ruuvi device with protocol version 5.
fn get_ruuvi_data(mdata: &HashMap<u16, Vec<u8>>) -> Option<&Vec<u8>> {
    match mdata.get(&RUUVI_ID) {
        Some(data) if !data.is_empty() && data[0] == PROTOCOL_VERSION => Some(data),
        _ => None,
    }
}

/// Temperature in `°C`.
fn temperature(data: &[u8]) -> f64 {
    assert!(data.len() >= 3);
    let value = [data[1], data[2]];
    let value = u16::from_be_bytes(value);
    (value as f64) * 0.005
}

/// Humidity in `%`.
fn humidity(data: &[u8]) -> f64 {
    assert!(data.len() >= 5);
    let value = [data[3], data[4]];
    let value = u16::from_be_bytes(value);
    (value as f64) * 0.0025
}

/// Pressure in `Pa`.
fn pressure(data: &[u8]) -> f64 {
    assert!(data.len() >= 7);
    let value = [data[5], data[6]];
    let value = u16::from_be_bytes(value);
    (value as f64) + 50_000_f64
}

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    pretty_env_logger::init();

    let (_, session) = BluetoothSession::new().await?;
    let mut events = session.event_stream().await?;
    // Requires duplicate data else new sensor measurements wouldn't be recognized.
    session
        .start_discovery_with_filter(&DiscoveryFilter {
            duplicate_data: Some(true),
            ..DiscoveryFilter::default()
        })
        .await?;

    while let Some(event) = events.next().await {
        match event {
            BluetoothEvent::Device {
                id,
                event: DeviceEvent::ManufacturerData { manufacturer_data },
            } => {
                if let Some(data) = get_ruuvi_data(&manufacturer_data) {
                    let t = temperature(data);
                    let h = humidity(data);
                    let p = pressure(data);
                    println!(
                        "RuuviTag {} measured: t = {:6.2} °C, h = {:6.2} %, p = {:6} Pa",
                        id, t, h, p
                    );
                }
            }
            _ => {}
        }
    }

    Ok(())
}
