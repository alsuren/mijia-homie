use dbus::arg::RefArg;
use dbus::arg::Variant;
use dbus::nonblock::stdintf::org_freedesktop_dbus::ObjectManager;
use dbus::nonblock::SyncConnection;
use std::collections::HashMap;
use std::{sync::Arc, time::Duration};

/// Convenience wrappers around low-level dbus details.
#[cfg_attr(test, faux::create)]
#[derive(Clone)]
pub(crate) struct DBusSession {
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
}
