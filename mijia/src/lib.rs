//! A library for connecting to Xiaomi Mijia 2 Bluetooth temperature/humidity sensors.
//!
//! Currently only supports running on Linux, as it depends on BlueZ for Bluetooth.
//!
//! Start by creating a [`MijiaSession`].
//!
//! [`MijiaSession']: struct.MijiaSession.html

pub use bluez_async as bluetooth;
use bluez_async::{
    BluetoothError, BluetoothEvent, BluetoothSession, CharacteristicEvent, DeviceEvent, DeviceId,
    DeviceInfo, MacAddress, SpawnError,
};
use core::future::Future;
use futures::Stream;
use std::ops::Range;
use std::time::{Duration, SystemTime};
use thiserror::Error;
use tokio::pin;
use tokio_stream::StreamExt;
use uuid::Uuid;

mod decode;
mod signed_duration;
pub use decode::comfort_level::ComfortLevel;
use decode::history::decode_range;
pub use decode::history::HistoryRecord;
pub use decode::readings::Readings;
pub use decode::temperature_unit::TemperatureUnit;
use decode::time::{decode_time, encode_time};
pub use decode::{DecodeError, EncodeError};
pub use signed_duration::SignedDuration;

const MIJIA_NAME: &str = "LYWSD03MMC";
const SERVICE_UUID: Uuid = Uuid::from_u128(0xebe0ccb0_7a0a_4b0c_8a1a_6ff2997da3a6);
const CLOCK_CHARACTERISTIC_UUID: Uuid = Uuid::from_u128(0xebe0ccb7_7a0a_4b0c_8a1a_6ff2997da3a6);
const HISTORY_RANGE_CHARACTERISTIC_UUID: Uuid =
    Uuid::from_u128(0xebe0ccb9_7a0a_4b0c_8a1a_6ff2997da3a6);
const HISTORY_INDEX_CHARACTERISTIC_UUID: Uuid =
    Uuid::from_u128(0xebe0ccba_7a0a_4b0c_8a1a_6ff2997da3a6);
const HISTORY_LAST_RECORD_CHARACTERISTIC_UUID: Uuid =
    Uuid::from_u128(0xebe0ccbb_7a0a_4b0c_8a1a_6ff2997da3a6);
const HISTORY_RECORDS_CHARACTERISTIC_UUID: Uuid =
    Uuid::from_u128(0xebe0ccbc_7a0a_4b0c_8a1a_6ff2997da3a6);
const TEMPERATURE_UNIT_CHARACTERISTIC_UUID: Uuid =
    Uuid::from_u128(0xebe0ccbe_7a0a_4b0c_8a1a_6ff2997da3a6);
const SENSOR_READING_CHARACTERISTIC_UUID: Uuid =
    Uuid::from_u128(0xebe0ccc1_7a0a_4b0c_8a1a_6ff2997da3a6);
const HISTORY_DELETE_CHARACTERISTIC_UUID: Uuid =
    Uuid::from_u128(0xebe0ccd1_7a0a_4b0c_8a1a_6ff2997da3a6);
const COMFORT_LEVEL_CHARACTERISTIC_UUID: Uuid =
    Uuid::from_u128(0xebe0ccd7_7a0a_4b0c_8a1a_6ff2997da3a6);
const CONNECTION_INTERVAL_CHARACTERISTIC_UUID: Uuid =
    Uuid::from_u128(0xebe0ccd8_7a0a_4b0c_8a1a_6ff2997da3a6);
/// 500 in little-endian
const CONNECTION_INTERVAL_500_MS: [u8; 3] = [0xF4, 0x01, 0x00];
const HISTORY_DELETE_VALUE: [u8; 1] = [0x01];
const HISTORY_RECORD_TIMEOUT: Duration = Duration::from_secs(2);

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
    /// A new sensor has been discovered.
    Discovered { id: DeviceId },
    /// A sensor has sent a new set of readings.
    Readings { id: DeviceId, readings: Readings },
    /// A sensor has sent a new historical record.
    HistoryRecord { id: DeviceId, record: HistoryRecord },
    /// The Bluetooth connection to a sensor has been lost.
    Disconnected { id: DeviceId },
}

