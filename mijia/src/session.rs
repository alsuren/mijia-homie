use crate::DBUS_METHOD_CALL_TIMEOUT;
use anyhow::Context;
use bluez_generated::generated::{OrgBluezAdapter1, OrgBluezDevice1};
use core::fmt::Debug;
use core::future::Future;
use dbus::arg::cast;
use dbus::arg::RefArg;
use dbus::nonblock::stdintf::org_freedesktop_dbus::ObjectManager;
use dbus::nonblock::SyncConnection;
use futures::FutureExt;
use itertools::Itertools;
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;
use std::sync::Arc;

/// Opaque identifier for a bluetooth device which the system knows about.
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

/// MAC address of a bluetooth device.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MacAddress(String);

impl Display for MacAddress {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseMacAddressError();

impl Display for ParseMacAddressError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
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

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub id: DeviceId,
    pub mac_address: MacAddress,
    pub service_data: HashMap<String, Vec<u8>>,
}

#[derive(Clone)]
pub struct BluetoothSession {
    pub connection: Arc<SyncConnection>,
}

impl Debug for BluetoothSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BluetoothSession")
    }
}

impl BluetoothSession {
    /// Returns a tuple of (join handle, Self).
    /// If the join handle ever completes then you're in trouble and should
    /// probably restart the process.
    pub async fn new(
    ) -> Result<(impl Future<Output = Result<(), anyhow::Error>>, Self), anyhow::Error> {
        // Connect to the D-Bus system bus (this is blocking, unfortunately).
        let (dbus_resource, connection) = dbus_tokio::connection::new_system_sync()?;
        // The resource is a task that should be spawned onto a tokio compatible
        // reactor ASAP. If the resource ever finishes, you lost connection to D-Bus.
        let dbus_handle = tokio::spawn(async {
            let err = dbus_resource.await;
            // TODO: work out why this err isn't 'static and use anyhow::Error::new instead
            Err::<(), anyhow::Error>(anyhow::anyhow!(err))
        });
        Ok((
            dbus_handle.map(|res| Ok(res??)),
            BluetoothSession { connection },
        ))
    }

    /// Power on the bluetooth adapter adapter and start scanning for devices.
    pub async fn start_discovery(&self) -> Result<(), anyhow::Error> {
        let adapter = dbus::nonblock::Proxy::new(
            "org.bluez",
            "/org/bluez/hci0",
            DBUS_METHOD_CALL_TIMEOUT,
            self.connection.clone(),
        );
        adapter
            .set_powered(true)
            .await
            .with_context(|| std::line!().to_string())?;
        adapter
            .start_discovery()
            .await
            .unwrap_or_else(|err| println!("starting discovery failed {:?}", err));
        Ok(())
    }

    /// Get a list of all bluetooth devices which have been discovered so far.
    pub async fn get_devices(&self) -> Result<Vec<DeviceInfo>, anyhow::Error> {
        let bluez_root = dbus::nonblock::Proxy::new(
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
                // UUIDs don't get populated until we connect. Use:
                //     "ServiceData": Variant(InternalDict { data: [
                //         ("0000fe95-0000-1000-8000-00805f9b34fb", Variant([48, 88, 91, 5, 1, 23, 33, 215, 56, 193, 164, 40, 1, 0])
                //     )], outer_sig: Signature("a{sv}") })
                // instead.
                let service_data: HashMap<String, Vec<u8>> = device_properties
                    .get("ServiceData")?
                    // Variant(...)
                    .as_iter()?
                    .next()?
                    // InternalDict(...)
                    .as_iter()?
                    .tuples::<(_, _)>()
                    .filter_map(|(k, v)| {
                        let k = k.as_str()?.into();
                        let v = v.box_clone();
                        let v = cast::<Vec<u8>>(&v)?.clone();
                        Some((k, v))
                    })
                    .collect();

                Some(DeviceInfo {
                    id: DeviceId {
                        object_path: path.to_string(),
                    },
                    mac_address: MacAddress(mac_address),
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
            self.connection.clone(),
        )
    }

    /// Connect to the bluetooth device with the given D-Bus object path.
    pub async fn connect(&self, id: &DeviceId) -> Result<(), anyhow::Error> {
        self.device(id)
            .connect()
            .await
            .with_context(|| std::line!().to_string())
    }

    /// Disconnect from the bluetooth device with the given D-Bus object path.
    pub async fn disconnect(&self, id: &DeviceId) -> Result<(), anyhow::Error> {
        self.device(id)
            .disconnect()
            .await
            .with_context(|| std::line!().to_string())
    }
}
