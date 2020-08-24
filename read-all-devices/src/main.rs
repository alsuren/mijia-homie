use blurz::{BluetoothDevice, BluetoothEvent, BluetoothSession};
use mijia::{
    connect_sensors, decode_value, find_sensors, hashmap_from_file, print_sensors, scan,
    start_notify_sensors, SERVICE_CHARACTERISTIC_PATH,
};
use std::thread;
use std::time::Duration;

mod explore_device;

const SENSOR_NAMES_FILENAME: &str = "sensor_names.conf";

fn main() {
    let sensor_names = hashmap_from_file(SENSOR_NAMES_FILENAME).unwrap();
    let bt_session = &BluetoothSession::create_session(None).unwrap();
    let device_list = scan(&bt_session).unwrap();
    let sensors = find_sensors(&bt_session, &device_list);
    println!();
    print_sensors(&sensors, &sensor_names);
    let connected_sensors = connect_sensors(&sensors);
    print_sensors(&connected_sensors, &sensor_names);

    // We need to wait a bit after calling connect to safely
    // get the gatt services
    thread::sleep(Duration::from_millis(5000));
    for device in &connected_sensors {
        explore_device::explore_gatt_profile(bt_session, &device);
    }
    start_notify_sensors(bt_session, &connected_sensors);

    println!("READINGS");
    loop {
        for event in bt_session.incoming(1000).map(BluetoothEvent::from) {
            let (object_path, value) = match event {
                Some(BluetoothEvent::Value { object_path, value }) => (object_path, value),
                _ => continue,
            };

            // TODO: Make this less hacky.
            if !object_path.ends_with(SERVICE_CHARACTERISTIC_PATH) {
                continue;
            }
            let device_path = &object_path[..object_path.len() - SERVICE_CHARACTERISTIC_PATH.len()];
            let device = BluetoothDevice::new(bt_session, device_path.to_string());

            if let Some(readings) = decode_value(&value) {
                let mac_address = device.get_address().unwrap();
                let name = sensor_names.get(&mac_address).unwrap_or(&mac_address);
                println!(
                    "{} ({}) Temperature: {:.2}ºC Humidity: {:?}% Battery: {:?} mV ({:?}%)",
                    object_path,
                    name,
                    readings.temperature,
                    readings.humidity,
                    readings.battery_voltage,
                    readings.battery_percent
                );
            }
        }
    }
}
