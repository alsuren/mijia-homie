//! An async wrapper around the D-Bus interface of BlueZ, the Linux Bluetooth daemon. This provides
//! type-safe interfaces to a subset of the Bluetooth client (i.e. central, in Bluetooth
//! terminology) interfaces exposed by BlueZ, focussing on the Generic Attribute Profile (GATT) of
//! Bluetooth Low Energy (BLE).
//!
//! Start by creating a [`BluetoothSession`].
//!
//! [`BluetoothSession']: struct.BluetoothSession.html

mod adapter;
mod bleuuid;
mod characteristic;
mod descriptor;
mod device;
mod events;
mod introspect;
mod macaddress;
mod messagestream;
mod service;

pub use self::adapter::{AdapterId, AdapterInfo};
pub use self::bleuuid::{uuid_from_u16, uuid_from_u32, BleUuid};
pub use self::characteristic::{CharacteristicFlags, CharacteristicId, CharacteristicInfo};
pub use self::descriptor::{DescriptorId, DescriptorInfo};
pub use self::device::{AddressType, DeviceId, DeviceInfo};
pub use self::events::{AdapterEvent, BluetoothEvent, CharacteristicEvent, DeviceEvent};
use self::introspect::IntrospectParse;
pub use self::macaddress::{MacAddress, ParseMacAddressError};
use self::messagestream::MessageStream;
pub use self::service::{ServiceId, ServiceInfo};
use bluez_generated::{
    OrgBluezAdapter1, OrgBluezAdapter1Properties, OrgBluezDevice1, OrgBluezDevice1Properties,
    OrgBluezGattCharacteristic1, OrgBluezGattDescriptor1, OrgBluezGattService1,
    ORG_BLUEZ_ADAPTER1_NAME, ORG_BLUEZ_DEVICE1_NAME,
};
use dbus::arg::{PropMap, Variant};
use dbus::nonblock::stdintf::org_freedesktop_dbus::{Introspectable, ObjectManager, Properties};
use dbus::nonblock::{Proxy, SyncConnection};
use dbus::Path;
use dbus_tokio::connection::IOResourceError;
use futures::stream::{self, select_all, StreamExt};
use futures::{FutureExt, Stream};
use std::collections::HashMap;
use std::convert::TryInto;
use std::fmt::{self, Debug, Display, Formatter};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::task::JoinError;
use tokio::time::timeout;
use uuid::Uuid;

const DBUS_METHOD_CALL_TIMEOUT: Duration = Duration::from_secs(30);
const SERVICE_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);

