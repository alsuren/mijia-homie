use blurz::{
    BluetoothAdapter, BluetoothDevice, BluetoothDiscoverySession, BluetoothGATTCharacteristic,
    BluetoothSession,
};
use std::cmp::max;
use std::collections::HashMap;
use std::convert::TryInto;
use std::error::Error;
use std::fs::File;
use std::io::{self, BufRead, BufReader, ErrorKind};
use std::thread;
use std::time::Duration;

const SCAN_DURATION: Duration = Duration::from_millis(5000);

const MIJIA_SERVICE_DATA_UUID: &str = "0000fe95-0000-1000-8000-00805f9b34fb";
pub const SERVICE_CHARACTERISTIC_PATH: &str = "/service0021/char0035";

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
    println!("{} sensors:", sensors.len());
    for device in sensors {
        println!(
            "{}: {:?}, {} services, {} service data",
            device.get_address().unwrap(),
            device.get_name(),
            device.get_gatt_services().map_or(0, |s| s.len()),
            device.get_service_data().map_or(0, |s| s.len())
        );
    }
}

pub fn connect_sensors<'a>(sensors: &'a [BluetoothDevice<'a>]) -> Vec<BluetoothDevice<'a>> {
    let mut connected_sensors = vec![];
    for device in sensors {
        if let Err(e) = device.connect(10000) {
            println!("Failed to connect {:?}: {:?}", device.get_id(), e);
        } else {
            connected_sensors.push(device.clone());
        }
    }

    println!("Connected to {} sensors.", connected_sensors.len());

    connected_sensors
}

pub fn start_notify_sensors<'a>(
    bt_session: &'a BluetoothSession,
    connected_sensors: &'a [BluetoothDevice<'a>],
) {
    for device in connected_sensors {
        let temp_humidity = BluetoothGATTCharacteristic::new(
            bt_session,
            device.get_id() + SERVICE_CHARACTERISTIC_PATH,
        );
        if let Err(e) = temp_humidity.start_notify() {
            println!("Failed to start notify on {}: {:?}", device.get_id(), e);
        }
    }
}

pub fn decode_value(value: &[u8]) -> Option<(f32, u8, u16, u16)> {
    if value.len() != 5 {
        return None;
    }

    let mut temperature_array = [0; 2];
    temperature_array.clone_from_slice(&value[..2]);
    let temperature = i16::from_le_bytes(temperature_array) as f32 * 0.01;
    let humidity = value[2];
    let battery_voltage = u16::from_le_bytes(value[3..5].try_into().unwrap());
    let battery_percent = (max(battery_voltage, 2100) - 2100) / 10;
    Some((temperature, humidity, battery_voltage, battery_percent))
}

/// Read the given file of key-value pairs into a hashmap.
/// Returns an empty hashmap if the file doesn't exist, or an error if it is malformed.
pub fn hashmap_from_file(filename: &str) -> Result<HashMap<String, String>, io::Error> {
    let mut map: HashMap<String, String> = HashMap::new();
    if let Ok(file) = File::open(filename) {
        for line in BufReader::new(file).lines() {
            let line = line?;
            let parts: Vec<&str> = line.splitn(2, '=').collect();
            if parts.len() != 2 {
                return Err(io::Error::new(
                    ErrorKind::Other,
                    format!("Invalid line '{}'", line),
                ));
            }
            map.insert(parts[0].to_string(), parts[1].to_string());
        }
    }
    Ok(map)
}
