use crate::bluetooth_event::BluetoothEvent;
use crate::introspect::Node;
use crate::DBUS_METHOD_CALL_TIMEOUT;
use bluez_generated::{
    OrgBluezAdapter1, OrgBluezDevice1, OrgBluezGattCharacteristic1, OrgBluezGattService1,
};
use core::fmt::Debug;
use core::future::Future;
use dbus::arg::{RefArg, Variant};
use dbus::nonblock::stdintf::org_freedesktop_dbus::{Introspectable, ObjectManager, Properties};
use dbus::nonblock::MsgMatch;
use dbus::nonblock::{Proxy, SyncConnection};
use dbus::Message;
use futures::channel::mpsc::UnboundedReceiver;
use futures::{FutureExt, Stream};
use itertools::Itertools;
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use std::task::{Context, Poll};
use thiserror::Error;
use tokio::stream::StreamExt;
use tokio::task::JoinError;
use tokio_compat_02::FutureExt as _;

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
}

/// Error type for futures representing tasks spawned by this crate.
#[derive(Error, Debug)]
pub enum SpawnError {
    #[error("D-Bus connection lost: {0}")]
    DbusConnectionLost(#[source] Box<dyn Error + Send + Sync>),
    #[error("Task failed: {0}")]
    Join(#[from] JoinError),
}

/// Opaque identifier for a Bluetooth device which the system knows about. This includes a reference
/// to which Bluetooth adapter it was discovered on, which means that any attempt to connect to it
/// will also happen from that adapter (in case the system has more than one).
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DeviceId {
    pub(crate) object_path: String,
}

impl DeviceId {
    pub(crate) fn new(object_path: &str) -> Self {
        Self {
            object_path: object_path.to_owned(),
        }
    }

    /// Get the ID of the Bluetooth adapter on which this device was discovered, e.g. `"hci0"`.
    pub fn adapter(&self) -> &str {
        self.object_path.split('/').nth(3).unwrap()
    }
}

/// Opaque identifier for a GATT service on a Bluetooth device.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ServiceId {
    pub(crate) object_path: String,
}

impl ServiceId {
    pub(crate) fn new(object_path: &str) -> Self {
        Self {
            object_path: object_path.to_owned(),
        }
    }

    /// Get the ID of the device on which this service was advertised.
    pub fn device(&self) -> DeviceId {
        let index = self
            .object_path
            .rfind('/')
            .expect("ServiceId object_path doesn't contain a slash.");
        DeviceId::new(&self.object_path[0..index])
    }
}

/// Opaque identifier for a GATT characteristic on a Bluetooth device.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CharacteristicId {
    pub(crate) object_path: String,
}

impl CharacteristicId {
    #[cfg(test)]
    pub(crate) fn new(object_path: &str) -> Self {
        Self {
            object_path: object_path.to_owned(),
        }
    }

    /// Get the ID of the service on which this characteristic was advertised.
    pub fn service(&self) -> ServiceId {
        let index = self
            .object_path
            .rfind('/')
            .expect("CharacteristicId object_path doesn't contain a slash.");
        ServiceId::new(&self.object_path[0..index])
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
    pub uuid: String,
    pub primary: bool,
}

/// Information about a GATT characteristic on a Bluetooth device.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CharacteristicInfo {
    /// An opaque identifier for the characteristic on the device, including a reference to which
    /// adapter it was discovered on.
    pub id: CharacteristicId,
    pub uuid: String,
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
                let name = device_properties.get("Name").map(|name| {
                    name.as_iter()
                        .unwrap()
                        .filter_map(|addr| addr.as_str())
                        .next()
                        .unwrap()
                        .to_string()
                });
                let service_data = get_service_data(device_properties).unwrap_or_default();