/// An error carrying out a Bluetooth operation.
#[derive(Debug, Error)]
pub enum BluetoothError {
    /// No Bluetooth adapters were found on the system.
    #[error("No Bluetooth adapters found.")]
    NoBluetoothAdapters,
    /// There was an error talking to the BlueZ daemon over D-Bus.
    #[error(transparent)]
    DbusError(#[from] dbus::Error),
    /// Error parsing XML for introspection.
    #[error("Error parsing XML for introspection: {0}")]
    XmlParseError(#[from] serde_xml_rs::Error),
    /// No service or characteristic was found for some UUID.
    #[error("Service or characteristic UUID {uuid} not found.")]
    UUIDNotFound { uuid: Uuid },
    /// Error parsing a UUID from a string.
    #[error("Error parsing UUID string: {0}")]
    UUIDParseError(#[from] uuid::Error),
    /// Error parsing a characteristic flag from a string.
    #[error("Invalid characteristic flag {0:?}")]
    FlagParseError(String),
    /// Error parsing an `AddressType` from a string.
    #[error("Invalid address type {0}")]
    AddressTypeParseError(String),
    /// A required property of some device or other object was not found.
    #[error("Required property {0} missing.")]
    RequiredPropertyMissing(&'static str),
    /// Service discovery didn't happen within the time limit.
    #[error("Service discovery timed out")]
    ServiceDiscoveryTimedOut,
    /// Error parsing a `MacAddress` from a string.
    #[error(transparent)]
    MacAddressParseError(#[from] ParseMacAddressError),
}

/// Error type for futures representing tasks spawned by this crate.
#[derive(Debug, Error)]
pub enum SpawnError {
    #[error("D-Bus connection lost: {0}")]
    DbusConnectionLost(#[source] IOResourceError),
    #[error("Task failed: {0}")]
    Join(#[from] JoinError),
}

/// The type of transport to use for a scan.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Transport {
    /// Interleaved scan, both BLE and Bluetooth Classic (if they are both enabled on the adapter).
    Auto,
    /// BR/EDR inquiry, i.e. Bluetooth Classic.
    BrEdr,
    /// LE scan only.
    Le,
}

impl Transport {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::BrEdr => "bredr",
            Self::Le => "le",
        }
    }
}

impl Display for Transport {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A set of filter parameters for discovery. Parameters may be set to `None` to use the BlueZ
/// defaults.
///
/// If no parameters are set then there is a default filter on the RSSI values, where only values
/// which have changed more than a certain amount will be reported.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DiscoveryFilter {
    /// If non-empty, only report devices which advertise at least one of these service UUIDs.
    pub service_uuids: Vec<Uuid>,
    /// Only report devices with RSSI values greater than the given threshold.
    pub rssi_threshold: Option<i16>,
    pub pathloss_threshold: Option<u16>,
    /// The type of scan.
    pub transport: Option<Transport>,
    /// Whether to include duplicate advertisements. If this is set to true then there will be an
    /// event whenever an advertisement containing manufacturer-specific data for a device is
    /// received.
    pub duplicate_data: Option<bool>,
    /// Whether to make the adapter discoverable while discovering.
    pub discoverable: Option<bool>,
    /// Only report devices whose address or name starts with the given pattern.
    pub pattern: Option<String>,
}

impl Into<PropMap> for &DiscoveryFilter {
    fn into(self) -> PropMap {
        let mut map: PropMap = HashMap::new();
        if !self.service_uuids.is_empty() {
            let uuids: Vec<String> = self.service_uuids.iter().map(Uuid::to_string).collect();
            map.insert("UUIDs".to_string(), Variant(Box::new(uuids)));
        }
        if let Some(rssi_threshold) = self.rssi_threshold {
            map.insert("RSSI".to_string(), Variant(Box::new(rssi_threshold)));
        }
        if let Some(pathloss_threshold) = self.pathloss_threshold {
            map.insert(
                "Pathloss".to_string(),
                Variant(Box::new(pathloss_threshold)),
            );
        }
        if let Some(transport) = self.transport {
            map.insert(
                "Transport".to_string(),
                Variant(Box::new(transport.to_string())),
            );
        }
        if let Some(duplicate_data) = self.duplicate_data {
            map.insert(
                "DuplicateData".to_string(),
                Variant(Box::new(duplicate_data)),
            );
        }
        if let Some(discoverable) = self.discoverable {
            map.insert("Discoverable".to_string(), Variant(Box::new(discoverable)));
        }
        if let Some(pattern) = &self.pattern {
            map.insert("Pattern".to_string(), Variant(Box::new(pattern.to_owned())));
        }
        map
    }
}

/// The type of write operation to use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteType {
    /// A write operation where the device is expected to respond with a confirmation or error. Also
    /// known as a request. This corresponds to
    /// [`CharacteristicFlags`](struct.CharacteristicFlags.html)`::WRITE`.
    WithResponse,
    /// A write-without-response, also known as a command. This corresponds to
    /// [`CharacteristicFlags`](struct.CharacteristicFlags.html)`::WRITE_WITHOUT_RESPONSE`.
    WithoutResponse,
    /// A reliable write. This corresponds to
    /// [`CharacteristicFlags`](struct.CharacteristicFlags.html)`::RELIABLE_WRITE`.
    Reliable,
}

impl WriteType {
    fn as_str(&self) -> &'static str {
        match self {
            Self::WithResponse => "request",
            Self::WithoutResponse => "command",
            Self::Reliable => "reliable",
        }
    }
}

impl Display for WriteType {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A set of options for a characteristic write operation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WriteOptions {
    /// The starting offset of the write operation.
    pub offset: usize,
    /// The type of write operation to use.
    ///
    /// Note that a given characteristic may not support all different types of write operations;
    /// you can check with
    /// [`CharacteristicInfo.flags`](struct.CharacteristicInfo.html#structfield.flags). The type of
    /// write operation will also affect the maximum possible length of data which can be written in
    /// a single operation.
    pub write_type: Option<WriteType>,
}

impl Into<PropMap> for WriteOptions {
    fn into(self) -> PropMap {
        let mut map: PropMap = HashMap::new();
        if self.offset != 0 {
            map.insert("offset".to_string(), Variant(Box::new(self.offset as u64)));
        }
        if let Some(write_type) = self.write_type {
            map.insert(
                "type".to_string(),
                Variant(Box::new(write_type.to_string())),
            );
        }
        map
    }
}

/// A connection to the Bluetooth daemon. This can be cheaply cloned and passed around to be used
/// from different places. It is the main entry point to the library.
#[derive(Clone)]
pub struct BluetoothSession {
    connection: Arc<SyncConnection>,
}

impl Debug for BluetoothSession {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "BluetoothSession")
    }
}

impl BluetoothSession {
    /// Establish a new D-Bus connection to communicate with BlueZ.
    ///
    /// Returns a tuple of (join handle, Self).
    /// If the join handle ever completes then you're in trouble and should
    /// probably restart the process.
    pub async fn new(
    ) -> Result<(impl Future<Output = Result<(), SpawnError>>, Self), BluetoothError> {
        // Connect to the D-Bus system bus (this is blocking, unfortunately).
        let (dbus_resource, connection) = dbus_tokio::connection::new_system_sync()?;
        // The resource is a task that should be spawned onto a tokio compatible
        // reactor ASAP. If the resource ever finishes, you lost connection to D-Bus.
        let dbus_handle = tokio::spawn(async {
            let err = dbus_resource.await;
            Err(SpawnError::DbusConnectionLost(err))
        });
        Ok((
            dbus_handle.map(|res| Ok(res??)),
            BluetoothSession { connection },
        ))
    }

