mod bleuuid;
mod events;
mod introspect;
mod messagestream;

pub use self::bleuuid::{uuid_from_u16, uuid_from_u32, BleUuid};
pub use self::events::{AdapterEvent, BluetoothEvent, CharacteristicEvent, DeviceEvent};
use self::introspect::IntrospectParse;
use self::messagestream::MessageStream;
use bitflags::bitflags;
use bluez_generated::{
    OrgBluezAdapter1, OrgBluezDevice1, OrgBluezDevice1Properties, OrgBluezGattCharacteristic1,
    OrgBluezGattDescriptor1, OrgBluezGattService1,
};
use dbus::arg::RefArg;
use dbus::nonblock::stdintf::org_freedesktop_dbus::{Introspectable, ObjectManager, Properties};
use dbus::nonblock::{Proxy, SyncConnection};
use dbus::Path;
use futures::stream::{self, select_all, StreamExt};
use futures::{FutureExt, Stream};
use itertools::Itertools;
use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};
use std::future::Future;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::task::JoinError;
use tokio_compat_02::FutureExt as _;
use uuid::Uuid;

const DBUS_METHOD_CALL_TIMEOUT: Duration = Duration::from_secs(30);

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
}

/// Error type for futures representing tasks spawned by this crate.
#[derive(Error, Debug)]
pub enum SpawnError {
    #[error("D-Bus connection lost: {0}")]
    DbusConnectionLost(#[source] Box<dyn Error + Send + Sync>),
    #[error("Task failed: {0}")]
    Join(#[from] JoinError),
}

/// Opaque identifier for a Bluetooth adapter on the system.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AdapterId {
    pub(crate) object_path: Path<'static>,
}

impl AdapterId {
    pub(crate) fn new(object_path: &str) -> Self {
        Self {
            object_path: object_path.to_owned().into(),
        }
    }
}

impl Display for AdapterId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.object_path
                .to_string()
                .strip_prefix("/org/bluez/")
                .ok_or(fmt::Error)?
        )
    }
}

/// Opaque identifier for a Bluetooth device which the system knows about. This includes a reference
/// to which Bluetooth adapter it was discovered on, which means that any attempt to connect to it
/// will also happen from that adapter (in case the system has more than one).
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DeviceId {
    pub(crate) object_path: Path<'static>,
}

impl DeviceId {
    pub(crate) fn new(object_path: &str) -> Self {
        Self {
            object_path: object_path.to_owned().into(),
        }
    }

    /// Get the ID of the Bluetooth adapter on which this device was discovered, e.g. `"hci0"`.
    pub fn adapter(&self) -> AdapterId {
        let index = self
            .object_path
            .rfind('/')
            .expect("DeviceId object_path must contain a slash.");
        AdapterId::new(&self.object_path[0..index])
    }
}

impl From<DeviceId> for Path<'static> {
    fn from(id: DeviceId) -> Self {
        id.object_path
    }
}

impl Display for DeviceId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.object_path
                .to_string()
                .strip_prefix("/org/bluez/")
                .ok_or(fmt::Error)?
        )
    }
}

/// Opaque identifier for a GATT service on a Bluetooth device.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ServiceId {
    pub(crate) object_path: Path<'static>,
}

impl ServiceId {
    pub(crate) fn new(object_path: &str) -> Self {
        Self {
            object_path: object_path.to_owned().into(),
        }
    }

    /// Get the ID of the device on which this service was advertised.
    pub fn device(&self) -> DeviceId {
        let index = self
            .object_path
            .rfind('/')
            .expect("ServiceId object_path must contain a slash.");
        DeviceId::new(&self.object_path[0..index])
    }
}

impl From<ServiceId> for Path<'static> {
    fn from(id: ServiceId) -> Self {
        id.object_path
    }
}

impl Display for ServiceId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.object_path
                .to_string()
                .strip_prefix("/org/bluez/")
                .ok_or(fmt::Error)?
        )
    }
}

/// Opaque identifier for a GATT characteristic on a Bluetooth device.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CharacteristicId {
    pub(crate) object_path: Path<'static>,
}

impl CharacteristicId {
    pub(crate) fn new(object_path: &str) -> Self {
        Self {
            object_path: object_path.to_owned().into(),
        }
    }

