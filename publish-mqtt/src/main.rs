use bluez_generated::bluetooth_event::BluetoothEvent;
use blurz::{BluetoothAdapter, BluetoothDevice, BluetoothDiscoverySession, BluetoothSession};
use dbus::nonblock::SyncConnection;
use futures::stream::StreamExt;
use futures::FutureExt;
use homie::{Datatype, HomieDevice, Node, Property};
use mijia::{
    decode_value, find_sensors, hashmap_from_file, print_sensors, start_notify_sensor, Readings,
    SERVICE_CHARACTERISTIC_PATH,
};
use rumqttc::MqttOptions;
use rustls::ClientConfig;
use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::{task, time, try_join};

const DEFAULT_MQTT_PREFIX: &str = "homie";
const DEFAULT_DEVICE_ID: &str = "mijia-bridge";
const DEFAULT_DEVICE_NAME: &str = "Mijia bridge";
const DEFAULT_HOST: &str = "test.mosquitto.org";
const DEFAULT_PORT: u16 = 1883;
const SCAN_DURATION: Duration = Duration::from_secs(15);
const CONNECT_TIMEOUT_MS: i32 = 4_000;
const UPDATE_TIMEOUT: Duration = Duration::from_secs(60);
const SENSOR_NAMES_FILENAME: &str = "sensor_names.conf";

async fn scan(bt_session: &BluetoothSession) -> Result<Vec<String>, Box<dyn Error>> {
    let adapter: BluetoothAdapter = BluetoothAdapter::init(bt_session)?;
    adapter.set_powered(true)?;

    let discover_session =
        BluetoothDiscoverySession::create_session(&bt_session, adapter.get_id())?;
    discover_session.start_discovery()?;
    println!("Scanning");
    // Wait for the adapter to scan for a while.
    time::delay_for(SCAN_DURATION).await;
    let device_list = adapter.get_device_list()?;

    discover_session.stop_discovery()?;

    println!("{:?} devices found", device_list.len());

    Ok(device_list)
}

#[tokio::main(core_threads = 3)]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    dotenv::dotenv()?;
    pretty_env_logger::init();
    color_backtrace::install();

    let device_id = std::env::var("DEVICE_ID").unwrap_or_else(|_| DEFAULT_DEVICE_ID.to_string());
    let device_name =
        std::env::var("DEVICE_NAME").unwrap_or_else(|_| DEFAULT_DEVICE_NAME.to_string());
    let client_name = std::env::var("CLIENT_NAME").unwrap_or_else(|_| device_id.clone());

    let host = std::env::var("HOST").unwrap_or_else(|_| DEFAULT_HOST.to_string());

    let port = std::env::var("PORT")
        .ok()
        .and_then(|val| val.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);

    let mut mqttoptions = MqttOptions::new(client_name, host, port);

    let username = std::env::var("USERNAME").ok();
    let password = std::env::var("PASSWORD").ok();

    mqttoptions.set_keep_alive(5);
    if let (Some(u), Some(p)) = (username, password) {
        mqttoptions.set_credentials(u, p);
    }

    // Use `env -u USE_TLS` to unset this variable if you need to clear it.
    if std::env::var("USE_TLS").is_ok() {
        let mut client_config = ClientConfig::new();
        client_config.root_store =
            rustls_native_certs::load_native_certs().expect("could not load platform certs");
        mqttoptions.set_tls_client_config(Arc::new(client_config));
    }

    let mqtt_prefix =
        std::env::var("MQTT_PREFIX").unwrap_or_else(|_| DEFAULT_MQTT_PREFIX.to_string());
    let device_base = format!("{}/{}", mqtt_prefix, device_id);
    let (homie, mqtt_handle) = HomieDevice::builder(&device_base, &device_name, mqttoptions)
        .spawn()
        .await?;

    let local = task::LocalSet::new();

    // Connect to the D-Bus session bus (this is blocking, unfortunately).
    let (dbus_resource, conn) = dbus_tokio::connection::new_session_sync()?;
    let dbus_handle = tokio::spawn(async {
        let err = dbus_resource.await;
        Err::<(), Box<dyn Error + Send + Sync>>(err)
    });

    let bluetooth_handle = local.spawn_local(async move {
        bluetooth_mainloop(homie, conn).await.unwrap();
    });

    // Poll everything to completion, until the first one bombs out.
    let res: Result<_, Box<dyn Error + Send + Sync>> = try_join! {
        // LocalSet finished first. Colour me confused.
        local.map(|()| Ok(println!("WTF?"))),
        // The resource is a task that should be spawned onto a tokio compatible
        // reactor ASAP. If the resource ever finishes, you lost connection to D-Bus.
        dbus_handle.map(|res| Ok(res??)),
        // Bluetooth finished first. Convert error and get on with your life.
        bluetooth_handle.map(|res| Ok(res?)),
        // MQTT event loop finished first.
        mqtt_handle,
    };
    res?;
    Ok(())
}

