use crate::dbus_session::DBusSession;
use crate::DBUS_METHOD_CALL_TIMEOUT;
use bluez_generated::{OrgBluezAdapter1, OrgBluezDevice1};
use core::fmt::Debug;
use core::future::Future;
use dbus::arg::{RefArg, Variant};
use dbus::nonblock::SyncConnection;
use eyre::{bail, WrapErr};
use futures::FutureExt;
use itertools::Itertools;
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;
use std::sync::Arc;

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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseMacAddressError();

impl Display for ParseMacAddressError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
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

#[cfg(test)]
impl DeviceInfo {
    fn fake_with_name(i: usize, name: &str) -> Self {
        Self {
            // This isn't actually valid, but is good for debugging.
            mac_address: MacAddress(format!("{}", i)),
            name: Some(name.to_string()),
            id: DeviceId {
                object_path: "obviously rubbish".to_string(),
            },
            service_data: HashMap::new(),
        }
    }
}

/// A connection to the Bluetooth daemon. This can be cheaply cloned and passed around to be used
/// from different places.
#[cfg_attr(test, faux::create)]
#[derive(Clone)]
pub struct BluetoothSession {
    dbus: DBusSession,
}

impl Debug for BluetoothSession {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "BluetoothSession")
    }
}

// ::new() cannot be mocked because Faux doesn't undertand how to rewrite the
// complex return type that involves Self.
impl BluetoothSession {
    /// Returns a tuple of (join handle, Self).
    /// If the join handle ever completes then you're in trouble and should
    /// probably restart the process.
    pub async fn new() -> Result<(impl Future<Output = Result<(), eyre::Error>>, Self), eyre::Error>
    {
        // Connect to the D-Bus system bus (this is blocking, unfortunately).
        let (dbus_resource, connection) = dbus_tokio::connection::new_system_sync()?;
        // The resource is a task that should be spawned onto a tokio compatible
        // reactor ASAP. If the resource ever finishes, you lost connection to D-Bus.
        let dbus_handle = tokio::spawn(async {
            let err = dbus_resource.await;
            // TODO: work out why this err isn't 'static and use eyre::Error::new instead
            Err::<(), eyre::Error>(eyre::eyre!(err))
        });
        Ok((
            dbus_handle.map(|res| Ok(res??)),
            BluetoothSession::from_dbus(DBusSession::new(connection, DBUS_METHOD_CALL_TIMEOUT)),
        ))
    }
}

#[cfg_attr(test, faux::methods)]
impl BluetoothSession {
    fn from_dbus(dbus: DBusSession) -> Self {
        Self { dbus }
    }

    pub fn connection(&self) -> Arc<SyncConnection> {
        self.dbus.connection()
    }

    /// Power on all Bluetooth adapters and start scanning for devices.
    pub async fn start_discovery(&self) -> Result<(), eyre::Error> {
        let tree = self.dbus.get_managed_objects("org.bluez").await?;
        let adapters: Vec<_> = tree
            .into_iter()
            .filter_map(|(path, interfaces)| interfaces.get("org.bluez.Adapter1").map(|_| path))
            .collect();

        if adapters.is_empty() {
            bail!("No Bluetooth adapters found.");
        }

        for path in adapters {
            log::trace!("Starting discovery on adapter {}", path);
            let adapter = dbus::nonblock::Proxy::new(
                "org.bluez",
                path,
                DBUS_METHOD_CALL_TIMEOUT,
                self.dbus.connection(),
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
    pub async fn get_devices(&self) -> Result<Vec<DeviceInfo>, eyre::Error> {
        let tree = self.dbus.get_managed_objects("org.bluez").await?;

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
        dbus::nonblock::Proxy::new(
            "org.bluez",
            id.object_path.to_owned(),
            DBUS_METHOD_CALL_TIMEOUT,
            self.dbus.connection(),
        )
    }

    /// Connect to the Bluetooth device with the given D-Bus object path.
    pub async fn connect(&self, id: &DeviceId) -> Result<(), eyre::Error> {
        self.device(id)
            .connect()
            .await
            .wrap_err_with(|| format!("connecting to {:?}", id))
    }

    /// Disconnect from the Bluetooth device with the given D-Bus object path.
    pub async fn disconnect(&self, id: &DeviceId) -> Result<(), eyre::Error> {
        self.device(id)
            .disconnect()
            .await
            .wrap_err_with(|| format!("disconnecting from {:?}", id))
    }
}

#[cfg(test)]
impl BluetoothSession {
    pub fn fake_with_device_names(names: &[&str]) -> Self {
        let devices: Vec<_> = names
            .into_iter()
            .enumerate()
            .map(|(i, s)| DeviceInfo::fake_with_name(i, *s))
            .collect();
        let mut bt_session = BluetoothSession::faux();
        faux::when!(bt_session.get_devices).safe_then(move |_| Ok(devices.clone()));
        bt_session
    }
}

// TESTME: This really wants testing, if we can construct the appropriate Variants.
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
