//! A library for connecting to Xiaomi Mijia 2 Bluetooth temperature/humidity sensors.

use core::future::Future;
use dbus::nonblock::MsgMatch;
use dbus::Message;
use futures::{Stream, StreamExt};
use std::time::Duration;

pub mod bluetooth;
mod bluetooth_event;
mod dbus_session;
pub mod decode;
pub use bluetooth::{BluetoothSession, DeviceId, MacAddress};
use bluetooth_event::BluetoothEvent;
pub use decode::Readings;

const MIJIA_NAME: &str = "LYWSD03MMC";
const SENSOR_READING_CHARACTERISTIC_PATH: &str = "/service0021/char0035";
const CONNECTION_INTERVAL_CHARACTERISTIC_PATH: &str = "/service0021/char0045";
/// 500 in little-endian
const CONNECTION_INTERVAL_500_MS: [u8; 3] = [0xF4, 0x01, 0x00];
const DBUS_METHOD_CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// The MAC address and opaque connection ID of a Mijia sensor which was discovered.
#[derive(Clone, Debug, PartialEq)]
pub struct SensorProps {
    /// An opaque identifier for the sensor, including a reference to which Bluetooth adapter it was
    /// discovered on. This can be used to connect to it.
    pub id: DeviceId,
    /// The MAC address of the sensor.
    pub mac_address: MacAddress,
}

// TODO: before publishing 1.0 to crates.io: annotate this enum as non-exhaustive.
// TESTME:
// * move this into its own event.rs
// * Construct dbus::message::Message examples, and assert that they are decoded
//   into the appropriate events.
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
                // TESTME: Make this less hacky.
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

        // TESTME (low priority): split this into filter_sensors() and test it.
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

    /// Assuming that the given device ID refers to a Mijia sensor device and that it has already
    /// been connected, subscribe to notifications of temperature/humidity readings, and adjust the
    /// connection interval to save power.
    ///
    /// Notifications will be delivered as events by `MijiaSession::event_stream()`.
    pub async fn start_notify_sensor(&self, id: &DeviceId) -> Result<(), eyre::Error> {
        let temp_humidity_path = id.object_path.to_string() + SENSOR_READING_CHARACTERISTIC_PATH;
        self.bt_session
            .dbus()
            .start_notify(&temp_humidity_path)
            .await?;

        let connection_interval_path =
            id.object_path.to_string() + CONNECTION_INTERVAL_CHARACTERISTIC_PATH;
        self.bt_session
            .dbus()
            .write_value(
                &connection_interval_path,
                CONNECTION_INTERVAL_500_MS.to_vec(),
                Default::default(),
            )
            .await?;
        Ok(())
    }

    /// Get a stream of reading/disconnected events for all sensors.
    ///
    /// If the MsgMatch is dropped then the Stream will close.
    pub async fn event_stream(
        &self,
    ) -> Result<(MsgMatch, impl Stream<Item = MijiaEvent>), eyre::Error> {
        // TESTME?: split out the `rule` constructor and test that it has the right shape?
        let mut rule = dbus::message::MatchRule::new();
        rule.msg_type = Some(dbus::message::MessageType::Signal);
        rule.sender = Some(dbus::strings::BusName::new("org.bluez").map_err(|s| eyre::eyre!(s))?);

        let (msg_match, events) = self
            .bt_session
            .connection()
            .add_match(rule)
            .await?
            .msg_stream();

        Ok((
            msg_match,
            Box::pin(events.filter_map(|event| async move { MijiaEvent::from(event) })),
        ))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn get_sensors_filters_out_sensors_with_wrong_name() -> Result<(), eyre::Report> {
        let bt_session = BluetoothSession::fake_with_device_names(&["ignored", "LYWSD03MMC"]);
        let session = MijiaSession { bt_session };

        let result = session.get_sensors().await?;

        assert_eq!(
            result
                .iter()
                .map(|s| s.mac_address.to_string())
                .collect::<Vec<_>>(),
            vec!["1"]
        );

        Ok(())
    }
}