    /// Power on all Bluetooth adapters, remove any discovery filter, and then start scanning for
    /// devices.
    ///
    /// This is equivalent to calling `start_discovery_with_filter(&DiscoveryFilter::default())`.
    pub async fn start_discovery(&self) -> Result<(), BluetoothError> {
        self.start_discovery_with_filter(&DiscoveryFilter::default())
            .await
    }

    /// Power on the given Bluetooth adapter, remove any discovery filter, and then start scanning
    /// for devices.
    ///
    /// This is equivalent to calling
    /// `start_discovery_on_adapter_with_filter(adapter, &DiscoveryFilter::default())`.
    pub async fn start_discovery_on_adapter(
        &self,
        adapter: &AdapterId,
    ) -> Result<(), BluetoothError> {
        self.start_discovery_on_adapter_with_filter(adapter, &DiscoveryFilter::default())
            .await
    }

    /// Power on all Bluetooth adapters, set the given discovery filter, and then start scanning for
    /// devices.
    ///
    /// Note that BlueZ combines discovery filters from all clients and sends events matching any
    /// filter to all clients, so you may receive unexpected discovery events if there are other
    /// clients on the system using Bluetooth as well.
    ///
    /// In most common cases, `DiscoveryFilter::default()` is fine.
    pub async fn start_discovery_with_filter(
        &self,
        discovery_filter: &DiscoveryFilter,
    ) -> Result<(), BluetoothError> {
        let adapters = self.get_adapters().await?;
        if adapters.is_empty() {
            return Err(BluetoothError::NoBluetoothAdapters);
        }

        for adapter in adapters {
            log::trace!("Starting discovery on adapter {}", adapter.id);
            self.start_discovery_on_adapter_with_filter(&adapter.id, discovery_filter)
                .await
                .unwrap_or_else(|err| println!("starting discovery failed {:?}", err));
        }
        Ok(())
    }

