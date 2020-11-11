//! A library for connecting to Xiaomi Mijia 2 Bluetooth temperature/humidity sensors.

use bluez_generated::OrgBluezGattCharacteristic1;
use core::future::Future;
use dbus::nonblock::MsgMatch;
use dbus::Message;
use futures::{Stream, StreamExt};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

pub mod bluetooth;
mod bluetooth_event;
pub mod decode;
pub use bluetooth::{BluetoothSession, DeviceId, MacAddress};
use bluetooth_event::BluetoothEvent;
pub use decode::Readings;
use decode::{decode_time, encode_time};

const MIJIA_NAME: &str = "LYWSD03MMC";
const SENSOR_READING_CHARACTERISTIC_PATH: &str = "/service0021/char0035";
const CONNECTION_INTERVAL_CHARACTERISTIC_PATH: &str = "/service0021/char0045";
const CLOCK_CHARACTERISTIC_PATH: &str = "/service0021/char0022";
/// 500 in little-endian
const CONNECTION_INTERVAL_500_MS: [u8; 3] = [0xF4, 0x01, 0x00];
const DBUS_METHOD_CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// The MAC address and opaque connection ID of a Mijia sensor which was discovered.
#[derive(Clone, Debug)]
pub struct SensorProps {
    /// An opaque identifier for the sensor, including a reference to which Bluetooth adapter it was
    /// discovered on. This can be used to connect to it.
    pub id: DeviceId,
    /// The MAC address of the sensor.
    pub mac_address: MacAddress,
}

// TODO before publishing to crates.io: annotate this enum as non-exhaustive.
/// An event from a Mijia sensor.
#[derive(Clone)]
pub enum MijiaEvent {
    /// A sensor has sent a new set of readings.
    Readings { id: DeviceId, readings: Readings },
    /// The Bluetooth connection to a sensor has been lost.
    Disconnected { id: DeviceId },
}

impl MijiaEvent {
    fn from(conn_msg: Message) -> Option<Self> {
        match BluetoothEvent::from(conn_msg) {
            Some(BluetoothEvent::Value { object_path, value }) => {
                // TODO: Make this less hacky.
                let object_path = object_path.strip_suffix(SENSOR_READING_CHARACTERISTIC_PATH)?;
                let readings = Readings::decode(&value)?;
                Some(MijiaEvent::Readings {
                    id: DeviceId::new(object_path),
                    readings,
                })
            }
            Some(BluetoothEvent::Connected {
                object_path,
                connected: false,
            }) => Some(MijiaEvent::Disconnected {
                id: DeviceId { object_path },
            }),
            _ => None,
        }
    }
}

/// A wrapper around a Bluetooth session which adds some methods for dealing with Mijia sensors.
/// The underlying Bluetooth session may still be accessed.
pub struct MijiaSession {
    pub bt_session: BluetoothSession,
}

impl MijiaSession {
    /// Returns a tuple of (join handle, Self).
    /// If the join handle ever completes then you're in trouble and should
    /// probably restart the process.
    pub async fn new() -> Result<(impl Future<Output = Result<(), eyre::Error>>, Self), eyre::Error>
    {
        let (handle, bt_session) = BluetoothSession::new().await?;
        Ok((handle, MijiaSession { bt_session }))
    }

    /// Get a list of all Mijia sensors which have currently been discovered.
    pub async fn get_sensors(&self) -> Result<Vec<SensorProps>, eyre::Error> {
        let devices = self.bt_session.get_devices().await?;

        let sensors = devices
            .into_iter()
            .filter_map(|device| {
                log::trace!(
                    "{} ({:?}): {:?}",
                    device.mac_address,
                    device.name,
                    device.service_data
                );
                if device.name.as_deref() == Some(MIJIA_NAME) {
                    Some(SensorProps {
                        id: device.id,
                        mac_address: device.mac_address,
                    })
                } else {
                    None
                }
            })
            .collect();
        Ok(sensors)
    }

    pub async fn get_time(&self, id: &DeviceId) -> Result<SystemTime, eyre::Error> {
        let clock_path = id.object_path.to_string() + CLOCK_CHARACTERISTIC_PATH;
        let clock = dbus::nonblock::Proxy::new(
            "org.bluez",
            clock_path,
            DBUS_METHOD_CALL_TIMEOUT,
            self.bt_session.connection.clone(),
        );
        let time_bytes = clock.read_value(HashMap::new()).await?;
        decode_time(&time_bytes)
    }

    pub async fn set_time(&self, id: &DeviceId, time: SystemTime) -> Result<(), eyre::Error> {
        let clock_path = id.object_path.to_string() + CLOCK_CHARACTERISTIC_PATH;
        let clock = dbus::nonblock::Proxy::new(
            "org.bluez",
            clock_path,
            DBUS_METHOD_CALL_TIMEOUT,
            self.bt_session.connection.clone(),
        );
        let time_bytes = encode_time(time)?;
        clock.write_value(time_bytes.into(), HashMap::new()).await?;
        Ok(())
    }

    /// Assuming that the given device ID refers to a Mijia sensor device and that it has already
    /// been connected, subscribe to notifications of temperature/humidity readings, and adjust the
    /// connection interval to save power.
    ///
    /// Notifications will be delivered as events by `MijiaSession::event_stream()`.
    pub async fn start_notify_sensor(&self, id: &DeviceId) -> Result<(), eyre::Error> {
        let temp_humidity_path = id.object_path.to_string() + SENSOR_READING_CHARACTERISTIC_PATH;
        let temp_humidity = dbus::nonblock::Proxy::new(
            "org.bluez",
            temp_humidity_path,
            DBUS_METHOD_CALL_TIMEOUT,
            self.bt_session.connection.clone(),
        );
        temp_humidity.start_notify().await?;

        let connection_interval_path =
            id.object_path.to_string() + CONNECTION_INTERVAL_CHARACTERISTIC_PATH;
        let connection_interval = dbus::nonblock::Proxy::new(
            "org.bluez",
            connection_interval_path,
            DBUS_METHOD_CALL_TIMEOUT,
            self.bt_session.connection.clone(),
        );
        connection_interval
            .write_value(CONNECTION_INTERVAL_500_MS.to_vec(), Default::default())
            .await?;
        Ok(())
    }

    /// Get a stream of reading/disconnected events for all sensors.
    ///
    /// If the MsgMatch is dropped then the Stream will close.
    pub async fn event_stream(
        &self,
    ) -> Result<(MsgMatch, impl Stream<Item = MijiaEvent>), eyre::Error> {
        let mut rule = dbus::message::MatchRule::new();
        rule.msg_type = Some(dbus::message::MessageType::Signal);
        rule.sender = Some(dbus::strings::BusName::new("org.bluez").map_err(|s| eyre::eyre!(s))?);

        let (msg_match, events) = self
            .bt_session
            .connection
            .add_match(rule)
            .await?
            .msg_stream();

        Ok((
            msg_match,
            Box::pin(events.filter_map(|event| async move { MijiaEvent::from(event) })),
        ))
    }
}
