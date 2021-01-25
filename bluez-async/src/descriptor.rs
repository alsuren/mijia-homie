use dbus::Path;
use std::fmt::{self, Display, Formatter};
use uuid::Uuid;

use crate::CharacteristicId;

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

/// Information about a GATT descriptor on a Bluetooth device.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DescriptorInfo {
    /// An opaque identifier for the descriptor on the device, including a reference to which
    /// adapter it was discovered on.
    pub id: DescriptorId,
    /// The 128-bit UUID of the descriptor.
    pub uuid: Uuid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_characteristic() {
        let characteristic_id =
            CharacteristicId::new("/org/bluez/hci0/dev_11_22_33_44_55_66/service0022/char0033");
        let descriptor_id = DescriptorId::new(
            "/org/bluez/hci0/dev_11_22_33_44_55_66/service0022/char0033/desc0034",
        );
        assert_eq!(descriptor_id.characteristic(), characteristic_id);
    }
}
