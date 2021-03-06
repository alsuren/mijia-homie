use bluez_generated::OrgBluezDevice1Properties;
use dbus::arg::{cast, PropMap, RefArg, Variant};
use dbus::Path;
use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;
use uuid::Uuid;

use crate::{AdapterId, BluetoothError, MacAddress};

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

/// Information about a Bluetooth device which was discovered.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceInfo {
    /// An opaque identifier for the device, including a reference to which adapter it was
    /// discovered on. This can be used to connect to it.
    pub id: DeviceId,
    /// The MAC address of the device.
    pub mac_address: MacAddress,
    /// The type of MAC address the device uses.
    pub address_type: AddressType,
    /// The human-readable name of the device, if available.
    pub name: Option<String>,
    /// The appearance of the device, as defined by GAP.
    pub appearance: Option<u16>,
    /// The GATT service UUIDs (if any) from the device's advertisement or service discovery.
    ///
    /// Note that service discovery only happens after a connection has been made to the device, but
    /// BlueZ may cache the list of services after it is disconnected.
    pub services: Vec<Uuid>,
    /// Whether the device is currently paired with the adapter.
    pub paired: bool,
    /// Whether the device is currently connected to the adapter.
    pub connected: bool,
    /// The Received Signal Strength Indicator of the device advertisement or inquiry.
    pub rssi: Option<i16>,
    /// The transmission power level advertised by the device.
    pub tx_power: Option<i16>,
    /// Manufacturer-specific advertisement data, if any. The keys are 'manufacturer IDs'.
    pub manufacturer_data: HashMap<u16, Vec<u8>>,
    /// The GATT service data from the device's advertisement, if any. This is a map from the
    /// service UUID to its data.
    pub service_data: HashMap<Uuid, Vec<u8>>,
    /// Whether service discovery has finished for the device.
    pub services_resolved: bool,
}

impl DeviceInfo {
    pub(crate) fn from_properties(
        id: DeviceId,
        device_properties: OrgBluezDevice1Properties,
    ) -> Result<DeviceInfo, BluetoothError> {
        let mac_address = device_properties
            .address()
            .ok_or(BluetoothError::RequiredPropertyMissing("Address"))?;
        let address_type = device_properties
            .address_type()
            .ok_or(BluetoothError::RequiredPropertyMissing("AddressType"))?
            .parse()?;
        let services = get_services(device_properties);
        let manufacturer_data = get_manufacturer_data(device_properties).unwrap_or_default();
        let service_data = get_service_data(device_properties).unwrap_or_default();

        Ok(DeviceInfo {
            id,
            mac_address: MacAddress(mac_address.to_owned()),
            address_type,
            name: device_properties.name().cloned(),
            appearance: device_properties.appearance(),
            services,
            paired: device_properties
                .paired()
                .ok_or(BluetoothError::RequiredPropertyMissing("Paired"))?,
            connected: device_properties
                .connected()
                .ok_or(BluetoothError::RequiredPropertyMissing("Connected"))?,
            rssi: device_properties.rssi(),
            tx_power: device_properties.tx_power(),
            manufacturer_data,
            service_data,
            services_resolved: device_properties
                .services_resolved()
                .ok_or(BluetoothError::RequiredPropertyMissing("ServicesResolved"))?,
        })
    }
}

/// MAC address type of a Bluetooth device.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AddressType {
    /// Public address.
    Public,
    /// Random address.
    Random,
}

impl AddressType {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Random => "random",
        }
    }
}

impl Display for AddressType {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AddressType {
    type Err = BluetoothError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "public" => Ok(Self::Public),
            "random" => Ok(Self::Random),
            _ => Err(BluetoothError::AddressTypeParseError(s.to_owned())),
        }
    }
}

fn get_manufacturer_data(
    device_properties: OrgBluezDevice1Properties,
) -> Option<HashMap<u16, Vec<u8>>> {
    Some(convert_manufacturer_data(
        device_properties.manufacturer_data()?,
    ))
}

pub(crate) fn convert_manufacturer_data(
    data: &HashMap<u16, Variant<Box<dyn RefArg>>>,
) -> HashMap<u16, Vec<u8>> {
    data.iter()
        .filter_map(|(&k, v)| {
            if let Some(v) = cast::<Vec<u8>>(&v.0) {
                Some((k, v.to_owned()))
            } else {
                log::warn!("Manufacturer data had wrong type: {:?}", &v.0);
                None
            }
        })
        .collect()
}

fn get_service_data(
    device_properties: OrgBluezDevice1Properties,
) -> Option<HashMap<Uuid, Vec<u8>>> {
    Some(convert_service_data(device_properties.service_data()?))
}

pub(crate) fn convert_service_data(data: &PropMap) -> HashMap<Uuid, Vec<u8>> {
    data.iter()
        .filter_map(|(k, v)| match Uuid::parse_str(k) {
            Ok(uuid) => {
                if let Some(v) = cast::<Vec<u8>>(&v.0) {
                    Some((uuid, v.to_owned()))
                } else {
                    log::warn!("Service data had wrong type: {:?}", &v.0);
                    None
                }
            }
            Err(err) => {
                log::warn!("Error parsing service data UUID: {}", err);
                None
            }
        })
        .collect()
}