    /// Get the ID of the service on which this characteristic was advertised.
    pub fn service(&self) -> ServiceId {
        let index = self
            .object_path
            .rfind('/')
            .expect("CharacteristicId object_path must contain a slash.");
        ServiceId::new(&self.object_path[0..index])
    }
}

impl From<CharacteristicId> for Path<'static> {
    fn from(id: CharacteristicId) -> Self {
        id.object_path
    }
}

impl Display for CharacteristicId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.object_path
                .to_string()
                .strip_prefix("/org/bluez/")
                .ok_or(fmt::Error)?
        )
    }
}

/// Opaque identifier for a GATT characteristic descriptor on a Bluetooth device.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DescriptorId {
    pub(crate) object_path: Path<'static>,
}

impl DescriptorId {
    #[cfg(test)]
    pub(crate) fn new(object_path: &str) -> Self {
        Self {
            object_path: object_path.to_owned().into(),
        }
    }

    /// Get the ID of the characteristic on which this descriptor was advertised.
    pub fn characteristic(&self) -> CharacteristicId {
        let index = self
            .object_path
            .rfind('/')
            .expect("DescriptorId object_path must contain a slash.");
        CharacteristicId::new(&self.object_path[0..index])
    }
}

impl From<DescriptorId> for Path<'static> {
    fn from(id: DescriptorId) -> Self {
        id.object_path
    }
}

impl Display for DescriptorId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.object_path
                .to_string()
                .strip_prefix("/org/bluez/")
                .ok_or(fmt::Error)?
        )
    }
}

/// MAC address of a Bluetooth device.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MacAddress(String);

impl Display for MacAddress {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// An error parsing a MAC address from a string.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("Invalid MAC address")]
pub struct ParseMacAddressError();

impl FromStr for MacAddress {
    type Err = ParseMacAddressError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let octets: Vec<_> = s.split(':').collect();
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

/// Information about a Bluetooth device which was discovered.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceInfo {
    /// An opaque identifier for the device, including a reference to which adapter it was
    /// discovered on. This can be used to connect to it.
    pub id: DeviceId,
    /// The MAC address of the device.
    pub mac_address: MacAddress,
    /// The human-readable name of the device, if available.
    pub name: Option<String>,
    /// The GATT service data from the device's advertisement, if any. This is a map from the
    /// service UUID to its data.
    pub service_data: HashMap<String, Vec<u8>>,
}

/// Information about a GATT service on a Bluetooth device.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceInfo {
    /// An opaque identifier for the service on the device, including a reference to which adapter
    /// it was discovered on.
    pub id: ServiceId,
    /// The 128-bit UUID of the service.
    pub uuid: Uuid,
    /// Whether this GATT service is a primary service.
    pub primary: bool,
}

/// Information about a GATT characteristic on a Bluetooth device.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CharacteristicInfo {
    /// An opaque identifier for the characteristic on the device, including a reference to which
    /// adapter it was discovered on.
    pub id: CharacteristicId,
    /// The 128-bit UUID of the characteristic.
    pub uuid: Uuid,
    /// The set of flags (a.k.a. properties) of the characteristic, defining how the characteristic
    /// can be used.
    pub flags: CharacteristicFlags,
}

bitflags! {
    /// The set of flags (a.k.a. properties) of a characteristic, defining how the characteristic
    /// can be used.
    pub struct CharacteristicFlags: u16 {
        const BROADCAST = 0x01;
        const READ = 0x02;
        const WRITE_WITHOUT_RESPONSE = 0x04;
        const WRITE = 0x08;
        const NOTIFY = 0x10;
        const INDICATE = 0x20;
        const SIGNED_WRITE = 0x40;
        const EXTENDED_PROPERTIES = 0x80;
        const RELIABLE_WRITE = 0x100;
        const WRITABLE_AUXILIARIES = 0x200;
        const ENCRYPT_READ = 0x400;
        const ENCRYPT_WRITE = 0x800;
        const ENCRYPT_AUTHENTICATED_READ = 0x1000;
        const ENCRYPT_AUTHENTICATED_WRITE = 0x2000;
        const AUTHORIZE = 0x4000;
    }
}