#[derive(Debug)]
struct Sensor {
    device_path: String,
    mac_address: String,
    name: String,
    last_update_timestamp: Instant,
}

impl Sensor {
    const PROPERTY_ID_TEMPERATURE: &'static str = "temperature";
    const PROPERTY_ID_HUMIDITY: &'static str = "humidity";
    const PROPERTY_ID_BATTERY: &'static str = "battery";

    pub fn new(
        device: &BluetoothDevice,
        sensor_names: &HashMap<String, String>,
    ) -> Result<Self, Box<dyn Error>> {
        let mac_address = device.get_address()?;
        let name = sensor_names
            .get(&mac_address)
            .cloned()
            .unwrap_or_else(|| mac_address.clone());
        Ok(Self {
            device_path: device.get_id(),
            mac_address,
            name,
            last_update_timestamp: Instant::now(),
        })
    }

    pub fn node_id(&self) -> String {
        self.mac_address.replace(":", "")
    }

    pub fn device<'a>(&self, session: &'a BluetoothSession) -> BluetoothDevice<'a> {
        BluetoothDevice::new(session, self.device_path.to_string())
    }

    fn as_node(&self) -> Node {
        Node::new(
            self.node_id(),
            self.name.to_string(),
            "Mijia sensor".to_string(),
            vec![
                Property::new(
                    Self::PROPERTY_ID_TEMPERATURE,
                    "Temperature",
                    Datatype::Float,
                    Some("ºC"),
                ),
                Property::new(
                    Self::PROPERTY_ID_HUMIDITY,
                    "Humidity",
                    Datatype::Integer,
                    Some("%"),
                ),
                Property::new(
                    Self::PROPERTY_ID_BATTERY,
                    "Battery level",
                    Datatype::Integer,
                    Some("%"),
                ),
            ],
        )
    }

    async fn publish_readings(
        &self,
        homie: &HomieDevice,
        readings: &Readings,
    ) -> Result<(), Box<dyn Error>> {
        println!("{} {} ({})", self.mac_address, readings, self.name);

        let node_id = self.node_id();
        homie
            .publish_value(
                &node_id,
                Self::PROPERTY_ID_TEMPERATURE,
                format!("{:.2}", readings.temperature),
            )
            .await?;
        homie
            .publish_value(&node_id, Self::PROPERTY_ID_HUMIDITY, readings.humidity)
            .await?;
        homie
            .publish_value(
                &node_id,
                Self::PROPERTY_ID_BATTERY,
                readings.battery_percent,
            )
            .await?;
        Ok(())
    }
}

#[derive(Debug)]
struct SensorState {
    sensors_to_connect: VecDeque<Sensor>,
    sensors_connected: Vec<Sensor>,
    homie: HomieDevice,
}

async fn bluetooth_mainloop(
    mut homie: HomieDevice,
    conn: Arc<SyncConnection>,
) -> Result<(), Box<dyn Error>> {
    let sensor_names = hashmap_from_file(SENSOR_NAMES_FILENAME)?;

    let bt_session = BluetoothSession::create_session(None)?;
    let device_list = scan(&bt_session).await?;
    let sensors = find_sensors(&bt_session, &device_list);
    print_sensors(&sensors, &sensor_names);
    let (named_sensors, unnamed_sensors): (Vec<_>, Vec<_>) = sensors
        .into_iter()
        .map(|d| Sensor::new(&d, &sensor_names).unwrap())
        .partition(|sensor| sensor_names.contains_key(&sensor.mac_address));
    println!(
        "{} named sensors, {} unnamed sensors",
        named_sensors.len(),
        unnamed_sensors.len()
    );

    homie.ready().await?;

    let state = Arc::new(Mutex::new(SensorState {
        sensors_to_connect: named_sensors.into(),
        sensors_connected: vec![],
        homie,
    }));

    let t1 = async {
        loop {
            {
                let state = &mut *state.lock().await;
                connect_first_sensor_in_queue(
                    &bt_session,
                    &mut state.homie,
                    &mut state.sensors_connected,
                    &mut state.sensors_to_connect,
                )
                .await?;
            }

            {
                let state = &mut *state.lock().await;
                disconnect_first_stale_sensor(
                    &mut state.homie,
                    &mut state.sensors_connected,
                    &mut state.sensors_to_connect,
                )
                .await?;
            }
        }
        #[allow(unreachable_code)]
        Ok(())
    };
    let t2 = async {
        service_bluetooth_event_queue(state.clone(), conn.clone()).await?;
        Ok(())
    };
    try_join!(t1, t2).map(|((), ())| ())
}

