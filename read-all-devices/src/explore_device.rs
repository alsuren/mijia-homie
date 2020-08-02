use blurz::{
    BluetoothDevice, BluetoothGATTCharacteristic, BluetoothGATTDescriptor, BluetoothGATTService,
    BluetoothSession,
};
use lazy_static::lazy_static;
use regex::Regex;
use std::str;

const UUID_REGEX: &str = r"([0-9a-f]{8})-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}";

pub fn explore_gatt_profile(session: &BluetoothSession, device: &BluetoothDevice) {
    println!("{:?}", device.get_name().unwrap());

    let services_list = match device.get_gatt_services() {
        Ok(services) => services,
        Err(e) => {
            println!("Failed to get services: {:?}", e);
            return;
        }
    };

    lazy_static! {
        static ref RE: Regex = Regex::new(UUID_REGEX).unwrap();
    }

    for service_path in services_list {
        let service = BluetoothGATTService::new(session, service_path.clone());
        let uuid = service.get_uuid().unwrap();
        let assigned_number = RE
            .captures(&uuid)
            .unwrap()
            .get(1)
            .map_or("", |m| m.as_str());

        println!(
            "Service UUID: {:?} Assigned Number: 0x{:?}",
            uuid, assigned_number
        );
        let characteristics = match service.get_gatt_characteristics() {
            Ok(characteristics) => characteristics,
            Err(e) => {
                println!("Failed to get characteristics: {:?}", e);
                return;
            }
        };
        for characteristic_path in characteristics {
            explore_gatt_characteristic(session, characteristic_path);
        }
        println!();
    }
}

fn explore_gatt_characteristic(session: &BluetoothSession, characteristic_path: String) {
    let characteristic = BluetoothGATTCharacteristic::new(session, characteristic_path.clone());
    lazy_static! {
        static ref RE: Regex = Regex::new(UUID_REGEX).unwrap();
    }
    let uuid = characteristic.get_uuid().unwrap();
    let assigned_number = RE
        .captures(&uuid)
        .unwrap()
        .get(1)
        .map_or("", |m| m.as_str());

    let flags = characteristic.get_flags().unwrap();

    println!(
        " Characteristic ID: {:?} Assigned Number: 0x{:?} Flags: {:?}",
        characteristic_path, assigned_number, flags
    );

    let descriptors = match characteristic.get_gatt_descriptors() {
        Ok(descriptors) => descriptors,
        Err(e) => {
            println!("Failed to get descriptors: {:?}", e);
            return;
        }
    };
    for descriptor_path in descriptors {
        explore_gatt_descriptor(&session, descriptor_path);
    }
}

fn explore_gatt_descriptor(session: &BluetoothSession, descriptor_path: String) {
    let descriptor = BluetoothGATTDescriptor::new(session, descriptor_path);
    lazy_static! {
        static ref RE: Regex = Regex::new(UUID_REGEX).unwrap();
    }
    let uuid = descriptor.get_uuid().unwrap();
    let assigned_number = RE
        .captures(&uuid)
        .unwrap()
        .get(1)
        .map_or("", |m| m.as_str());

    let value = descriptor.read_value(None).unwrap();
    let value = match &assigned_number[4..] {
        "2901" => str::from_utf8(&value).unwrap().to_string(),
        _ => format!("{:?}", value),
    };

    println!(
        "    Descriptor Assigned Number: 0x{:?} Read Value: {:?}",
        assigned_number, value
    );
}
