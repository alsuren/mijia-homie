use blurz::{
    BluetoothAdapter, BluetoothDevice, BluetoothDiscoverySession, BluetoothEvent,
    BluetoothGATTCharacteristic, BluetoothSession,
};
use std::thread;
use std::time::Duration;

mod explore_device;

const SCAN_DURATION: Duration = Duration::from_millis(5000);

const MIJIA_SERVICE_DATA_UUID: &str = "0000fe95-0000-1000-8000-00805f9b34fb";

fn find_sensors<'a>(bt_session: &'a BluetoothSession) -> Vec<BluetoothDevice<'a>> {
    let adapter: BluetoothAdapter = BluetoothAdapter::init(bt_session).unwrap();
    if let Err(_error) = adapter.set_powered(true) {
        panic!("Failed to power adapter");
    }

    let discover_session =
        BluetoothDiscoverySession::create_session(&bt_session, adapter.get_id()).unwrap();
    if let Err(_error) = discover_session.start_discovery() {
        panic!("Failed to start discovery");
    }
    println!("Scanning");
    // Wait for the adapter to scan for a while.
    thread::sleep(SCAN_DURATION);
    let device_list = adapter.get_device_list().unwrap();

    discover_session.stop_discovery().unwrap();

    println!("{:?} devices found", device_list.len());

    let mut sensors = vec![];
    for device_path in device_list {
        let device = BluetoothDevice::new(bt_session, device_path.to_string());
        println!(
            "Device: {:?} Name: {:?}",
            device_path,
            device.get_name().ok()
        );
        if let Ok(service_data) = device.get_service_data() {
            println!("Service data: {:?}", service_data);
            // If there are no services advertised here then trying to connect below will fail, so
            // no point trying.
            if service_data.contains_key(MIJIA_SERVICE_DATA_UUID)
                && device.get_gatt_services().map_or(0, |s| s.len()) > 0
            {
                sensors.push(device);
            }
        }
    }

    println!();
    println!("{} sensors:", sensors.len());
    for device in &sensors {
        println!(
            "{:?}, {} services, {} service data",
            device.get_name(),
            device.get_gatt_services().map_or(0, |s| s.len()),
            device.get_service_data().map_or(0, |s| s.len())
        );
    }

    sensors
}

fn connect_sensors<'a>(sensors: &'a [BluetoothDevice<'a>]) -> Vec<&'a BluetoothDevice<'a>> {
    let mut connected_sensors = vec![];
    for device in sensors {
        if let Err(e) = device.connect(10000) {
            println!("Failed to connect {:?}: {:?}", device.get_id(), e);
        } else {
            connected_sensors.push(device);
        }
    }

    println!("Connected to {} sensors.", connected_sensors.len());

    connected_sensors
}

fn decode_value(value: &[u8]) -> (f32, u8) {
    let mut temperature_array = [0; 2];
    temperature_array.clone_from_slice(&value[..2]);
    let temperature = u16::from_le_bytes(temperature_array) as f32 * 0.01;
    let humidity = value[2];
    (temperature, humidity)
}

fn main() {
    let bt_session = &BluetoothSession::create_session(None).unwrap();
    let mut sensors = find_sensors(&bt_session);
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

            let (temperature, humidity) = decode_value(&value);
            println!(
                "{} Temperature: {:.2}ºC Humidity: {:?}%",
                object_path, temperature, humidity
            );
        }
    }
}