    /// Power on the given Bluetooth adapter, set the given discovery filter, and then start
    /// scanning for devices.
    ///
    /// Note that BlueZ combines discovery filters from all clients and sends events matching any
    /// filter to all clients, so you may receive unexpected discovery events if there are other
    /// clients on the system using Bluetooth as well.
    ///
    /// In most common cases, `DiscoveryFilter::default()` is fine.
    pub async fn start_discovery_on_adapter_with_filter(
        &self,
        adapter_id: &AdapterId,
        discovery_filter: &DiscoveryFilter,
    ) -> Result<(), BluetoothError> {
        let adapter = self.adapter(&adapter_id);
        adapter.set_powered(true).await?;
        adapter
            .set_discovery_filter(discovery_filter.into())
            .await?;
        adapter.start_discovery().await?;
        Ok(())
    }

    /// Stop scanning for devices on all Bluetooth adapters.
    pub async fn stop_discovery(&self) -> Result<(), BluetoothError> {
        let adapters = self.get_adapters().await?;
        if adapters.is_empty() {
            return Err(BluetoothError::NoBluetoothAdapters);
        }

        for adapter in adapters {
            self.stop_discovery_on_adapter(&adapter.id).await?;
        }

        Ok(())
    }

    /// Stop scanning for devices on the given Bluetooth adapters.
    pub async fn stop_discovery_on_adapter(
        &self,
        adapter_id: &AdapterId,
    ) -> Result<(), BluetoothError> {
        let adapter = self.adapter(&adapter_id);
        adapter.stop_discovery().await?;
        Ok(())
    }

    /// Get a list of all Bluetooth adapters on the system.
    pub async fn get_adapters(&self) -> Result<Vec<AdapterInfo>, BluetoothError> {
        let bluez_root = Proxy::new(
            "org.bluez",
            "/",
            DBUS_METHOD_CALL_TIMEOUT,
            self.connection.clone(),
        );
        // TODO: See whether there is a way to do this with introspection instead, rather than
        // getting lots of objects we don't care about.
        let tree = bluez_root.get_managed_objects().await?;
        Ok(tree
            .into_iter()
            .filter_map(|(object_path, interfaces)| {
                let adapter_properties = OrgBluezAdapter1Properties::from_interfaces(&interfaces)?;
                AdapterInfo::from_properties(AdapterId { object_path }, adapter_properties).ok()
            })
            .collect())
    }

    /// Get a list of all Bluetooth devices which have been discovered so far.
    pub async fn get_devices(&self) -> Result<Vec<DeviceInfo>, BluetoothError> {
        let bluez_root = Proxy::new(
            "org.bluez",
            "/",
            DBUS_METHOD_CALL_TIMEOUT,
            self.connection.clone(),
        );
        let tree = bluez_root.get_managed_objects().await?;

        let devices = tree
            .into_iter()
            .filter_map(|(object_path, interfaces)| {
                let device_properties = OrgBluezDevice1Properties::from_interfaces(&interfaces)?;
                DeviceInfo::from_properties(DeviceId { object_path }, device_properties).ok()
            })
            .collect();
        Ok(devices)
    }

    /// Get a list of all Bluetooth devices which have been discovered so far on a given adapter.
    pub async fn get_devices_on_adapter(
        &self,
        adapter: &AdapterId,
    ) -> Result<Vec<DeviceInfo>, BluetoothError> {
        let devices = self.get_devices().await?;
        Ok(devices
            .into_iter()
            .filter(|device| device.id.adapter() == *adapter)
            .collect())
    }

