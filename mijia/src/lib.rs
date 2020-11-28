//! A library for connecting to Xiaomi Mijia 2 Bluetooth temperature/humidity sensors.

use core::future::Future;
use dbus::nonblock::MsgMatch;
use dbus::Message;
use futures::{Stream, StreamExt};
use std::ops::Range;
use std::time::{Duration, SystemTime};
use thiserror::Error;

pub mod bluetooth;
mod bluetooth_event;
mod decode;
pub use bluetooth::{BluetoothError, BluetoothSession, DeviceId, MacAddress, SpawnError};
use bluetooth_event::BluetoothEvent;
pub use decode::comfort_level::ComfortLevel;
use decode::history::decode_range;
pub use decode::history::Record;
pub use decode::readings::Readings;
pub use decode::temperature_unit::TemperatureUnit;
use decode::time::{decode_time, encode_time};
pub use decode::{DecodeError, EncodeError};

const MIJIA_NAME: &str = "LYWSD03MMC";
const SENSOR_READING_CHARACTERISTIC_PATH: &str = "/service0021/char0035";
const CONNECTION_INTERVAL_CHARACTERISTIC_PATH: &str = "/service0021/char0045";
const CLOCK_CHARACTERISTIC_PATH: &str = "/service0021/char0022";
const TEMPERATURE_UNIT_CHARACTERISTIC_PATH: &str = "/service0021/char0032";
const COMFORT_LEVEL_CHARACTERISTIC_PATH: &str = "/service0021/char0042";
const HISTORY_RANGE_CHARACTERISTIC_PATH: &str = "/service0021/char0025";
const HISTORY_DELETE_CHARACTERISTIC_PATH: &str = "/service0021/char003f";
const HISTORY_LAST_RECORD_CHARACTERISTIC_PATH: &str = "/service0021/char002b";
/// 500 in little-endian
const CONNECTION_INTERVAL_500_MS: [u8; 3] = [0xF4, 0x01, 0x00];
const HISTORY_DELETE_VALUE: [u8; 1] = [0x01];
const DBUS_METHOD_CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// An error interacting with a Mijia sensor.
#[derive(Debug, Error)]
pub enum MijiaError {
    /// The error was with the Bluetooth connection.
    #[error(transparent)]
    Bluetooth(#[from] BluetoothError),
    /// The error was with decoding a value from a sensor.
    #[error(transparent)]
    Decoding(#[from] DecodeError),
    /// The error was with encoding a value to send to a sensor.
    #[error(transparent)]
    Encoding(#[from] EncodeError),
}

/// The MAC address and opaque connection ID of a Mijia sensor which was discovered.
#[derive(Clone, Debug)]
pub struct SensorProps {
    /// An opaque identifier for the sensor, including a reference to which Bluetooth adapter it was
    /// discovered on. This can be used to connect to it.
    pub id: DeviceId,
    /// The MAC address of the sensor.
    pub mac_address: MacAddress,
}

/// An event from a Mijia sensor.
#[non_exhaustive]
#[derive(Clone, Debug)]
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
                let readings = Readings::decode(&value).ok()?;
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
    pub async fn new(
    ) -> Result<(impl Future<Output = Result<(), SpawnError>>, Self), BluetoothError> {
        let (handle, bt_session) = BluetoothSession::new().await?;
        Ok((handle, MijiaSession { bt_session }))
    }

    /// Get a list of all Mijia sensors which have currently been discovered.
    pub async fn get_sensors(&self) -> Result<Vec<SensorProps>, BluetoothError> {
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

    /// Get the current time of the sensor.
    pub async fn get_time(&self, id: &DeviceId) -> Result<SystemTime, MijiaError> {
        let value = self
            .bt_session
            .read_characteristic_value(id, CLOCK_CHARACTERISTIC_PATH)
            .await?;
        Ok(decode_time(&value)?)
    }

    /// Set the current time of the sensor.
    pub async fn set_time(&self, id: &DeviceId, time: SystemTime) -> Result<(), MijiaError> {
        let time_bytes = encode_time(time)?;
        Ok(self
            .bt_session
            .write_characteristic_value(id, CLOCK_CHARACTERISTIC_PATH, time_bytes)
            .await?)
    }

    /// Get the temperature unit which the sensor uses for its display.
    pub async fn get_temperature_unit(&self, id: &DeviceId) -> Result<TemperatureUnit, MijiaError> {
        let value = self
            .bt_session
            .read_characteristic_value(id, TEMPERATURE_UNIT_CHARACTERISTIC_PATH)
            .await?;
        Ok(TemperatureUnit::decode(&value)?)
    }

    /// Set the temperature unit which the sensor uses for its display.
    pub async fn set_temperature_unit(
        &self,
        id: &DeviceId,
        unit: TemperatureUnit,
    ) -> Result<(), BluetoothError> {
        Ok(self
            .bt_session
            .write_characteristic_value(id, TEMPERATURE_UNIT_CHARACTERISTIC_PATH, unit.encode())
            .await?)
    }

    /// Get the comfort level configuration which determines when the sensor displays a happy face.
    pub async fn get_comfort_level(&self, id: &DeviceId) -> Result<ComfortLevel, MijiaError> {
        let value = self
            .bt_session
            .read_characteristic_value(id, COMFORT_LEVEL_CHARACTERISTIC_PATH)
            .await?;
        Ok(ComfortLevel::decode(&value)?)
    }

    /// Set the comfort level configuration which determines when the sensor displays a happy face.
    pub async fn set_comfort_level(
        &self,
        id: &DeviceId,
        comfort_level: &ComfortLevel,
    ) -> Result<(), MijiaError> {
        Ok(self
            .bt_session
            .write_characteristic_value(
                id,
                COMFORT_LEVEL_CHARACTERISTIC_PATH,
                comfort_level.encode()?,
            )
            .await?)
    }

    /// Get the range of indices for historical data stored on the sensor.
    pub async fn get_history_range(&self, id: &DeviceId) -> Result<Range<u32>, MijiaError> {
        let value = self
            .bt_session
            .read_characteristic_value(id, HISTORY_RANGE_CHARACTERISTIC_PATH)
            .await?;
        Ok(decode_range(&value)?)
    }

    /// Delete all historical data stored on the sensor.
    pub async fn delete_history(&self, id: &DeviceId) -> Result<(), BluetoothError> {
        self.bt_session
            .write_characteristic_value(
                id,
                HISTORY_DELETE_CHARACTERISTIC_PATH,
                HISTORY_DELETE_VALUE,
            )
            .await
    }

    /// Get the last historical record stored on the sensor.
    pub async fn get_last_history_record(&self, id: &DeviceId) -> Result<Record, MijiaError> {
        let value = self
            .bt_session
            .read_characteristic_value(id, HISTORY_LAST_RECORD_CHARACTERISTIC_PATH)
            .await?;
        Ok(Record::decode(&value)?)
    }

    /// Assuming that the given device ID refers to a Mijia sensor device and that it has already
    /// been connected, subscribe to notifications of temperature/humidity readings, and adjust the
    /// connection interval to save power.
    ///
    /// Notifications will be delivered as events by `MijiaSession::event_stream()`.
    pub async fn start_notify_sensor(&self, id: &DeviceId) -> Result<(), BluetoothError> {
        self.bt_session
            .start_notify(id, SENSOR_READING_CHARACTERISTIC_PATH)
            .await?;
        self.bt_session
            .write_characteristic_value(
                id,
                CONNECTION_INTERVAL_CHARACTERISTIC_PATH,
                CONNECTION_INTERVAL_500_MS,
            )
            .await?;
        Ok(())
    }

    /// Get a stream of reading/disconnected events for all sensors.
    ///
    /// If the MsgMatch is dropped then the Stream will close.
    pub async fn event_stream(
        &self,
    ) -> Result<(MsgMatch, impl Stream<Item = MijiaEvent>), BluetoothError> {
        let mut rule = dbus::message::MatchRule::new();
        rule.msg_type = Some(dbus::message::MessageType::Signal);
        // BusName validation just checks that the length and format is valid, so it should never
        // fail for a constant that we know is valid.
        rule.sender = Some(dbus::strings::BusName::new("org.bluez").unwrap());

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
