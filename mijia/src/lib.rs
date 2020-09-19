use bluez_generated::generated::OrgBluezGattCharacteristic1;
use dbus::arg::RefArg;
use dbus::nonblock::stdintf::org_freedesktop_dbus::ObjectManager;
use itertools::Itertools;
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;
use std::time::Duration;

pub mod decode;
pub mod session;
pub use decode::Readings;
pub use session::{MijiaEvent, MijiaSession};

const MIJIA_SERVICE_DATA_UUID: &str = "0000fe95-0000-1000-8000-00805f9b34fb";
const SENSOR_READING_CHARACTERISTIC_PATH: &str = "/service0021/char0035";
const CONNECTION_INTERVAL_CHARACTERISTIC_PATH: &str = "/service0021/char0045";
/// 500 in little-endian
const CONNECTION_INTERVAL_500_MS: [u8; 3] = [0xF4, 0x01, 0x00];
const DBUS_METHOD_CALL_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SensorId {
    object_path: String,
}

impl SensorId {
    pub(crate) fn new(object_path: &str) -> Self {
        Self {
            object_path: object_path.to_owned(),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MacAddress(String);

impl Display for MacAddress {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseMacAddressError();

impl Display for ParseMacAddressError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Invalid MAC address")
    }
}

impl Error for ParseMacAddressError {}

impl FromStr for MacAddress {
    type Err = ParseMacAddressError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let octets: Vec<_> = s.split(":").collect();
        if octets.len() != 6 {
            return Err(ParseMacAddressError());
        }
        for octet in octets {
            if octet.len() != 2 {
                return Err(ParseMacAddressError());
            }
            if !octet.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(ParseMacAddressError());
            }
        }
        Ok(MacAddress(s.to_uppercase()))
    }
}

#[derive(Clone, Debug)]
pub struct SensorProps {
    pub id: SensorId,
    pub mac_address: MacAddress,
}

pub async fn get_sensors(bt_session: &MijiaSession) -> Result<Vec<SensorProps>, anyhow::Error> {
    let bluez_root = dbus::nonblock::Proxy::new(
        "org.bluez",
        "/",
        DBUS_METHOD_CALL_TIMEOUT,
        bt_session.connection.clone(),
    );
    let tree = bluez_root.get_managed_objects().await?;

    let sensors = tree
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
                .filter_map(|(k, v)| Some((k.as_str()?.into(), v)))
                .collect();

            if service_data.contains_key(MIJIA_SERVICE_DATA_UUID) {
                Some(SensorProps {
                    id: SensorId {
                        object_path: path.to_string(),
                    },
                    mac_address: MacAddress(mac_address),
                })
            } else {
                None
            }
        })
        .collect();
    Ok(sensors)
}

pub async fn start_notify_sensor(
    bt_session: &MijiaSession,
    id: &SensorId,
) -> Result<(), anyhow::Error> {
    let temp_humidity_path = id.object_path.to_string() + SENSOR_READING_CHARACTERISTIC_PATH;
    let temp_humidity = dbus::nonblock::Proxy::new(
        "org.bluez",
        temp_humidity_path,
        DBUS_METHOD_CALL_TIMEOUT,
        bt_session.connection.clone(),
    );
    temp_humidity.start_notify().await?;

    let connection_interval_path =
        id.object_path.to_string() + CONNECTION_INTERVAL_CHARACTERISTIC_PATH;
    let connection_interval = dbus::nonblock::Proxy::new(
        "org.bluez",
        connection_interval_path,
        DBUS_METHOD_CALL_TIMEOUT,
        bt_session.connection.clone(),
    );
    connection_interval
        .write_value(CONNECTION_INTERVAL_500_MS.to_vec(), Default::default())
        .await?;
    Ok(())
}