impl MijiaEvent {
    pub async fn from(event: BluetoothEvent, session: BluetoothSession) -> Option<Self> {
        match event {
            BluetoothEvent::Characteristic {
                id: characteristic,
                event: CharacteristicEvent::Value { value },
            } => {
                let info = session
                    .get_characteristic_info(&characteristic)
                    .await
                    .map_err(|e| log::error!("Error getting characteristic UUID: {e:?}"))
                    .ok()?;
                match info.uuid {
                    SENSOR_READING_CHARACTERISTIC_UUID => match Readings::decode(&value) {
                        Ok(readings) => Some(MijiaEvent::Readings {
                            id: characteristic.service().device(),
                            readings,
                        }),
                        Err(e) => {
                            log::error!("Error decoding readings: {e:?}");
                            None
                        }
                    },
                    HISTORY_RECORDS_CHARACTERISTIC_UUID => match HistoryRecord::decode(&value) {
                        Ok(record) => Some(MijiaEvent::HistoryRecord {
                            id: characteristic.service().device(),
                            record,
                        }),
                        Err(e) => {
                            log::error!("Error decoding historical record: {e:?}");
                            None
                        }
                    },
                    _ => {
                        log::trace!(
                            "Got BluetoothEvent::Value for characteristic {characteristic:?} with value {value:?}"
                        );
                        None
                    }
                }
            }
            BluetoothEvent::Device {
                id,
                event: DeviceEvent::Connected { connected: false },
            } => Some(MijiaEvent::Disconnected { id }),
            BluetoothEvent::Device {
                id,
                event: DeviceEvent::Discovered,
            } => {
                // Only pass through discovery events for sensors we recognise.
                let device = session
                    .get_device_info(&id)
                    .await
                    .map_err(|e| log::error!("Error getting device info: {e:?}"))
                    .ok()?;
                if is_mijia_sensor(&device) {
                    Some(MijiaEvent::Discovered { id })
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

/// A wrapper around a Bluetooth session which adds some methods for dealing with Mijia sensors.
/// This is the main entry point to the library.
///
/// The underlying Bluetooth session may still be accessed. For example, to scan for sensors:
/// ```rust
/// # use std::error::Error;
/// # use std::time::Duration;
/// # use tokio::time;
/// # use mijia::MijiaSession;
/// # async fn example() -> Result<(), Box<dyn Error>> {
/// // Create a new session. This establishes the D-Bus connection. In this case we ignore the join
/// // handle, as we don't intend to run indefinitely.
/// let (_, session) = MijiaSession::new().await?;
///
/// // Start scanning for Bluetooth devices, and wait a few seconds for some to be discovered.
/// session.bt_session.start_discovery().await?;
/// time::sleep(Duration::from_secs(5)).await;
///
/// // Get the list of sensors which are currently known.
/// let sensors = session.get_sensors().await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct MijiaSession {
    /// The underlying `BluetoothSession`. You can use this for Bluetooth operations which are not
    /// specific to Mijia sensors, such as connecting and disconnecting.
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
                if is_mijia_sensor(&device) {
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
        let characteristic = self
            .bt_session
            .get_service_characteristic_by_uuid(id, SERVICE_UUID, CLOCK_CHARACTERISTIC_UUID)
            .await?;
        let value = self
            .bt_session
            .read_characteristic_value(&characteristic.id)
            .await?;
        Ok(decode_time(&value)?)
    }

    /// Set the current time of the sensor.
    pub async fn set_time(&self, id: &DeviceId, time: SystemTime) -> Result<(), MijiaError> {
        let time_bytes = encode_time(time)?;
        let characteristic = self
            .bt_session
            .get_service_characteristic_by_uuid(id, SERVICE_UUID, CLOCK_CHARACTERISTIC_UUID)
            .await?;
        Ok(self
            .bt_session
            .write_characteristic_value(&characteristic.id, time_bytes)
            .await?)
    }

    /// Get the temperature unit which the sensor uses for its display.
    pub async fn get_temperature_unit(&self, id: &DeviceId) -> Result<TemperatureUnit, MijiaError> {
        let characteristic = self
            .bt_session
            .get_service_characteristic_by_uuid(
                id,
                SERVICE_UUID,
                TEMPERATURE_UNIT_CHARACTERISTIC_UUID,
            )
            .await?;
        let value = self
            .bt_session
            .read_characteristic_value(&characteristic.id)
            .await?;
        Ok(TemperatureUnit::decode(&value)?)
    }

    /// Set the temperature unit which the sensor uses for its display.
    pub async fn set_temperature_unit(
        &self,
        id: &DeviceId,
        unit: TemperatureUnit,
    ) -> Result<(), BluetoothError> {
        let characteristic = self
            .bt_session
            .get_service_characteristic_by_uuid(
                id,
                SERVICE_UUID,
                TEMPERATURE_UNIT_CHARACTERISTIC_UUID,
            )
            .await?;
        self.bt_session
            .write_characteristic_value(&characteristic.id, unit.encode())
            .await
    }

    /// Get the comfort level configuration which determines when the sensor displays a happy face.
    pub async fn get_comfort_level(&self, id: &DeviceId) -> Result<ComfortLevel, MijiaError> {
        let characteristic = self
            .bt_session
            .get_service_characteristic_by_uuid(id, SERVICE_UUID, COMFORT_LEVEL_CHARACTERISTIC_UUID)
            .await?;
        let value = self
            .bt_session
            .read_characteristic_value(&characteristic.id)
            .await?;
        Ok(ComfortLevel::decode(&value)?)
    }

    /// Set the comfort level configuration which determines when the sensor displays a happy face.
    pub async fn set_comfort_level(
        &self,
        id: &DeviceId,
        comfort_level: &ComfortLevel,
    ) -> Result<(), MijiaError> {
        let characteristic = self
            .bt_session
            .get_service_characteristic_by_uuid(id, SERVICE_UUID, COMFORT_LEVEL_CHARACTERISTIC_UUID)
            .await?;
        Ok(self
            .bt_session
            .write_characteristic_value(&characteristic.id, comfort_level.encode()?)
            .await?)
    }

    /// Get the range of indices for historical data stored on the sensor.
    pub async fn get_history_range(&self, id: &DeviceId) -> Result<Range<u32>, MijiaError> {
        let characteristic = self
            .bt_session
            .get_service_characteristic_by_uuid(id, SERVICE_UUID, HISTORY_RANGE_CHARACTERISTIC_UUID)
            .await?;
        let value = self
            .bt_session
            .read_characteristic_value(&characteristic.id)
            .await?;
        Ok(decode_range(&value)?)
    }

    /// Delete all historical data stored on the sensor.
    pub async fn delete_history(&self, id: &DeviceId) -> Result<(), BluetoothError> {
        let characteristic = self
            .bt_session
            .get_service_characteristic_by_uuid(
                id,
                SERVICE_UUID,
                HISTORY_DELETE_CHARACTERISTIC_UUID,
            )
            .await?;
        self.bt_session
            .write_characteristic_value(&characteristic.id, HISTORY_DELETE_VALUE)
            .await
    }

    /// Get the last historical record stored on the sensor.
    pub async fn get_last_history_record(
        &self,
        id: &DeviceId,
    ) -> Result<HistoryRecord, MijiaError> {
        let characteristic = self
            .bt_session
            .get_service_characteristic_by_uuid(
                id,
                SERVICE_UUID,
                HISTORY_LAST_RECORD_CHARACTERISTIC_UUID,
            )
            .await?;
        let value = self
            .bt_session
            .read_characteristic_value(&characteristic.id)
            .await?;
        Ok(HistoryRecord::decode(&value)?)
    }

    /// Start receiving historical records from the sensor.
    ///
    /// # Arguments
    /// * `id`: The ID of the sensor to request records from.
    /// * `start_index`: The record index to start at. If this is not specified then all records
    ///   which have not yet been received from the sensor since it was connected will be requested.
    pub async fn start_notify_history(
        &self,
        id: &DeviceId,
        start_index: Option<u32>,
    ) -> Result<(), BluetoothError> {
        let service = self
            .bt_session
            .get_service_by_uuid(id, SERVICE_UUID)
            .await?;
        let history_records_characteristic = self
            .bt_session
            .get_characteristic_by_uuid(&service.id, HISTORY_RECORDS_CHARACTERISTIC_UUID)
            .await?;
        if let Some(start_index) = start_index {
            let history_index_characteristic = self
                .bt_session
                .get_characteristic_by_uuid(&service.id, HISTORY_INDEX_CHARACTERISTIC_UUID)
                .await?;
            self.bt_session
                .write_characteristic_value(
                    &history_index_characteristic.id,
                    start_index.to_le_bytes(),
                )
                .await?
        }
        self.bt_session
            .start_notify(&history_records_characteristic.id)
            .await
    }

    /// Stop receiving historical records from the sensor.
    pub async fn stop_notify_history(&self, id: &DeviceId) -> Result<(), BluetoothError> {
        let characteristic = self
            .bt_session
            .get_service_characteristic_by_uuid(
                id,
                SERVICE_UUID,
                HISTORY_RECORDS_CHARACTERISTIC_UUID,
            )
            .await?;
        self.bt_session.stop_notify(&characteristic.id).await
    }

    /// Try to get all historical records for the sensor.
    pub async fn get_all_history(
        &self,
        id: &DeviceId,
    ) -> Result<Vec<Option<HistoryRecord>>, MijiaError> {
        let history_range = self.get_history_range(id).await?;

        let history_record_characteristic = self
            .bt_session
            .get_service_characteristic_by_uuid(
                id,
                SERVICE_UUID,
                HISTORY_RECORDS_CHARACTERISTIC_UUID,
            )
            .await?;
        let events = self
            .bt_session
            .characteristic_event_stream(&history_record_characteristic.id)
            .await?;
        let events = events.timeout(HISTORY_RECORD_TIMEOUT);
        pin!(events);
        self.start_notify_history(id, Some(0)).await?;

        let mut history = vec![None; history_range.len()];
        while let Some(Ok(event)) = events.next().await {
            if let BluetoothEvent::Characteristic {
                id: record_id,
                event: CharacteristicEvent::Value { value },
            } = event
            {
                let record = HistoryRecord::decode(&value)?;
                log::trace!("{record_id:?}: {record}");
                if record_id == history_record_characteristic.id {
                    if history_range.contains(&record.index) {
                        let offset = record.index - history_range.start;
                        history[offset as usize] = Some(record);
                    } else {
                        log::error!(
                            "Got record {record:?} for sensor {id:?} out of bounds {history_range:?}"
                        );
                    }
                } else {
                    log::warn!("Got record for wrong characteristic {record_id:?}");
                }
            } else {
                log::warn!("Unexpected event: {event:?}");
            }
        }

        self.stop_notify_history(id).await?;

        Ok(history)
    }

    /// Assuming that the given device ID refers to a Mijia sensor device and that it has already
    /// been connected, subscribe to notifications of temperature/humidity readings, and adjust the
    /// connection interval to save power.
    ///
    /// Notifications will be delivered as events by `MijiaSession::event_stream()`.
    pub async fn start_notify_sensor(&self, id: &DeviceId) -> Result<(), BluetoothError> {
        let service = self
            .bt_session
            .get_service_by_uuid(id, SERVICE_UUID)
            .await?;
        let sensor_reading_characteristic = self
            .bt_session
            .get_characteristic_by_uuid(&service.id, SENSOR_READING_CHARACTERISTIC_UUID)
            .await?;
        let connection_interval_characteristic = self
            .bt_session
            .get_characteristic_by_uuid(&service.id, CONNECTION_INTERVAL_CHARACTERISTIC_UUID)
            .await?;
        self.bt_session
            .start_notify(&sensor_reading_characteristic.id)
            .await?;
        self.bt_session
            .write_characteristic_value(
                &connection_interval_characteristic.id,
                CONNECTION_INTERVAL_500_MS,
            )
            .await?;
        Ok(())
    }

    /// Get a stream of reading/history/disconnected events for all sensors.
    pub async fn event_stream(&self) -> Result<impl Stream<Item = MijiaEvent>, BluetoothError> {
        let events = self.bt_session.event_stream().await?;
        let session = self.bt_session.clone();
        Ok(Box::pin(futures::stream::StreamExt::filter_map(
            events,
            move |event| MijiaEvent::from(event, session.clone()),
        )))
    }
}

/// Check whether the given Bluetooth device is a Mijia sensor which we support.
fn is_mijia_sensor(device: &DeviceInfo) -> bool {
    device.name.as_deref() == Some(MIJIA_NAME)
}