    /// Get a list of all GATT services which the given Bluetooth device offers.
    ///
    /// Note that this won't be filled in until the device is connected.
    pub async fn get_services(
        &self,
        device: &DeviceId,
    ) -> Result<Vec<ServiceInfo>, BluetoothError> {
        let device_node = self.device(device).introspect_parse().await?;
        let mut services = vec![];
        for subnode in device_node.nodes {
            let subnode_name = subnode.name.as_ref().unwrap();
            // Service paths are always of the form
            // /org/bluez/{hci0,hci1,...}/dev_XX_XX_XX_XX_XX_XX/serviceXXXX
            if subnode_name.starts_with("service") {
                let service_id = ServiceId {
                    object_path: format!("{}/{}", device.object_path, subnode_name).into(),
                };
                let service = self.service(&service_id);
                let uuid = Uuid::parse_str(&service.uuid().await?)?;
                let primary = service.primary().await?;
                services.push(ServiceInfo {
                    id: service_id,
                    uuid,
                    primary,
                });
            }
        }
        Ok(services)
    }

    /// Get a list of all characteristics on the given GATT service.
    pub async fn get_characteristics(
        &self,
        service: &ServiceId,
    ) -> Result<Vec<CharacteristicInfo>, BluetoothError> {
        let service_node = self.service(service).introspect_parse().await?;
        let mut characteristics = vec![];
        for subnode in service_node.nodes {
            let subnode_name = subnode.name.as_ref().unwrap();
            // Characteristic paths are always of the form
            // /org/bluez/{hci0,hci1,...}/dev_XX_XX_XX_XX_XX_XX/serviceXXXX/charYYYY
            if subnode_name.starts_with("char") {
                let characteristic_id = CharacteristicId {
                    object_path: format!("{}/{}", service.object_path, subnode_name).into(),
                };
                let characteristic = self.characteristic(&characteristic_id);
                let uuid = Uuid::parse_str(&characteristic.uuid().await?)?;
                let flags = characteristic.flags().await?;
                characteristics.push(CharacteristicInfo {
                    id: characteristic_id,
                    uuid,
                    flags: flags.try_into()?,
                });
            }
        }
        Ok(characteristics)
    }

    /// Get a list of all descriptors on the given GATT characteristic.
    pub async fn get_descriptors(
        &self,
        characteristic: &CharacteristicId,
    ) -> Result<Vec<DescriptorInfo>, BluetoothError> {
        let characteristic_node = self
            .characteristic(characteristic)
            .introspect_parse()
            .await?;
        let mut descriptors = vec![];
        for subnode in characteristic_node.nodes {
            let subnode_name = subnode.name.as_ref().unwrap();
            // Service paths are always of the form
            // /org/bluez/{hci0,hci1,...}/dev_XX_XX_XX_XX_XX_XX/serviceXXXX/charYYYY/descZZZZ
            if subnode_name.starts_with("desc") {
                let descriptor_id = DescriptorId {
                    object_path: format!("{}/{}", characteristic.object_path, subnode_name).into(),
                };
                let uuid = Uuid::parse_str(&self.descriptor(&descriptor_id).uuid().await?)?;
                descriptors.push(DescriptorInfo {
                    id: descriptor_id,
                    uuid,
                });
            }
        }
        Ok(descriptors)
    }

    /// Find a GATT service with the given UUID advertised by the given device, if any.
    ///
    /// Note that this generally won't work until the device is connected.
    pub async fn get_service_by_uuid(
        &self,
        device: &DeviceId,
        uuid: Uuid,
    ) -> Result<ServiceInfo, BluetoothError> {
        let services = self.get_services(device).await?;
        services
            .into_iter()
            .find(|service_info| service_info.uuid == uuid)
            .ok_or(BluetoothError::UUIDNotFound { uuid })
    }

