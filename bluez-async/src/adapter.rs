use bluez_generated::OrgBluezAdapter1Properties;
use dbus::Path;
use std::fmt::{self, Display, Formatter};

use crate::{AddressType, BluetoothError, MacAddress};

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

/// Information about a Bluetooth adapter on the system.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterInfo {
    /// An opaque identifier for the adapter. This can be used to perform operations on it.
    pub id: AdapterId,
    /// The MAC address of the adapter.
    pub mac_address: MacAddress,
    /// The type of MAC address the adapter uses.
    pub address_type: AddressType,
    /// The Bluetooth system hostname.
    pub name: String,
    /// The Bluetooth friendly name. This defaults to the system hostname.
    pub alias: String,
    /// Whether the adapter is currently turned on.
    pub powered: bool,
    /// Whether the adapter is currently discovering devices.
    pub discovering: bool,
}

impl AdapterInfo {
    pub(crate) fn from_properties(
        id: AdapterId,
        adapter_properties: OrgBluezAdapter1Properties,
    ) -> Result<AdapterInfo, BluetoothError> {
        let mac_address = adapter_properties
            .address()
            .ok_or_else(|| BluetoothError::RequiredPropertyMissing("Address".to_string()))?;
        let address_type = adapter_properties
            .address_type()
            .ok_or_else(|| BluetoothError::RequiredPropertyMissing("AddressType".to_string()))?
            .parse()?;

        Ok(AdapterInfo {
            id,
            mac_address: MacAddress(mac_address.to_owned()),
            address_type,
            name: adapter_properties
                .name()
                .ok_or_else(|| BluetoothError::RequiredPropertyMissing("Name".to_string()))?
                .to_owned(),
            alias: adapter_properties
                .alias()
                .ok_or_else(|| BluetoothError::RequiredPropertyMissing("Alias".to_string()))?
                .to_owned(),
            powered: adapter_properties
                .powered()
                .ok_or_else(|| BluetoothError::RequiredPropertyMissing("Powered".to_string()))?,
            discovering: adapter_properties.discovering().ok_or_else(|| {
                BluetoothError::RequiredPropertyMissing("Discovering".to_string())
            })?,
        })
    }
}

#[cfg(test)]
mod tests {
    use dbus::arg::{RefArg, Variant};
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn adapter_info_minimal() {
        let id = AdapterId::new("/org/bluez/hci0");
        let mut adapter_properties: HashMap<String, Variant<Box<dyn RefArg>>> = HashMap::new();
        adapter_properties.insert(
            "Address".to_string(),
            Variant(Box::new("00:11:22:33:44:55".to_string())),
        );
        adapter_properties.insert(
            "AddressType".to_string(),
            Variant(Box::new("public".to_string())),
        );
        adapter_properties.insert("Name".to_string(), Variant(Box::new("name".to_string())));
        adapter_properties.insert("Alias".to_string(), Variant(Box::new("alias".to_string())));
        adapter_properties.insert("Powered".to_string(), Variant(Box::new(false)));
        adapter_properties.insert("Discovering".to_string(), Variant(Box::new(false)));

        let adapter = AdapterInfo::from_properties(
            id.clone(),
            OrgBluezAdapter1Properties(&adapter_properties),
        )
        .unwrap();
        assert_eq!(
            adapter,
            AdapterInfo {
                id,
                mac_address: MacAddress("00:11:22:33:44:55".to_string()),
                address_type: AddressType::Public,
                name: "name".to_string(),
                alias: "alias".to_string(),
                powered: false,
                discovering: false
            }
        )
    }
}