impl TryFrom<Vec<String>> for CharacteristicFlags {
    type Error = BluetoothError;

    fn try_from(value: Vec<String>) -> Result<Self, BluetoothError> {
        let mut flags = Self::empty();
        for flag_string in value {
            let flag = match flag_string.as_ref() {
                "broadcast" => Self::BROADCAST,
                "read" => Self::READ,
                "write-without-response" => Self::WRITE_WITHOUT_RESPONSE,
                "write" => Self::WRITE,
                "notify" => Self::NOTIFY,
                "indicate" => Self::INDICATE,
                "authenticated-signed-write" => Self::SIGNED_WRITE,
                "extended-properties" => Self::EXTENDED_PROPERTIES,
                "reliable-write" => Self::RELIABLE_WRITE,
                "writable-auxiliaries" => Self::WRITABLE_AUXILIARIES,
                "encrypt-read" => Self::ENCRYPT_READ,
                "encrypt-write" => Self::ENCRYPT_WRITE,
                "encrypt-authenticated-read" => Self::ENCRYPT_AUTHENTICATED_READ,
                "encrypt-authenticated-write" => Self::ENCRYPT_AUTHENTICATED_WRITE,
                "authorize" => Self::AUTHORIZE,
                _ => return Err(BluetoothError::FlagParseError(flag_string)),
            };
            flags.insert(flag);
        }
        Ok(flags)
    }
}

/// Information about a GATT descriptor on a Bluetooth device.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DescriptorInfo {
    /// An opaque identifier for the descriptor on the device, including a reference to which
    /// adapter it was discovered on.
    pub id: DescriptorId,
    /// The 128-bit UUID of the descriptor.
    pub uuid: Uuid,
}

/// A connection to the Bluetooth daemon. This can be cheaply cloned and passed around to be used
/// from different places.
#[derive(Clone)]
pub struct BluetoothSession {
    pub connection: Arc<SyncConnection>,
}

impl Debug for BluetoothSession {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "BluetoothSession")
    }
}

