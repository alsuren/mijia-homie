use dbus::arg::prop_cast;
use dbus::message::{MatchRule, SignalArgs};
use dbus::nonblock::stdintf::org_freedesktop_dbus::PropertiesPropertiesChanged;
use dbus::Message;

use super::{AdapterId, CharacteristicId, DeviceId};

/// An event relating to a Bluetooth device or adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BluetoothEvent {
    /// An event related to a Bluetooth adapter.
    Adapter {
        /// The ID of the Bluetooth adapter in question.
        id: AdapterId,
        /// Details of the specific event.
        event: AdapterEvent,
    },
    /// An event related to a Bluetooth device.
    Device {
        /// The ID of the Bluetooth device in question.
        id: DeviceId,
        /// Details of the specific event.
        event: DeviceEvent,
    },
    /// An event related to a GATT characteristic of a Bluetooth device.
    Characteristic {
        /// The ID of the GATT characteristic in question.
        id: CharacteristicId,
        /// Details of the specific event.
        event: CharacteristicEvent,
    },
}

/// Details of an event related to a Bluetooth adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdapterEvent {
    /// The adapter has been powered on or off.
    Powered { powered: bool },
    /// The adapter has started or stopped scanning for devices.
    Discovering { discovering: bool },
}

/// Details of an event related to a Bluetooth device.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeviceEvent {
    /// The device has connected or disconnected.
    Connected { connected: bool },
    /// A new value is available for the RSSI of the device.
    RSSI { rssi: i16 },
}

/// Details of an event related to a GATT characteristic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CharacteristicEvent {
    /// A new value of the characteristic has been received. This may be from a notification.
    Value { value: Vec<u8> },
}

impl BluetoothEvent {
    /// Return a `MatchRule` which will match all D-Bus messages which represent Bluetooth events.
    pub(crate) fn match_rule() -> MatchRule<'static> {
        // BusName validation just checks that the length and format is valid, so it should never
        // fail for a constant that we know is valid.
        let bus_name = "org.bluez".into();
        PropertiesPropertiesChanged::match_rule(Some(&bus_name), None).static_clone()
    }

    /// Return a list of Bluetooth events parsed from the given D-Bus message.
    pub(crate) fn message_to_events(message: Message) -> Vec<BluetoothEvent> {
        let mut events = vec![];
        // Return events for PropertiesChanged signals.
        if let Some(properties_changed) = PropertiesPropertiesChanged::from_message(&message) {
            let object_path = message.path().unwrap().to_string();
            log::trace!(
                "PropertiesChanged for {}: {:?}",
                object_path,
                properties_changed
            );
            let changed_properties = &properties_changed.changed_properties;
            match properties_changed.interface_name.as_ref() {
                "org.bluez.Adapter1" => {
                    let id = AdapterId { object_path };
                    if let Some(&powered) = prop_cast(changed_properties, "Powered") {
                        events.push(BluetoothEvent::Adapter {
                            id: id.clone(),
                            event: AdapterEvent::Powered { powered },
                        })
                    }
                    if let Some(&discovering) = prop_cast(changed_properties, "Discovering") {
                        events.push(BluetoothEvent::Adapter {
                            id,
                            event: AdapterEvent::Discovering { discovering },
                        });
                    }
                }
                "org.bluez.Device1" => {
                    let id = DeviceId { object_path };
                    if let Some(&connected) = prop_cast(changed_properties, "Connected") {
                        events.push(BluetoothEvent::Device {
                            id: id.clone(),
                            event: DeviceEvent::Connected { connected },
                        });
                    }
                    if let Some(&rssi) = prop_cast(changed_properties, "RSSI") {
                        events.push(BluetoothEvent::Device {
                            id,
                            event: DeviceEvent::RSSI { rssi },
                        });
                    }
                }
                "org.bluez.GattCharacteristic1" => {
                    let id = CharacteristicId { object_path };
                    if let Some(value) = prop_cast::<Vec<u8>>(changed_properties, "Value") {
                        events.push(BluetoothEvent::Characteristic {
                            id,
                            event: CharacteristicEvent::Value {
                                value: value.to_owned(),
                            },
                        })
                    }
                }
                _ => {}
            }
        } else {
            log::info!("Unexpected message: {:?}", message);
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use dbus::arg::{RefArg, Variant};
    use dbus::Path;

    use super::*;

    #[test]
    fn adapter_powered() {
        let mut changed_properties: HashMap<String, Variant<Box<dyn RefArg>>> = HashMap::new();
        changed_properties.insert("Powered".to_string(), Variant(Box::new(true)));
        let properties_changed = PropertiesPropertiesChanged {
            interface_name: "org.bluez.Adapter1".to_string(),
            changed_properties,
            invalidated_properties: vec![],
        };
        let message = properties_changed.to_emit_message(&Path::new("/org/bluez/hci0").unwrap());
        let id = AdapterId::new("/org/bluez/hci0");
        assert_eq!(
            BluetoothEvent::message_to_events(message),
            vec![BluetoothEvent::Adapter {
                id,
                event: AdapterEvent::Powered { powered: true }
            }]
        )
    }

    #[test]
    fn device_rssi() {
        let rssi = 42;
        let mut changed_properties: HashMap<String, Variant<Box<dyn RefArg>>> = HashMap::new();
        changed_properties.insert("RSSI".to_string(), Variant(Box::new(rssi)));
        let properties_changed = PropertiesPropertiesChanged {
            interface_name: "org.bluez.Device1".to_string(),
            changed_properties,
            invalidated_properties: vec![],
        };
        let message = properties_changed
            .to_emit_message(&Path::new("/org/bluez/hci0/dev_11_22_33_44_55_66").unwrap());
        let id = DeviceId::new("/org/bluez/hci0/dev_11_22_33_44_55_66");
        assert_eq!(
            BluetoothEvent::message_to_events(message),
            vec![BluetoothEvent::Device {
                id,
                event: DeviceEvent::RSSI { rssi }
            }]
        )
    }

    #[test]
    fn characteristic_value() {
        let mut changed_properties: HashMap<String, Variant<Box<dyn RefArg>>> = HashMap::new();
        let value: Vec<u8> = vec![1, 2, 3];
        changed_properties.insert("Value".to_string(), Variant(Box::new(value.clone())));
        let properties_changed = PropertiesPropertiesChanged {
            interface_name: "org.bluez.GattCharacteristic1".to_string(),
            changed_properties,
            invalidated_properties: vec![],
        };
        let message = properties_changed.to_emit_message(
            &Path::new("/org/bluez/hci0/dev_11_22_33_44_55_66/service0012/char0034").unwrap(),
        );
        let id =
            CharacteristicId::new("/org/bluez/hci0/dev_11_22_33_44_55_66/service0012/char0034");
        assert_eq!(
            BluetoothEvent::message_to_events(message),
            vec![BluetoothEvent::Characteristic {
                id,
                event: CharacteristicEvent::Value { value }
            }]
        )
    }
}