async fn connect_first_sensor_in_queue(
    bt_session: &BluetoothSession,
    homie: &mut HomieDevice,
    sensors_connected: &mut Vec<Sensor>,
    sensors_to_connect: &mut VecDeque<Sensor>,
) -> Result<(), Box<dyn Error>> {
    println!("{} sensors in queue to connect.", sensors_to_connect.len());
    // Try to connect to a sensor.
    if let Some(mut sensor) = sensors_to_connect.pop_front() {
        println!("Trying to connect to {}", sensor.name);
        match connect_start_sensor(bt_session, homie, &mut sensor).await {
            Err(e) => {
                println!("Failed to connect to {}: {:?}", sensor.name, e);
                sensors_to_connect.push_back(sensor);
            }
            Ok(()) => {
                println!("Connected to {} and started notifications", sensor.name);
                sensors_connected.push(sensor);
            }
        }
    }
    Ok(())
}

async fn connect_start_sensor<'a>(
    bt_session: &'a BluetoothSession,
    homie: &mut HomieDevice,
    sensor: &mut Sensor,
) -> Result<(), Box<dyn Error>> {
    let device = sensor.device(bt_session);
    println!("Connecting");
    device.connect(CONNECT_TIMEOUT_MS)?;
    println!("Starting notifications");
    start_notify_sensor(bt_session, &device)?;

    println!("Adding Homie node");
    homie.add_node(sensor.as_node()).await?;
    println!("Resetting update timestamp");
    sensor.last_update_timestamp = Instant::now();
    Ok(())
}

/// If a sensor hasn't sent any updates in a while, disconnect it and add it back to the
/// connect queue.
async fn disconnect_first_stale_sensor(
    homie: &mut HomieDevice,
    sensors_connected: &mut Vec<Sensor>,
    sensors_to_connect: &mut VecDeque<Sensor>,
) -> Result<(), Box<dyn Error>> {
    let now = Instant::now();
    if let Some(sensor_index) = sensors_connected
        .iter()
        .position(|s| now - s.last_update_timestamp > UPDATE_TIMEOUT)
    {
        let sensor = sensors_connected.remove(sensor_index);
        println!(
            "No update from {} for {:?}, reconnecting",
            sensor.name,
            now - sensor.last_update_timestamp
        );
        homie.remove_node(&sensor.node_id()).await?;
        sensors_to_connect.push_back(sensor);
    }
    Ok(())
}

async fn service_bluetooth_event_queue(
    state: Arc<Mutex<SensorState>>,
    conn: Arc<SyncConnection>,
) -> Result<(), Box<dyn Error>> {
    let mut rule = dbus::message::MatchRule::new();
    rule.msg_type = Some(dbus::message::MessageType::Signal);
    rule.sender = Some(dbus::strings::BusName::new("org.bluez")?);

    let (msg_match, mut events) = conn.add_match(rule).await?.msg_stream();
    // Process events until there are none available for the timeout.
    while let Some(raw_event) = events.next().await {
        if let Some(event) = BluetoothEvent::from(raw_event) {
            handle_bluetooth_event(state.clone(), event).await?
        }
    }
    // TODO: move this into a defer or scope guard or something.
    conn.remove_match(msg_match.token()).await?;
    Ok(())
}

async fn handle_bluetooth_event(
    state: Arc<Mutex<SensorState>>,
    event: BluetoothEvent,
) -> Result<(), Box<dyn Error>> {
    let state = &mut *state.lock().await;
    let homie = &mut state.homie;
    let sensors_connected = &mut state.sensors_connected;
    let sensors_to_connect = &mut state.sensors_to_connect;
    match event {
        BluetoothEvent::Value { object_path, value } => {
            // TODO: Make this less hacky.
            let device_path = match object_path.strip_suffix(SERVICE_CHARACTERISTIC_PATH) {
                Some(path) => path,
                None => return Ok(()),
            };
            if let Some(sensor) = sensors_connected
                .iter_mut()
                .find(|s| s.device_path == device_path)
            {
                sensor.last_update_timestamp = Instant::now();
                if let Some(readings) = decode_value(&value) {
                    sensor.publish_readings(homie, &readings).await?;
                } else {
                    println!("Invalid value from {}", sensor.name);
                }
            } else {
                // TODO: Still send it, in case it is useful?
                println!("Got update from unexpected device {}", device_path);
            }
        }
        BluetoothEvent::Connected {
            object_path,
            connected: false,
        } => {
            if let Some(sensor_index) = sensors_connected
                .iter()
                .position(|s| s.device_path == object_path)
            {
                let sensor = sensors_connected.remove(sensor_index);
                println!("{} disconnected", sensor.name);
                homie.remove_node(&sensor.node_id()).await?;
                sensors_to_connect.push_back(sensor);
            } else {
                println!(
                    "{} disconnected but wasn't known to be connected.",
                    object_path
                );
            }
        }
        _ => {
            log::trace!("{:?}", event);
        }
    };
    Ok(())
}