    /// Find a characteristic with the given UUID as part of the given GATT service advertised by a
    /// device, if there is any.
    pub async fn get_characteristic_by_uuid(
        &self,
        service: &ServiceId,
        uuid: Uuid,
    ) -> Result<CharacteristicInfo, BluetoothError> {
        let characteristics = self.get_characteristics(service).await?;
        characteristics
            .into_iter()
            .find(|characteristic_info| characteristic_info.uuid == uuid)
            .ok_or(BluetoothError::UUIDNotFound { uuid })
    }

    /// Convenience method to get a GATT charactacteristic with the given UUID advertised by a
    /// device as part of the given service.
    ///
    /// This is equivalent to calling `get_service_by_uuid` and then `get_characteristic_by_uuid`.
    pub async fn get_service_characteristic_by_uuid(
        &self,
        device: &DeviceId,
        service_uuid: Uuid,
        characteristic_uuid: Uuid,
    ) -> Result<CharacteristicInfo, BluetoothError> {
        let service = self.get_service_by_uuid(device, service_uuid).await?;
        self.get_characteristic_by_uuid(&service.id, characteristic_uuid)
            .await
    }

    /// Get information about the given Bluetooth device.
    pub async fn get_device_info(&self, id: &DeviceId) -> Result<DeviceInfo, BluetoothError> {
        let device = self.device(&id);
        let properties = device.get_all(ORG_BLUEZ_DEVICE1_NAME).await?;
        DeviceInfo::from_properties(id.to_owned(), OrgBluezDevice1Properties(&properties))
    }

    /// Get information about the given Bluetooth adapter.
    pub async fn get_adapter_info(&self, id: &AdapterId) -> Result<AdapterInfo, BluetoothError> {
        let adapter = self.adapter(&id);
        let properties = adapter.get_all(ORG_BLUEZ_ADAPTER1_NAME).await?;
        AdapterInfo::from_properties(id.to_owned(), OrgBluezAdapter1Properties(&properties))
    }

    /// Get information about the given GATT service.
    pub async fn get_service_info(&self, id: &ServiceId) -> Result<ServiceInfo, BluetoothError> {
        let service = self.service(&id);
        let uuid = Uuid::parse_str(&service.uuid().await?)?;
        let primary = service.primary().await?;
        Ok(ServiceInfo {
            id: id.to_owned(),
            uuid,
            primary,
        })
    }

    /// Get information about the given GATT characteristic.
    pub async fn get_characteristic_info(
        &self,
        id: &CharacteristicId,
    ) -> Result<CharacteristicInfo, BluetoothError> {
        let characteristic = self.characteristic(&id);
        let uuid = Uuid::parse_str(&characteristic.uuid().await?)?;
        let flags = characteristic.flags().await?;
        Ok(CharacteristicInfo {
            id: id.to_owned(),
            uuid,
            flags: flags.try_into()?,
        })
    }

    /// Get information about the given GATT descriptor.
    pub async fn get_descriptor_info(
        &self,
        id: &DescriptorId,
    ) -> Result<DescriptorInfo, BluetoothError> {
        let uuid = Uuid::parse_str(&self.descriptor(&id).uuid().await?)?;
        Ok(DescriptorInfo {
            id: id.to_owned(),
            uuid,
        })
    }

    fn adapter(&self, id: &AdapterId) -> impl OrgBluezAdapter1 + Introspectable + Properties {
        Proxy::new(
            "org.bluez",
            id.object_path.to_owned(),
            DBUS_METHOD_CALL_TIMEOUT,
            self.connection.clone(),
        )
    }

    fn device(&self, id: &DeviceId) -> impl OrgBluezDevice1 + Introspectable + Properties {
        Proxy::new(
            "org.bluez",
            id.object_path.to_owned(),
            DBUS_METHOD_CALL_TIMEOUT,
            self.connection.clone(),
        )
    }

