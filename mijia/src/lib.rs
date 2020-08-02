use blurz::{BluetoothAdapter, BluetoothDevice, BluetoothDiscoverySession, BluetoothSession};
use std::error::Error;
use std::thread;
use std::time::Duration;

const SCAN_DURATION: Duration = Duration::from_millis(5000);

const MIJIA_SERVICE_DATA_UUID: &str = "0000fe95-0000-1000-8000-00805f9b34fb";

pub fn scan<'a>(bt_session: &'a BluetoothSession) -> Result<Vec<String>, Box<dyn Error>> {
    let adapter: BluetoothAdapter = BluetoothAdapter::init(bt_session)?;
    adapter.set_powered(true)?;

    let discover_session =
        BluetoothDiscoverySession::create_session(&bt_session, adapter.get_id())?;
    discover_session.start_discovery()?;
    println!("Scanning");
    // Wait for the adapter to scan for a while.
    thread::sleep(SCAN_DURATION);
    let device_list = adapter.get_device_list()?;

    discover_session.stop_discovery()?;

    println!("{:?} devices found", device_list.len());

    Ok(device_list)
}

pub fn find_sensors<'a>(
    bt_session: &'a BluetoothSession,
    device_list: &[String],
) -> Vec<BluetoothDevice<'a>> {
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

    sensors
}

pub fn print_sensors(sensors: &[BluetoothDevice]) {
    println!();
    println!("{} sensors:", sensors.len());
    for device in sensors {
        println!(
            "{:?}, {} services, {} service data",
            device.get_name(),
            device.get_gatt_services().map_or(0, |s| s.len()),
            device.get_service_data().map_or(0, |s| s.len())
        );
    }
}

pub fn connect_sensors<'a>(sensors: &'a [BluetoothDevice<'a>]) -> Vec<&'a BluetoothDevice<'a>> {
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

pub fn decode_value(value: &[u8]) -> Option<(f32, u8)> {
    if value.len() != 5 {
        return None;
    }

    let mut temperature_array = [0; 2];
    temperature_array.clone_from_slice(&value[..2]);
    let temperature = u16::from_le_bytes(temperature_array) as f32 * 0.01;
    let humidity = value[2];
    Some((temperature, humidity))
}
