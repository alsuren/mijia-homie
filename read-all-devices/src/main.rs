use blurz::{BluetoothEvent, BluetoothGATTCharacteristic, BluetoothSession};
use mijia::{connect_sensors, decode_value, find_sensors, print_sensors, scan};
use std::thread;
use std::time::Duration;

mod explore_device;

fn main() {
    let bt_session = &BluetoothSession::create_session(None).unwrap();
    let device_list = scan(&bt_session);
    let sensors = find_sensors(&bt_session, &device_list);
    print_sensors(&sensors);
    let connected_sensors = connect_sensors(&sensors);

    // We need to wait a bit after calling connect to safely
    // get the gatt services
    thread::sleep(Duration::from_millis(5000));
    for device in connected_sensors {
        explore_device::explore_gatt_profile(bt_session, &device);
        let temp_humidity =
            BluetoothGATTCharacteristic::new(bt_session, device.get_id() + "/service0021/char0035");
        if let Err(e) = temp_humidity.start_notify() {
            println!("Failed to start notify on {}: {:?}", device.get_id(), e);
        }
    }

    println!("READINGS");
    loop {
        for event in BluetoothSession::create_session(None)
            .unwrap()
            .incoming(1000)
            .map(BluetoothEvent::from)
        {
            if event.is_none() {
                continue;
            }

            let (object_path, value) = match event.clone().unwrap() {
                BluetoothEvent::Value { object_path, value } => (object_path, value),
                _ => continue,
            };

            if let Some((temperature, humidity)) = decode_value(&value) {
                println!(
                    "{} Temperature: {:.2}ºC Humidity: {:?}%",
                    object_path, temperature, humidity
                );
            }
        }
    }
}