    fn service(&self, id: &ServiceId) -> impl OrgBluezGattService1 + Introspectable + Properties {
        Proxy::new(
            "org.bluez",
            id.object_path.to_owned(),
            DBUS_METHOD_CALL_TIMEOUT,
            self.connection.clone(),
        )
    }

    fn characteristic(
        &self,
        id: &CharacteristicId,
    ) -> impl OrgBluezGattCharacteristic1 + Introspectable + Properties {
        Proxy::new(
            "org.bluez",
            id.object_path.to_owned(),
            DBUS_METHOD_CALL_TIMEOUT,
            self.connection.clone(),
        )
    }

    fn descriptor(
        &self,
        id: &DescriptorId,
    ) -> impl OrgBluezGattDescriptor1 + Introspectable + Properties {
        Proxy::new(
            "org.bluez",
            id.object_path.to_owned(),
            DBUS_METHOD_CALL_TIMEOUT,
            self.connection.clone(),
        )
    }

    /// Wait until the services of the given device have been resolved.
    async fn await_service_discovery(&self, device_id: &DeviceId) -> Result<(), BluetoothError> {
        // We need to subscribe to events before checking current value to avoid a race condition.
        let mut events = self.device_event_stream(device_id).await?;
        if self.device(device_id).services_resolved().await? {
            log::info!("Services already resolved.");
            return Ok(());
        }
        timeout(SERVICE_DISCOVERY_TIMEOUT, async {
            while let Some(event) = events.next().await {
                if matches!(event, BluetoothEvent::Device {
                    id,
                    event: DeviceEvent::ServicesResolved,
                } if device_id == &id)
                {
                    return Ok(());
                }
            }

            // Stream ended prematurely. This shouldn't happen, so something has gone wrong.
            Err(BluetoothError::ServiceDiscoveryTimedOut)
        })
        .await
        .unwrap_or(Err(BluetoothError::ServiceDiscoveryTimedOut))
    }

    /// Connect to the given Bluetooth device.
    pub async fn connect(&self, id: &DeviceId) -> Result<(), BluetoothError> {
        self.device(id).connect().await?;
        self.await_service_discovery(id).await
    }

    /// Disconnect from the given Bluetooth device.
    pub async fn disconnect(&self, id: &DeviceId) -> Result<(), BluetoothError> {
        Ok(self.device(id).disconnect().await?)
    }

    /// Read the value of the given GATT characteristic.
    ///
    /// This is equivalent to calling `read_characteristic_value_with_offset(0)`.
    pub async fn read_characteristic_value(
        &self,
        id: &CharacteristicId,
    ) -> Result<Vec<u8>, BluetoothError> {
        self.read_characteristic_value_with_offset(id, 0).await
    }

    /// Read the value of the given GATT characteristic, starting from the given offset.
    pub async fn read_characteristic_value_with_offset(
        &self,
        id: &CharacteristicId,
        offset: usize,
    ) -> Result<Vec<u8>, BluetoothError> {
        let characteristic = self.characteristic(id);
        Ok(characteristic.read_value(offset_to_propmap(offset)).await?)
    }

    /// Write the given value to the given GATT characteristic, with default options.
    ///
    /// This is equivalent to calling `write_characteristic_value_with_options(WriteOptions::default())`.
    pub async fn write_characteristic_value(
        &self,
        id: &CharacteristicId,
        value: impl Into<Vec<u8>>,
    ) -> Result<(), BluetoothError> {
        self.write_characteristic_value_with_options(id, value, WriteOptions::default())
            .await
    }

    /// Write the given value to the given GATT characteristic, with the given options.
    pub async fn write_characteristic_value_with_options(
        &self,
        id: &CharacteristicId,
        value: impl Into<Vec<u8>>,
        options: WriteOptions,
    ) -> Result<(), BluetoothError> {
        let characteristic = self.characteristic(id);
        Ok(characteristic
            .write_value(value.into(), options.into())
            .await?)
    }

