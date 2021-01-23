use dbus::Path;
use std::fmt::{self, Display, Formatter};
use uuid::Uuid;

use crate::DeviceId;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_device() {
        let device_id = DeviceId::new("/org/bluez/hci0/dev_11_22_33_44_55_66");
        let service_id = ServiceId::new("/org/bluez/hci0/dev_11_22_33_44_55_66/service0022");
        assert_eq!(service_id.device(), device_id);
    }
}
