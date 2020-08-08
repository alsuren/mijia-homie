use blurz::{BluetoothEvent, BluetoothSession};
use mijia::{
    connect_sensors, decode_value, find_sensors, print_sensors, scan, start_notify_sensors,
};
use std::thread;
use std::time::Duration;

mod explore_device;

fn main() {
    let bt_session = &BluetoothSession::create_session(None).unwrap();
    let device_list = scan(&bt_session).unwrap();
    let sensors = find_sensors(&bt_session, &device_list);
    println!();
    print_sensors(&sensors);
    let connected_sensors = connect_sensors(&sensors);
    print_sensors(&connected_sensors);

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

            if let Some((temperature, humidity, battery_voltage, battery_percent)) =
                decode_value(&value)
            {
                println!(
                    "{} Temperature: {:.2}ºC Humidity: {:?}% Battery: {:?} mV ({:?}%)",
                    object_path, temperature, humidity, battery_voltage, battery_percent
                );
            }
        }
    }
}
