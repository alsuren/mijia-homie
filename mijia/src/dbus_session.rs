use bluez_generated::OrgBluezAdapter1;
use bluez_generated::OrgBluezDevice1;
use bluez_generated::OrgBluezGattCharacteristic1;
use dbus::arg::RefArg;
use dbus::arg::Variant;
use dbus::nonblock::stdintf::org_freedesktop_dbus::ObjectManager;
use dbus::nonblock::SyncConnection;
use std::collections::HashMap;
use std::{sync::Arc, time::Duration};

/// Convenience wrappers around low-level dbus details.
#[cfg_attr(test, faux::create)]
#[derive(Clone)]
pub struct DBusSession {
    connection: Arc<SyncConnection>,
    timeout: Duration,
    address: &'static str,
}

#[cfg_attr(test, faux::methods)]
impl DBusSession {
    pub(crate) fn new(
        connection: Arc<SyncConnection>,
        timeout: Duration,
        address: &'static str,
    ) -> Self {
        Self {
            connection,
            timeout,
            address,
        }
    }

    pub(crate) fn connection(&self) -> Arc<SyncConnection> {
        self.connection.clone()
    }

    pub(crate) async fn get_managed_objects(
        &self,
    ) -> Result<
        HashMap<
            dbus::Path<'static>,
            HashMap<String, HashMap<String, Variant<Box<dyn RefArg + 'static>>>>,
        >,
        dbus::Error,
    > {
        let root = dbus::nonblock::Proxy::new(self.address, "/", self.timeout, self.connection());
        root.get_managed_objects().await
    }

    pub(crate) async fn start_notify(&self, path: &str) -> Result<(), dbus::Error> {
        let proxy = dbus::nonblock::Proxy::new(self.address, path, self.timeout, self.connection());
        proxy.start_notify().await
    }

    pub(crate) fn write_value(
        &self,
        path: &str,
        value: Vec<u8>,
        options: ::std::collections::HashMap<
            &'static str,
            dbus::arg::Variant<Box<dyn dbus::arg::RefArg>>,
        >,
    ) -> dbus::nonblock::MethodReply<()> {
        let proxy = dbus::nonblock::Proxy::new(self.address, path, self.timeout, self.connection());
        proxy.write_value(value, options)
    }

    pub(crate) fn set_powered(&self, path: &str, value: bool) -> dbus::nonblock::MethodReply<()> {
        let proxy = dbus::nonblock::Proxy::new(self.address, path, self.timeout, self.connection());
        proxy.set_powered(value)
    }

    pub(crate) fn start_discovery(&self, path: &str) -> dbus::nonblock::MethodReply<()> {
        let proxy = dbus::nonblock::Proxy::new(self.address, path, self.timeout, self.connection());
        proxy.start_discovery()
    }

    pub(crate) fn connect(&self, path: &str) -> dbus::nonblock::MethodReply<()> {
        let proxy = dbus::nonblock::Proxy::new(self.address, path, self.timeout, self.connection());
        proxy.connect()
    }

    pub(crate) fn disconnect(&self, path: &str) -> dbus::nonblock::MethodReply<()> {
        let proxy = dbus::nonblock::Proxy::new(self.address, path, self.timeout, self.connection());
        proxy.disconnect()
    }
}