impl BluetoothSession {
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
            let err = dbus_resource.compat().await;
            Err(SpawnError::DbusConnectionLost(err))
        });
        Ok((
            dbus_handle.map(|res| Ok(res??)),
            BluetoothSession { connection },
        ))
    }

    /// Power on all Bluetooth adapters and start scanning for devices.
    pub async fn start_discovery(&self) -> Result<(), BluetoothError> {
        let bluez_root = Proxy::new(
            "org.bluez",
            "/",
            DBUS_METHOD_CALL_TIMEOUT,
            self.connection.clone(),
        );
        let tree = bluez_root.get_managed_objects().compat().await?;
        let adapters: Vec<_> = tree
            .into_iter()
            .filter_map(|(path, interfaces)| interfaces.get("org.bluez.Adapter1").map(|_| path))
            .collect();

        if adapters.is_empty() {
            return Err(BluetoothError::NoBluetoothAdapters);
        }

        for path in adapters {
            log::trace!("Starting discovery on adapter {}", path);
            let adapter = Proxy::new(
                "org.bluez",
                path,
                DBUS_METHOD_CALL_TIMEOUT,
                self.connection.clone(),
            );
            adapter.set_powered(true).compat().await?;
            adapter
                .start_discovery()
                .compat()
                .await
                .unwrap_or_else(|err| println!("starting discovery failed {:?}", err));
        }
        Ok(())
    }

    /// Get a list of all Bluetooth devices which have been discovered so far.
    pub async fn get_devices(&self) -> Result<Vec<DeviceInfo>, BluetoothError> {
        let bluez_root = Proxy::new(
            "org.bluez",
            "/",
            DBUS_METHOD_CALL_TIMEOUT,
            self.connection.clone(),
        );
        let tree = bluez_root.get_managed_objects().compat().await?;

        let sensors = tree
            .into_iter()
            .filter_map(|(object_path, interfaces)| {
                let device_properties = OrgBluezDevice1Properties::from_interfaces(&interfaces)?;

                let mac_address = device_properties.address()?;
                let name = device_properties.name();
                let service_data = get_service_data(device_properties).unwrap_or_default();

                Some(DeviceInfo {
                    id: DeviceId { object_path },
                    mac_address: MacAddress(mac_address.to_owned()),
                    name: name.cloned(),
                    service_data,
                })
            })
            .collect();
        Ok(sensors)
    }

    /// Get a list of all GATT services which the given Bluetooth device offers.
    ///
    /// Note that this won't be filled in until the device is connected.
    pub async fn get_services(
        &self,
        device: &DeviceId,
    ) -> Result<Vec<ServiceInfo>, BluetoothError> {
        let device_node = self.device(device).introspect_parse().compat().await?;
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
                let uuid = Uuid::parse_str(&service.uuid().compat().await?)?;
                let primary = service.primary().compat().await?;
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
        let service_node = self.service(service).introspect_parse().compat().await?;
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
                let uuid = Uuid::parse_str(&characteristic.uuid().compat().await?)?;
                let flags = characteristic.flags().compat().await?;
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
            .compat()
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
                let uuid =
                    Uuid::parse_str(&self.descriptor(&descriptor_id).uuid().compat().await?)?;
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

    /// Get information about the given GATT service.
    pub async fn get_service_info(&self, id: &ServiceId) -> Result<ServiceInfo, BluetoothError> {
        let service = self.service(&id);
        let uuid = Uuid::parse_str(&service.uuid().compat().await?)?;
        let primary = service.primary().compat().await?;
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
        let uuid = Uuid::parse_str(&characteristic.uuid().compat().await?)?;
        let flags = characteristic.flags().compat().await?;
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
        let uuid = Uuid::parse_str(&self.descriptor(&id).uuid().compat().await?)?;
        Ok(DescriptorInfo {
            id: id.to_owned(),
            uuid,
        })
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

    /// Connect to the Bluetooth device with the given D-Bus object path.
    pub async fn connect(&self, id: &DeviceId) -> Result<(), BluetoothError> {
        Ok(self.device(id).connect().compat().await?)
    }

    /// Disconnect from the Bluetooth device with the given D-Bus object path.
    pub async fn disconnect(&self, id: &DeviceId) -> Result<(), BluetoothError> {
        Ok(self.device(id).disconnect().compat().await?)
    }

    /// Read the value of the given GATT characteristic.
    pub async fn read_characteristic_value(
        &self,
        id: &CharacteristicId,
    ) -> Result<Vec<u8>, BluetoothError> {
        let characteristic = self.characteristic(id);
        Ok(characteristic.read_value(HashMap::new()).compat().await?)
    }

    /// Write the given value to the given GATT characteristic.
    pub async fn write_characteristic_value(
        &self,
        id: &CharacteristicId,
        value: impl Into<Vec<u8>>,
    ) -> Result<(), BluetoothError> {
        let characteristic = self.characteristic(id);
        Ok(characteristic
            .write_value(value.into(), HashMap::new())
            .compat()
            .await?)
    }

    /// Read the value of the given GATT descriptor.
    pub async fn read_descriptor_value(
        &self,
        id: &DescriptorId,
    ) -> Result<Vec<u8>, BluetoothError> {
        let descriptor = self.descriptor(id);
        Ok(descriptor.read_value(HashMap::new()).compat().await?)
    }

    /// Write the given value to the given GATT descriptor.
    pub async fn write_descriptor_value(
        &self,
        id: &DescriptorId,
        value: impl Into<Vec<u8>>,
    ) -> Result<(), BluetoothError> {
        let descriptor = self.descriptor(id);
        Ok(descriptor
            .write_value(value.into(), HashMap::new())
            .compat()
            .await?)
    }

    /// Start notifications on the given GATT characteristic.
    pub async fn start_notify(&self, id: &CharacteristicId) -> Result<(), BluetoothError> {
        let characteristic = self.characteristic(id);
        characteristic.start_notify().compat().await?;
        Ok(())
    }

    /// Stop notifications on the given GATT characteristic.
    pub async fn stop_notify(&self, id: &CharacteristicId) -> Result<(), BluetoothError> {
        let characteristic = self.characteristic(id);
        characteristic.stop_notify().compat().await?;
        Ok(())
    }

    /// Get a stream of events for all devices.
    pub async fn event_stream(&self) -> Result<impl Stream<Item = BluetoothEvent>, BluetoothError> {
        self.filtered_event_stream(None::<&DeviceId>).await
    }

    /// Get a stream of events for a particular device.
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
            let msg_match = self.connection.add_match(match_rule).compat().await?;
            message_streams.push(MessageStream::new(msg_match, self.connection.clone()));
        }
        Ok(select_all(message_streams)
            .flat_map(|message| stream::iter(BluetoothEvent::message_to_events(message))))
    }
}