fn get_services(device_properties: OrgBluezDevice1Properties) -> Vec<Uuid> {
    if let Some(uuids) = device_properties.uuids() {
        convert_services(uuids)
    } else {
        vec![]
    }
}

pub(crate) fn convert_services(uuids: &Vec<String>) -> Vec<Uuid> {
    uuids
        .iter()
        .filter_map(|uuid| {
            Uuid::parse_str(uuid)
                .map_err(|err| {
                    log::warn!("Error parsing service data UUID: {}", err);
                    err
                })
                .ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::uuid_from_u32;

    use super::*;

    #[test]
    fn device_adapter() {
        let adapter_id = AdapterId::new("/org/bluez/hci0");
        let device_id = DeviceId::new("/org/bluez/hci0/dev_11_22_33_44_55_66");
        assert_eq!(device_id.adapter(), adapter_id);
    }

    #[test]
    fn service_data() {
        let uuid = uuid_from_u32(0x11223344);
        let mut service_data: HashMap<String, Variant<Box<dyn RefArg>>> = HashMap::new();
        service_data.insert(uuid.to_string(), Variant(Box::new(vec![1u8, 2, 3])));
        let mut device_properties: HashMap<String, Variant<Box<dyn RefArg>>> = HashMap::new();
        device_properties.insert("ServiceData".to_string(), Variant(Box::new(service_data)));

        let mut expected_service_data = HashMap::new();
        expected_service_data.insert(uuid, vec![1u8, 2, 3]);

        assert_eq!(
            get_service_data(OrgBluezDevice1Properties(&device_properties)),
            Some(expected_service_data)
        );
    }

    #[test]
    fn manufacturer_data() {
        let manufacturer_id = 0x1122;
        let mut manufacturer_data: HashMap<u16, Variant<Box<dyn RefArg>>> = HashMap::new();
        manufacturer_data.insert(manufacturer_id, Variant(Box::new(vec![1u8, 2, 3])));
        let mut device_properties: HashMap<String, Variant<Box<dyn RefArg>>> = HashMap::new();
        device_properties.insert(
            "ManufacturerData".to_string(),
            Variant(Box::new(manufacturer_data)),
        );

        let mut expected_manufacturer_data = HashMap::new();
        expected_manufacturer_data.insert(manufacturer_id, vec![1u8, 2, 3]);

        assert_eq!(
            get_manufacturer_data(OrgBluezDevice1Properties(&device_properties)),
            Some(expected_manufacturer_data)
        );
    }

    #[test]
    fn device_info_minimal() {
        let id = DeviceId::new("/org/bluez/hci0/dev_11_22_33_44_55_66");
        let mut device_properties: HashMap<String, Variant<Box<dyn RefArg>>> = HashMap::new();
        device_properties.insert(
            "Address".to_string(),
            Variant(Box::new("00:11:22:33:44:55".to_string())),
        );
        device_properties.insert(
            "AddressType".to_string(),
            Variant(Box::new("public".to_string())),
        );
        device_properties.insert("Paired".to_string(), Variant(Box::new(false)));
        device_properties.insert("Connected".to_string(), Variant(Box::new(false)));
        device_properties.insert("ServicesResolved".to_string(), Variant(Box::new(false)));

        let device =
            DeviceInfo::from_properties(id.clone(), OrgBluezDevice1Properties(&device_properties))
                .unwrap();
        assert_eq!(
            device,
            DeviceInfo {
                id,
                mac_address: MacAddress("00:11:22:33:44:55".to_string()),
                address_type: AddressType::Public,
                name: None,
                appearance: None,
                services: vec![],
                paired: false,
                connected: false,
                rssi: None,
                tx_power: None,
                manufacturer_data: HashMap::new(),
                service_data: HashMap::new(),
                services_resolved: false,
            }
        )
    }

    #[test]
    fn get_services_none() {
        let device_properties: HashMap<String, Variant<Box<dyn RefArg>>> = HashMap::new();

        assert_eq!(
            get_services(OrgBluezDevice1Properties(&device_properties)),
            vec![]
        )
    }

    #[test]
    fn get_services_some() {
        let uuid = uuid_from_u32(0x11223344);
        let uuids = vec![uuid.to_string()];
        let mut device_properties: HashMap<String, Variant<Box<dyn RefArg>>> = HashMap::new();
        device_properties.insert("UUIDs".to_string(), Variant(Box::new(uuids)));

        assert_eq!(
            get_services(OrgBluezDevice1Properties(&device_properties)),
            vec![uuid]
        )
    }

    #[test]
    fn address_type_parse() {
        for &address_type in &[AddressType::Public, AddressType::Random] {
            assert_eq!(
                address_type.to_string().parse::<AddressType>().unwrap(),
                address_type
            );
        }
    }
}