                Some(DeviceInfo {
                    id: DeviceId {
                        object_path: path.to_string(),
                    },
                    mac_address: MacAddress(mac_address),
                    name,
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
        let introspection_xml = self.device(device).introspect().compat().await?;
        let device_node: Node = serde_xml_rs::from_str(&introspection_xml)?;
        let mut services = vec![];
        for subnode in device_node.nodes {
            let subnode_name = subnode.name.as_ref().unwrap();
            if subnode_name.starts_with("service") {
                let service_id = ServiceId {
                    object_path: format!("{}/{}", device.object_path, subnode_name),
                };
                let service = self.service(&service_id);
                let uuid = service.uuid().compat().await?;
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
        let introspection_xml = self.service(service).introspect().compat().await?;
        let service_node: Node = serde_xml_rs::from_str(&introspection_xml)?;
        let mut characteristics = vec![];
        for subnode in service_node.nodes {
            let subnode_name = subnode.name.as_ref().unwrap();
            if subnode_name.starts_with("char") {
                let characteristic_id = CharacteristicId {
                    object_path: format!("{}/{}", service.object_path, subnode_name),
                };
                let uuid = self
                    .characteristic(&characteristic_id)
                    .uuid()
                    .compat()
                    .await?;
                characteristics.push(CharacteristicInfo {
                    id: characteristic_id,
                    uuid,
                });
            }
        }
        Ok(characteristics)
    }

    /// Find a GATT service with the given UUID advertised by the given device, if any.
    ///
    /// Note that this generally won't work until the device is connected.
    pub async fn get_service_by_uuid(
        &self,
        device: &DeviceId,
        uuid: &str,
    ) -> Result<Option<ServiceInfo>, BluetoothError> {
        let services = self.get_services(device).await?;
        Ok(services
            .into_iter()
            .find(|service_info| service_info.uuid == uuid))
    }

    /// Find a characteristic with the given UUID as part of the given GATT service advertised by a
    /// device, if there is any.
    pub async fn get_characteristic_by_uuid(
        &self,
        service: &ServiceId,
        uuid: &str,
    ) -> Result<Option<CharacteristicInfo>, BluetoothError> {
        let characteristics = self.get_characteristics(service).await?;
        Ok(characteristics
            .into_iter()
            .find(|characteristic_info| characteristic_info.uuid == uuid))
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

    /// Connect to the Bluetooth device with the given D-Bus object path.
    pub async fn connect(&self, id: &DeviceId) -> Result<(), BluetoothError> {
        Ok(self.device(id).connect().compat().await?)
    }

    /// Disconnect from the Bluetooth device with the given D-Bus object path.
    pub async fn disconnect(&self, id: &DeviceId) -> Result<(), BluetoothError> {
        Ok(self.device(id).disconnect().compat().await?)
    }

    // TODO: Change this to lookup the path from the UUIDs instead.
    /// Read the value of the characteristic of the given device with the given path. The path
    /// should be of the form "/service0001/char0002".
    pub(crate) async fn read_characteristic_value(
        &self,
        id: &DeviceId,
        characteristic_path: &str,
    ) -> Result<Vec<u8>, BluetoothError> {
        let characteristic = self.get_characteristic_proxy(id, characteristic_path);
        Ok(characteristic.read_value(HashMap::new()).compat().await?)
    }

    // TODO: Change this to lookup the path from the UUIDs instead.
    /// Write the given value to the characteristic of the given device with the given path. The
    /// path should be of the form "/service0001/char0002".
    pub(crate) async fn write_characteristic_value(
        &self,
        id: &DeviceId,
        characteristic_path: &str,
        value: impl Into<Vec<u8>>,
    ) -> Result<(), BluetoothError> {
        let characteristic = self.get_characteristic_proxy(id, characteristic_path);
        Ok(characteristic
            .write_value(value.into(), HashMap::new())
            .compat()
            .await?)
    }

    /// Start notifications on the characteristic of the given device with the given path. The path
    /// should be of the form "/service0001/char0002".
    pub(crate) async fn start_notify(
        &self,
        id: &DeviceId,
        characteristic_path: &str,
    ) -> Result<(), BluetoothError> {
        let characteristic = self.get_characteristic_proxy(id, characteristic_path);
        characteristic.start_notify().compat().await?;
        Ok(())
    }

    /// Stop notifications on the characteristic of the given device with the given path. The path
    /// should be of the form "/service0001/char0002".
    pub(crate) async fn stop_notify(
        &self,
        id: &DeviceId,
        characteristic_path: &str,
    ) -> Result<(), BluetoothError> {
        let characteristic = self.get_characteristic_proxy(id, characteristic_path);
        characteristic.stop_notify().compat().await?;
        Ok(())
    }

    fn get_characteristic_proxy(
        &self,
        id: &DeviceId,
        characteristic_path: &str,
    ) -> impl OrgBluezGattCharacteristic1 + Introspectable + Properties {
        let characteristic_id = CharacteristicId {
            object_path: id.object_path.to_string() + characteristic_path,
        };
        self.characteristic(&characteristic_id)
    }

    /// Get a stream of events for all devices.
    pub async fn event_stream(&self) -> Result<impl Stream<Item = BluetoothEvent>, BluetoothError> {
        let mut rule = dbus::message::MatchRule::new();
        rule.msg_type = Some(dbus::message::MessageType::Signal);
        // BusName validation just checks that the length and format is valid, so it should never
        // fail for a constant that we know is valid.
        rule.sender = Some(dbus::strings::BusName::new("org.bluez").unwrap());

        let msg_match = self.connection.add_match(rule).compat().await?;
        let event_stream = EventStream::new(msg_match, self.connection.clone());
        Ok(event_stream.filter_map(BluetoothEvent::from))
    }
}

/// Wrapper for a stream of D-Bus events which automatically removes the `MsgMatch` from the D-Bus
/// connection when it is dropped.
struct EventStream {
    msg_match: Option<MsgMatch>,
    events: UnboundedReceiver<Message>,
    connection: Arc<SyncConnection>,
}

impl EventStream {
    fn new(msg_match: MsgMatch, connection: Arc<SyncConnection>) -> Self {
        let (msg_match, events) = msg_match.msg_stream();
        Self {
            msg_match: Some(msg_match),
            events,
            connection,
        }
    }
}

impl Stream for EventStream {
    type Item = Message;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.events).poll_next(cx)
    }
}

impl Drop for EventStream {
    fn drop(&mut self) {
        let connection = self.connection.clone();
        let msg_match = self.msg_match.take().unwrap();
        tokio::spawn(async move {
            connection
                .remove_match(msg_match.token())
                .compat()
                .await
                .unwrap()
        });
    }
}

fn get_service_data(
    device_properties: &HashMap<String, Variant<Box<dyn RefArg>>>,
) -> Option<HashMap<String, Vec<u8>>> {
    // UUIDs don't get populated until we connect. Use:
    //     "ServiceData": Variant(InternalDict { data: [
    //         ("0000fe95-0000-1000-8000-00805f9b34fb", Variant([48, 88, 91, 5, 1, 23, 33, 215, 56, 193, 164, 40, 1, 0])
    //     )], outer_sig: Signature("a{sv}") })
    // instead.
    Some(
        device_properties
            .get("ServiceData")?
            // Variant(...)
            .as_iter()?
            .next()?
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

    #[test]
    fn device_adapter() {
        let device_id = DeviceId::new("/org/bluez/hci0/dev_11_22_33_44_55_66");
        assert_eq!(device_id.adapter(), "hci0");
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
}
