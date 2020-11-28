use crate::DBUS_METHOD_CALL_TIMEOUT;
use bluez_generated::{OrgBluezAdapter1, OrgBluezDevice1, OrgBluezGattCharacteristic1};
use core::fmt::Debug;
use core::future::Future;
use dbus::arg::{RefArg, Variant};
use dbus::nonblock::stdintf::org_freedesktop_dbus::ObjectManager;
use dbus::nonblock::{Proxy, SyncConnection};
use futures::FutureExt;
use itertools::Itertools;
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;
use std::sync::Arc;
use thiserror::Error;
use tokio::task::JoinError;

/// An error carrying out a Bluetooth operation.
#[derive(Debug, Error)]
pub enum BluetoothError {
    /// No Bluetooth adapters were found on the system.
    #[error("No Bluetooth adapters found.")]
    NoBluetoothAdapters,
    /// There was an error talking to the BlueZ daemon over D-Bus.
    #[error(transparent)]
    DbusError(#[from] dbus::Error),
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
#[derive(Clone, Debug)]
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
            let err = dbus_resource.await;
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
        let tree = bluez_root.get_managed_objects().await?;
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
            adapter.set_powered(true).await?;
            adapter
                .start_discovery()
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

    fn device(&self, id: &DeviceId) -> impl OrgBluezDevice1 {
        Proxy::new(
            "org.bluez",
            id.object_path.to_owned(),
            DBUS_METHOD_CALL_TIMEOUT,
            self.connection.clone(),
        )
    }

    /// Connect to the Bluetooth device with the given D-Bus object path.
    pub async fn connect(&self, id: &DeviceId) -> Result<(), BluetoothError> {
        Ok(self.device(id).connect().await?)
    }

    /// Disconnect from the Bluetooth device with the given D-Bus object path.
    pub async fn disconnect(&self, id: &DeviceId) -> Result<(), BluetoothError> {
        Ok(self.device(id).disconnect().await?)
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
        Ok(characteristic.read_value(HashMap::new()).await?)
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
        characteristic.start_notify().await?;
        Ok(())
    }

    fn get_characteristic_proxy(
        &self,
        id: &DeviceId,
        characteristic_path: &str,
    ) -> Proxy<Arc<SyncConnection>> {
        let full_path = id.object_path.to_string() + characteristic_path;
        Proxy::new(
            "org.bluez",
            full_path,
            DBUS_METHOD_CALL_TIMEOUT,
            self.connection.clone(),
        )
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
