use bluez_generated::generated::OrgBluezGattCharacteristic1;
use dbus::arg::RefArg;
use dbus::nonblock::stdintf::org_freedesktop_dbus::ObjectManager;
use itertools::Itertools;
use std::cmp::max;
use std::collections::HashMap;
use std::convert::TryInto;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{self, BufRead, BufReader, ErrorKind};
use std::time::Duration;

pub mod session;
pub use session::{MijiaEvent, MijiaSession};

pub const MIJIA_SERVICE_DATA_UUID: &str = "0000fe95-0000-1000-8000-00805f9b34fb";
pub const SERVICE_CHARACTERISTIC_PATH: &str = "/service0021/char0035";
pub const CONNECTION_INTERVAL_CHARACTERISTIC_PATH: &str = "/service0021/char0045";
/// 500 in little-endian
pub const CONNECTION_INTERVAL_500_MS: [u8; 3] = [0xF4, 0x01, 0x00];

pub struct SensorProps {
    pub object_path: String,
    pub mac_address: String,
}

pub async fn get_sensors(bt_session: MijiaSession) -> Result<Vec<SensorProps>, anyhow::Error> {
    let bluez_root = dbus::nonblock::Proxy::new(
        "org.bluez",
        "/",
        Duration::from_secs(30),
        bt_session.connection.clone(),
    );
    let tree = bluez_root.get_managed_objects().await?;

    let paths = tree
        .into_iter()
        .filter_map(|(path, interfaces)| {
            // FIXME: can we generate a strongly typed deserialiser for this,
            // based on the introspection data?
            let device_properties = interfaces.get("org.bluez.Device1")?;

            let mac_address = device_properties
                .get("Address")?
                .as_iter()?
                .filter_map(|addr| addr.as_str())
                .next()?
                .to_string();
            // UUIDs don't get populated until we connect. Use:
            //     "ServiceData": Variant(InternalDict { data: [
            //         ("0000fe95-0000-1000-8000-00805f9b34fb", Variant([48, 88, 91, 5, 1, 23, 33, 215, 56, 193, 164, 40, 1, 0])
            //     )], outer_sig: Signature("a{sv}") })
            // instead.
            let service_data: HashMap<String, _> = device_properties
                .get("ServiceData")?
                // Variant(...)
                .as_iter()?
                .next()?
                // InternalDict(...)
                .as_iter()?
                .tuples::<(_, _)>()
                .map(|(k, v)| (k.as_str().unwrap().into(), v))
                .collect();

            if service_data.contains_key(MIJIA_SERVICE_DATA_UUID) {
                Some(SensorProps {
                    object_path: path.to_string(),
                    mac_address,
                })
            } else {
                None
            }
        })
        .collect();
    Ok(paths)
}

pub async fn start_notify_sensor(
    bt_session: MijiaSession,
    device_path: &str,
) -> Result<(), anyhow::Error> {
    let temp_humidity_path: String = device_path.to_string() + SERVICE_CHARACTERISTIC_PATH;
    let temp_humidity = dbus::nonblock::Proxy::new(
        "org.bluez",
        temp_humidity_path,
        Duration::from_secs(30),
        bt_session.connection.clone(),
    );
    temp_humidity.start_notify().await?;

    let connection_interval_path: String =
        device_path.to_string() + CONNECTION_INTERVAL_CHARACTERISTIC_PATH;
    let connection_interval = dbus::nonblock::Proxy::new(
        "org.bluez",
        connection_interval_path,
        Duration::from_secs(30),
        bt_session.connection.clone(),
    );
    connection_interval
        .write_value(CONNECTION_INTERVAL_500_MS.to_vec(), Default::default())
        .await?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Readings {
    /// Temperature in ºC, with 2 decimal places of precision
    pub temperature: f32,
    /// Percent humidity
    pub humidity: u8,
    /// Voltage in millivolts
    pub battery_voltage: u16,
    /// Inferred from `battery_voltage` with a bit of hand-waving.
    pub battery_percent: u16,
}

impl Display for Readings {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(
            f,
            "Temperature: {:.2}ºC Humidity: {:?}% Battery: {:?} mV ({:?}%)",
            self.temperature, self.humidity, self.battery_voltage, self.battery_percent
        )
    }
}

pub fn decode_value(value: &[u8]) -> Option<Readings> {
    if value.len() != 5 {
        return None;
    }

    let mut temperature_array = [0; 2];
    temperature_array.clone_from_slice(&value[..2]);
    let temperature = i16::from_le_bytes(temperature_array) as f32 * 0.01;
    let humidity = value[2];
    let battery_voltage = u16::from_le_bytes(value[3..5].try_into().unwrap());
    let battery_percent = (max(battery_voltage, 2100) - 2100) / 10;
    Some(Readings {
        temperature,
        humidity,
        battery_voltage,
        battery_percent,
    })
}

/// Read the given file of key-value pairs into a hashmap.
/// Returns an empty hashmap if the file doesn't exist, or an error if it is malformed.
pub fn hashmap_from_file(filename: &str) -> Result<HashMap<String, String>, io::Error> {
    let mut map: HashMap<String, String> = HashMap::new();
    if let Ok(file) = File::open(filename) {
        for line in BufReader::new(file).lines() {
            let line = line?;
            let parts: Vec<&str> = line.splitn(2, '=').collect();
            if parts.len() != 2 {
                return Err(io::Error::new(
                    ErrorKind::Other,
                    format!("Invalid line '{}'", line),
                ));
            }
            map.insert(parts[0].to_string(), parts[1].to_string());
        }
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_empty() {
        assert_eq!(decode_value(&[]), None);
    }

    #[test]
    fn decode_too_short() {
        assert_eq!(decode_value(&[1, 2, 3, 4]), None);
    }

    #[test]
    fn decode_too_long() {
        assert_eq!(decode_value(&[1, 2, 3, 4, 5, 6]), None);
    }

    #[test]
    fn decode_valid() {
        assert_eq!(
            decode_value(&[1, 2, 3, 4, 10]),
            Some(Readings {
                temperature: 5.13,
                humidity: 3,
                battery_voltage: 2564,
                battery_percent: 46
            })
        );
    }
}
