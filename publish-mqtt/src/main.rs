use anyhow::Context;
use futures::stream::StreamExt;
use futures::{FutureExt, TryFutureExt};
use homie::{Datatype, HomieDevice, Node, Property};
use mijia::{
    get_sensors, hashmap_from_file, start_notify_sensor, MijiaEvent, MijiaSession, Readings,
    SensorProps,
};
use rumqttc::MqttOptions;
use rustls::ClientConfig;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::{task, time, try_join};

const DEFAULT_MQTT_PREFIX: &str = "homie";
const DEFAULT_DEVICE_ID: &str = "mijia-bridge";
const DEFAULT_DEVICE_NAME: &str = "Mijia bridge";
const DEFAULT_HOST: &str = "test.mosquitto.org";
const DEFAULT_PORT: u16 = 1883;
const SCAN_INTERVAL: Duration = Duration::from_secs(15);
const CONNECT_INTERVAL: Duration = Duration::from_secs(1);
const UPDATE_TIMEOUT: Duration = Duration::from_secs(60);
const SENSOR_NAMES_FILENAME: &str = "sensor_names.conf";

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
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

    // Connect a bluetooth session.
    let (dbus_handle, bt_session) = MijiaSession::new().await?;

    let bluetooth_handle =
        local.run_until(async move { bluetooth_mainloop(homie, bt_session).await });

    // Poll everything to completion, until the first one bombs out.
    let res: Result<_, anyhow::Error> = try_join! {
        // The resource is a task that should be spawned onto a tokio compatible
        // reactor ASAP. If the resource ever finishes, you lost connection to D-Bus.
        dbus_handle,
        // Bluetooth finished first. Convert error and get on with your life.
        bluetooth_handle.map(|res| Ok(res?)),
        // MQTT event loop finished first.
        mqtt_handle.map_err(|err| anyhow::anyhow!(err)),
    };
    res?;
    Ok(())
}

#[derive(Debug)]
enum ConnectionStatus {
    /// Not yet attempted to connect. Might already be connected from a previous
    /// run of this program.
    Unknown,
    /// Connected, but could not subscribe to updates. GATT characteristics
    /// sometimes take a while to show up after connecting, so this retry is
    /// a bit of a work-around.
    SubscribingFailedOnce,
    /// Disconnected because we stopped getting updates.
    WatchdogTimeOut,
    /// We explicity disconnected because something else went wrong.
    Disconnected,
    /// We received a Disconnected event.
    /// This should only be treated as informational, because disconnection
    /// events might be received racily. The sensor might actually be Connected.
    MarkedDisconnected,
    /// Connected and subscribed to updates
    Connected,
}

#[derive(Debug)]
struct Sensor {
    object_path: String,
    mac_address: String,
    name: String,
    last_update_timestamp: Instant,
    connection_status: ConnectionStatus,
}

impl Sensor {
    const PROPERTY_ID_TEMPERATURE: &'static str = "temperature";
    const PROPERTY_ID_HUMIDITY: &'static str = "humidity";
    const PROPERTY_ID_BATTERY: &'static str = "battery";

    pub fn new(
        props: SensorProps,
        sensor_names: &HashMap<String, String>,
    ) -> Result<Self, anyhow::Error> {
        let name = sensor_names
            .get(&props.mac_address)
            .cloned()
            .unwrap_or_else(|| props.mac_address.clone());
        Ok(Self {
            object_path: props.object_path,
            mac_address: props.mac_address,
            name,
            last_update_timestamp: Instant::now(),
            connection_status: ConnectionStatus::Unknown,
        })
    }

