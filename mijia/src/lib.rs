use bluez_generated::bluetooth_event::BluetoothEvent;
use bluez_generated::generated::OrgBluezGattCharacteristic1;
use core::future::Future;
use dbus::nonblock::MsgMatch;
use dbus::Message;
use futures::{Stream, StreamExt};
use std::time::Duration;

pub mod bluetooth;
pub mod decode;
pub use bluetooth::{BluetoothSession, DeviceId, MacAddress};
pub use decode::Readings;

const MIJIA_SERVICE_DATA_UUID: &str = "0000fe95-0000-1000-8000-00805f9b34fb";
const SENSOR_READING_CHARACTERISTIC_PATH: &str = "/service0021/char0035";
const CONNECTION_INTERVAL_CHARACTERISTIC_PATH: &str = "/service0021/char0045";
/// 500 in little-endian
const CONNECTION_INTERVAL_500_MS: [u8; 3] = [0xF4, 0x01, 0x00];
const DBUS_METHOD_CALL_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug)]
pub struct SensorProps {
    pub id: DeviceId,
    pub mac_address: MacAddress,
}

// TODO before publishing to crates.io: annotate this enum as non-exhaustive.
/// An event from a Mijia sensor.
#[derive(Clone)]
pub enum MijiaEvent {
    Readings { id: DeviceId, readings: Readings },
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

/// A wrapper around a bluetooth session which adds some methods for dealing with Mijia sensors.
/// The underlying bluetooth session may still be accessed.
pub struct MijiaSession {
    pub bt_session: BluetoothSession,
}

impl MijiaSession {
    /// Returns a tuple of (join handle, Self).
    /// If the join handle ever completes then you're in trouble and should
    /// probably restart the process.
    pub async fn new(
    ) -> Result<(impl Future<Output = Result<(), anyhow::Error>>, Self), anyhow::Error> {
        let (handle, bt_session) = BluetoothSession::new().await?;
        Ok((handle, MijiaSession { bt_session }))
    }

    /// Get a list of all Mijia sensors which have currently been discovered.
    pub async fn get_sensors(&self) -> Result<Vec<SensorProps>, anyhow::Error> {
        let devices = self.bt_session.get_devices().await?;

        let sensors = devices
            .into_iter()
            .filter_map(|device| {
                if device.service_data.contains_key(MIJIA_SERVICE_DATA_UUID) {
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

    /// Assuming that the given device ID refers to a Mijia sensor device and that it has already
    /// been connected, subscribe to notifications of temperature/humidity readings, and adjust the
    /// connection interval to save power.
    ///
    /// Notifications will be delivered as events by `MijiaSession::event_stream()`.
    pub async fn start_notify_sensor(&self, id: &DeviceId) -> Result<(), anyhow::Error> {
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
    ) -> Result<(MsgMatch, impl Stream<Item = MijiaEvent>), anyhow::Error> {
        let mut rule = dbus::message::MatchRule::new();
        rule.msg_type = Some(dbus::message::MessageType::Signal);
        rule.sender =
            Some(dbus::strings::BusName::new("org.bluez").map_err(|s| anyhow::anyhow!(s))?);

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