fn get_service_data(
    device_properties: OrgBluezDevice1Properties,
) -> Option<HashMap<String, Vec<u8>>> {
    // UUIDs don't get populated until we connect. Use:
    //     "ServiceData": Variant(InternalDict { data: [
    //         ("0000fe95-0000-1000-8000-00805f9b34fb", Variant([48, 88, 91, 5, 1, 23, 33, 215, 56, 193, 164, 40, 1, 0])
    //     )], outer_sig: Signature("a{sv}") })
    // instead.
    Some(
        device_properties
            .0
            .get("ServiceData")?
            // Variant(...)
            .0
            // InternalDict(...)
            .as_iter()?
            .tuples::<(_, _)>()
            .filter_map(|(k, v)| {
                let k = k.as_str()?.into();
                let v: Option<Vec<u8>> = v
                    .box_clone()
                    .as_static_inner(0)?
                    .as_iter()?
                    .map(|el| Some(el.as_u64()? as u8))
                    .collect();
                let v = v?;
                Some((k, v))
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use dbus::arg::Variant;

    #[test]
    fn device_adapter() {
        let adapter_id = AdapterId::new("/org/bluez/hci0");
        let device_id = DeviceId::new("/org/bluez/hci0/dev_11_22_33_44_55_66");
        assert_eq!(device_id.adapter(), adapter_id);
    }

    #[test]
    fn service_device() {
        let device_id = DeviceId::new("/org/bluez/hci0/dev_11_22_33_44_55_66");
        let service_id = ServiceId::new("/org/bluez/hci0/dev_11_22_33_44_55_66/service0022");
        assert_eq!(service_id.device(), device_id);
    }

    #[test]
    fn characteristic_service() {
        let service_id = ServiceId::new("/org/bluez/hci0/dev_11_22_33_44_55_66/service0022");
        let characteristic_id =
            CharacteristicId::new("/org/bluez/hci0/dev_11_22_33_44_55_66/service0022/char0033");
        assert_eq!(characteristic_id.service(), service_id);
    }

    #[test]
    fn descriptor_characteristic() {
        let characteristic_id =
            CharacteristicId::new("/org/bluez/hci0/dev_11_22_33_44_55_66/service0022/char0033");
        let descriptor_id = DescriptorId::new(
            "/org/bluez/hci0/dev_11_22_33_44_55_66/service0022/char0033/desc0034",
        );
        assert_eq!(descriptor_id.characteristic(), characteristic_id);
    }

    #[test]
    fn parse_flags() {
        let flags: CharacteristicFlags = vec!["read".to_string(), "encrypt-write".to_string()]
            .try_into()
            .unwrap();
        assert_eq!(
            flags,
            CharacteristicFlags::READ | CharacteristicFlags::ENCRYPT_WRITE
        )
    }

    #[test]
    fn parse_flags_fail() {
        let flags: Result<CharacteristicFlags, BluetoothError> =
            vec!["read".to_string(), "invalid flag".to_string()].try_into();
        assert!(
            matches!(flags, Err(BluetoothError::FlagParseError(string)) if string == "invalid flag")
        );
    }

    #[test]
    fn service_data() {
        let mut service_data: HashMap<String, Variant<Box<dyn RefArg>>> = HashMap::new();
        service_data.insert("uuid".to_string(), Variant(Box::new(vec![1u8, 2, 3])));
        let mut device_properties: HashMap<String, Variant<Box<dyn RefArg>>> = HashMap::new();
        device_properties.insert("ServiceData".to_string(), Variant(Box::new(service_data)));

        let mut expected_service_data = HashMap::new();
        expected_service_data.insert("uuid".to_string(), vec![1u8, 2, 3]);

        assert_eq!(
            get_service_data(OrgBluezDevice1Properties(&device_properties)),
            Some(expected_service_data)
        );
    }
}