    pub fn node_id(&self) -> String {
        self.mac_address.replace(":", "")
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
        &mut self,
        homie: &HomieDevice,
        readings: &Readings,
    ) -> Result<(), anyhow::Error> {
        println!("{} {} ({})", self.mac_address, readings, self.name);

        let node_id = self.node_id();
        self.last_update_timestamp = Instant::now();
        homie
            .publish_value(
                &node_id,
                Self::PROPERTY_ID_TEMPERATURE,
                format!("{:.2}", readings.temperature),
            )
            .await
            .with_context(|| std::line!().to_string())?;
        homie
            .publish_value(&node_id, Self::PROPERTY_ID_HUMIDITY, readings.humidity)
            .await
            .with_context(|| std::line!().to_string())?;
        homie
            .publish_value(
                &node_id,
                Self::PROPERTY_ID_BATTERY,
                readings.battery_percent,
            )
            .await
            .with_context(|| std::line!().to_string())?;
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
    bt_session: MijiaSession,
) -> Result<(), anyhow::Error> {
    let sensor_names = hashmap_from_file(SENSOR_NAMES_FILENAME)?;

    homie
        .ready()
        .await
        .with_context(|| std::line!().to_string())?;

    let state = Arc::new(Mutex::new(SensorState {
        sensors_to_connect: VecDeque::new(),
        sensors_connected: vec![],
        homie,
    }));

    let mut next_scan_due = Instant::now();

    let t1 = async {
        loop {
            let now = Instant::now();
            if now > next_scan_due
                && state.lock().await.sensors_connected.len() < sensor_names.len()
            {
                next_scan_due = now + SCAN_INTERVAL;
                check_for_sensors(state.clone(), bt_session.clone(), &sensor_names)
                    .await
                    .with_context(|| std::line!().to_string())?;
            }

            {
                let state = &mut *state.lock().await;
                connect_first_sensor_in_queue(
                    bt_session.clone(),
                    &mut state.homie,
                    &mut state.sensors_connected,
                    &mut state.sensors_to_connect,
                )
                .await
                .with_context(|| std::line!().to_string())?;
            }

            {
                let state = &mut *state.lock().await;
                disconnect_first_stale_sensor(
                    &mut state.homie,
                    &mut state.sensors_connected,
                    &mut state.sensors_to_connect,
                )
                .await
                .with_context(|| std::line!().to_string())?;
            }
            time::delay_for(CONNECT_INTERVAL).await;
        }
        #[allow(unreachable_code)]
        Ok(())
    };
    let t2 = async {
        service_bluetooth_event_queue(state.clone(), bt_session.clone())
            .await
            .with_context(|| std::line!().to_string())?;
        Ok(())
    };
    try_join!(t1, t2).map(|((), ())| ())
}

async fn check_for_sensors(
    state: Arc<Mutex<SensorState>>,
    bt_session: MijiaSession,
    sensor_names: &HashMap<String, String>,
) -> Result<(), anyhow::Error> {
    bt_session.start_discovery().await?;

    let sensors = get_sensors(bt_session.clone())
        .await
        .with_context(|| std::line!().to_string())?;
    let state = &mut *state.lock().await;
    for props in sensors {
        // Race Condition: When connecting, we remove from
        // sensors_to_connect before adding to sensors_connected
        if sensor_names.contains_key(&props.mac_address)
            && !state
                .sensors_to_connect
                .iter()
                .chain(state.sensors_connected.iter())
                .find(|s| s.mac_address == props.mac_address)
                .is_some()
        {
            state
                .sensors_to_connect
                .push_back(Sensor::new(props, &sensor_names)?)
        }
    }
    Ok(())
}

async fn connect_first_sensor_in_queue(
    bt_session: MijiaSession,
    homie: &mut HomieDevice,
    sensors_connected: &mut Vec<Sensor>,
    sensors_to_connect: &mut VecDeque<Sensor>,
) -> Result<(), anyhow::Error> {
    println!("{} sensors in queue to connect.", sensors_to_connect.len());
    // Try to connect to a sensor.
    if let Some(mut sensor) = sensors_to_connect.pop_front() {
        println!("Trying to connect to {}", sensor.name);
        match connect_start_sensor(bt_session.clone(), homie, &mut sensor).await {
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
    bt_session: MijiaSession,
    homie: &mut HomieDevice,
    sensor: &mut Sensor,
) -> Result<(), anyhow::Error> {
    println!("Connecting from status: {:?}", sensor.connection_status);
    bt_session
        .connect(&sensor.object_path)
        .await
        .with_context(|| std::line!().to_string())?;
    match start_notify_sensor(bt_session.clone(), &sensor.object_path).await {
        Ok(()) => {
            homie
                .add_node(sensor.as_node())
                .await
                .with_context(|| std::line!().to_string())?;
            sensor.connection_status = ConnectionStatus::Connected;
            sensor.last_update_timestamp = Instant::now();
            Ok(())
        }
        Err(e) => {
            // If starting notifications failed, disconnect so that we start again from a clean
            // state next time.
            match sensor.connection_status {
                ConnectionStatus::Unknown
                | ConnectionStatus::Disconnected
                | ConnectionStatus::MarkedDisconnected
                | ConnectionStatus::WatchdogTimeOut => {
                    sensor.connection_status = ConnectionStatus::SubscribingFailedOnce;
                }
                ConnectionStatus::SubscribingFailedOnce => {
                    bt_session
                        .disconnect(&sensor.object_path)
                        .await
                        .with_context(|| std::line!().to_string())?;
                    sensor.connection_status = ConnectionStatus::Disconnected;
                }
                ConnectionStatus::Connected => panic!("This should never happen."),
            };
            Err(e)
        }
    }
}

/// If a sensor hasn't sent any updates in a while, disconnect it and add it back to the
/// connect queue.
async fn disconnect_first_stale_sensor(
    homie: &mut HomieDevice,
    sensors_connected: &mut Vec<Sensor>,
    sensors_to_connect: &mut VecDeque<Sensor>,
) -> Result<(), anyhow::Error> {
    let now = Instant::now();
    if let Some(sensor_index) = sensors_connected
        .iter()
        .position(|s| now - s.last_update_timestamp > UPDATE_TIMEOUT)
    {
        let mut sensor = sensors_connected.remove(sensor_index);
        println!(
            "No update from {} for {:?}, reconnecting",
            sensor.name,
            now - sensor.last_update_timestamp
        );
        sensor.connection_status = ConnectionStatus::WatchdogTimeOut;
        homie
            .remove_node(&sensor.node_id())
            .await
            .with_context(|| std::line!().to_string())?;
        sensors_to_connect.push_back(sensor);
    }
    Ok(())
}

async fn service_bluetooth_event_queue(
    state: Arc<Mutex<SensorState>>,
    bt_session: MijiaSession,
) -> Result<(), anyhow::Error> {
    println!("Subscribing to events");
    let (msg_match, mut events) = bt_session.event_stream().await?;
    println!("Processing events");
    // Process events until there are none available for the timeout.
    while let Some(event) = events.next().await {
        handle_bluetooth_event(state.clone(), event)
            .await
            .with_context(|| std::line!().to_string())?
    }
    bt_session
        .connection
        .remove_match(msg_match.token())
        .await?;
    panic!("no more events");
}

async fn handle_bluetooth_event(
    state: Arc<Mutex<SensorState>>,
    event: MijiaEvent,
) -> Result<(), anyhow::Error> {
    let state = &mut *state.lock().await;
    let homie = &mut state.homie;
    let sensors_connected = &mut state.sensors_connected;
    let sensors_to_connect = &mut state.sensors_to_connect;
    match event {
        MijiaEvent::Readings {
            object_path,
            readings,
        } => {
            if let Some(sensor) = sensors_connected
                .iter_mut()
                .find(|s| s.object_path == object_path)
            {
                sensor.publish_readings(homie, &readings).await?;
            } else if let Some(sensor_index) = sensors_to_connect
                .iter()
                .position(|s| s.object_path == object_path)
            {
                let mut sensor = sensors_to_connect.remove(sensor_index).unwrap();
                println!(
                    "Got update from disconnected device {}. Connecting.",
                    object_path
                );
                sensor.publish_readings(homie, &readings).await?;
                sensors_connected.push(sensor);
            }
        }
        MijiaEvent::Disconnected { object_path } => {
            if let Some(sensor_index) = sensors_connected
                .iter()
                .position(|s| s.object_path == object_path)
            {
                let mut sensor = sensors_connected.remove(sensor_index);
                println!("{} disconnected", sensor.name);
                sensor.connection_status = ConnectionStatus::MarkedDisconnected;
                homie.remove_node(&sensor.node_id()).await?;
                sensors_to_connect.push_back(sensor);
            } else {
                println!(
                    "{} disconnected but wasn't known to be connected.",
                    object_path
                );
            }
        }
    };

    Ok(())
}