    /// Read the value of the given GATT descriptor.
    ///
    /// This is equivalent to calling `read_descriptor_value_with_offset(0)`.
    pub async fn read_descriptor_value(
        &self,
        id: &DescriptorId,
    ) -> Result<Vec<u8>, BluetoothError> {
        self.read_descriptor_value_with_offset(id, 0).await
    }

    /// Read the value of the given GATT descriptor, starting from the given offset.
    pub async fn read_descriptor_value_with_offset(
        &self,
        id: &DescriptorId,
        offset: usize,
    ) -> Result<Vec<u8>, BluetoothError> {
        let descriptor = self.descriptor(id);
        Ok(descriptor.read_value(offset_to_propmap(offset)).await?)
    }

    /// Write the given value to the given GATT descriptor.
    ///
    /// This is equivalent to calling `write_descriptor_value_with_offset(0)`.
    pub async fn write_descriptor_value(
        &self,
        id: &DescriptorId,
        value: impl Into<Vec<u8>>,
    ) -> Result<(), BluetoothError> {
        self.write_descriptor_value_with_offset(id, value, 0).await
    }

    /// Write the given value to the given GATT descriptor, starting from the given offset.
    pub async fn write_descriptor_value_with_offset(
        &self,
        id: &DescriptorId,
        value: impl Into<Vec<u8>>,
        offset: usize,
    ) -> Result<(), BluetoothError> {
        let descriptor = self.descriptor(id);
        Ok(descriptor
            .write_value(value.into(), offset_to_propmap(offset))
            .await?)
    }

    /// Start notifications on the given GATT characteristic.
    pub async fn start_notify(&self, id: &CharacteristicId) -> Result<(), BluetoothError> {
        let characteristic = self.characteristic(id);
        characteristic.start_notify().await?;
        Ok(())
    }

    /// Stop notifications on the given GATT characteristic.
    pub async fn stop_notify(&self, id: &CharacteristicId) -> Result<(), BluetoothError> {
        let characteristic = self.characteristic(id);
        characteristic.stop_notify().await?;
        Ok(())
    }

    /// Get a stream of events for all devices.
    pub async fn event_stream(&self) -> Result<impl Stream<Item = BluetoothEvent>, BluetoothError> {
        self.filtered_event_stream(None::<&DeviceId>).await
    }

    /// Get a stream of events for a particular device. This includes events for all its
    /// characteristics.
    pub async fn device_event_stream(
        &self,
        device: &DeviceId,
    ) -> Result<impl Stream<Item = BluetoothEvent>, BluetoothError> {
        self.filtered_event_stream(Some(device)).await
    }

    /// Get a stream of events for a particular characteristic of a device.
    pub async fn characteristic_event_stream(
        &self,
        characteristic: &CharacteristicId,
    ) -> Result<impl Stream<Item = BluetoothEvent>, BluetoothError> {
        self.filtered_event_stream(Some(characteristic)).await
    }

    async fn filtered_event_stream(
        &self,
        object: Option<&(impl Into<Path<'static>> + Clone)>,
    ) -> Result<impl Stream<Item = BluetoothEvent>, BluetoothError> {
        let mut message_streams = vec![];
        for match_rule in BluetoothEvent::match_rules(object.cloned()) {
            let msg_match = self.connection.add_match(match_rule).await?;
            message_streams.push(MessageStream::new(msg_match, self.connection.clone()));
        }
        Ok(select_all(message_streams)
            .flat_map(|message| stream::iter(BluetoothEvent::message_to_events(message))))
    }
}

fn offset_to_propmap(offset: usize) -> PropMap {
    let mut map: PropMap = HashMap::new();
    if offset != 0 {
        map.insert("offset".to_string(), Variant(Box::new(offset as u64)));
    }
    map
}
