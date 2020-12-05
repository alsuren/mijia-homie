#![type_length_limit = "1138969"]

use backoff::{future::FutureOperation, ExponentialBackoff};
use futures::stream::StreamExt;
use futures::TryFutureExt;
use homie_device::{HomieDevice, Node, Property};
use itertools::Itertools;
use mijia::{DeviceId, MacAddress, MijiaEvent, MijiaSession, Readings, SensorProps};
use rumqttc::MqttOptions;
use rustls::ClientConfig;
use stable_eyre::eyre;
use stable_eyre::eyre::WrapErr;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
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
// SENSOR_CONNECT_RETRY_TIMEOUT must be smaller than
// SENSOR_CONNECT_RESERVATION_TIMEOUT by at least a couple of dbus timeouts in
// order to avoid races.
const SENSOR_CONNECT_RESERVATION_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const SENSOR_CONNECT_RETRY_TIMEOUT: Duration = Duration::from_secs(60);
const SENSOR_NAMES_FILENAME: &str = "sensor_names.conf";

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    stable_eyre::install()?;
    dotenv::dotenv().wrap_err("reading .env")?;
    pretty_env_logger::init();
    color_backtrace::install();

    let device_id = std::env::var("DEVICE_ID").unwrap_or_else(|_| DEFAULT_DEVICE_ID.to_string());
    let device_name =
        std::env::var("DEVICE_NAME").unwrap_or_else(|_| DEFAULT_DEVICE_NAME.to_string());

    let mqtt_options = get_mqtt_options(&device_id);
    let mqtt_prefix =
        std::env::var("MQTT_PREFIX").unwrap_or_else(|_| DEFAULT_MQTT_PREFIX.to_string());
    let device_base = format!("{}/{}", mqtt_prefix, device_id);
    let mut homie_builder = HomieDevice::builder(&device_base, &device_name, mqtt_options);
    homie_builder.set_firmware(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    let (homie, homie_handle) = homie_builder.spawn().await?;

    let local = task::LocalSet::new();

    // Connect a Bluetooth session.
    let (dbus_handle, session) = MijiaSession::new().await?;

    let sensor_handle = local.run_until(async move { run_sensor_system(homie, &session).await });

    // Poll everything to completion, until the first one bombs out.
    let res: Result<_, eyre::Report> = try_join! {
        // If this ever finishes, we lost connection to D-Bus.
        dbus_handle.err_into(),
        // Bluetooth finished first. Convert error and get on with your life.
        sensor_handle.err_into(),
        // MQTT event loop finished first.
        homie_handle.err_into(),
    };
    res?;
    Ok(())
}

/// Construct the `MqttOptions` for connecting to the MQTT broker based on configuration options or
/// defaults.
fn get_mqtt_options(device_id: &str) -> MqttOptions {
    let client_name = std::env::var("CLIENT_NAME").unwrap_or_else(|_| device_id.to_owned());

    let host = std::env::var("HOST").unwrap_or_else(|_| DEFAULT_HOST.to_string());
    let port = std::env::var("PORT")
        .ok()
        .and_then(|val| val.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);

    let mut mqtt_options = MqttOptions::new(client_name, host, port);

    let username = std::env::var("USERNAME").ok();
    let password = std::env::var("PASSWORD").ok();

    mqtt_options.set_keep_alive(5);
    if let (Some(u), Some(p)) = (username, password) {
        mqtt_options.set_credentials(u, p);
    }

    // Use `env -u USE_TLS` to unset this variable if you need to clear it.
    if std::env::var("USE_TLS").is_ok() {
        let mut client_config = ClientConfig::new();
        client_config.root_store =
            rustls_native_certs::load_native_certs().expect("could not load platform certs");
        mqtt_options.set_tls_client_config(Arc::new(client_config));
    }
    mqtt_options
}

#[derive(Debug, Copy, Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum ConnectionStatus {
    /// Not yet attempted to connect. Might already be connected from a previous
    /// run of this program.
    Unknown,
    /// Currently connecting. Don't try again until the timeout expires.
    Connecting { reserved_until: Instant },
    /// We explicity disconnected, either because we failed to connect or
    /// because we stopped receiving updates. The device is definitely
    /// disconnected now. Promise.
    Disconnected,
    /// We received a Disconnected event.
    /// This should only be treated as informational, because disconnection
    /// events might be received racily. The sensor might actually be Connected.
    MarkedDisconnected,
    /// Connected and subscribed to updates
    Connected,
}

#[derive(Debug, Clone)]
struct Sensor {
    id: DeviceId,
    mac_address: MacAddress,
    name: String,
    last_update_timestamp: Instant,
    connection_status: ConnectionStatus,
}

impl Sensor {
    const PROPERTY_ID_TEMPERATURE: &'static str = "temperature";
    const PROPERTY_ID_HUMIDITY: &'static str = "humidity";
    const PROPERTY_ID_BATTERY: &'static str = "battery";

    pub fn new(props: SensorProps, sensor_names: &HashMap<MacAddress, String>) -> Self {
        let name = sensor_names
            .get(&props.mac_address)
            .cloned()
            .unwrap_or_else(|| props.mac_address.to_string());
        Self {
            id: props.id,
            mac_address: props.mac_address,
            name,
            last_update_timestamp: Instant::now(),
            connection_status: ConnectionStatus::Unknown,
        }
    }

    pub fn node_id(&self) -> String {
        self.mac_address.to_string().replace(":", "")
    }

    fn as_node(&self) -> Node {
        Node::new(
            &self.node_id(),
            &self.name,
            "Mijia sensor",
            vec![
                Property::float(
                    Self::PROPERTY_ID_TEMPERATURE,
                    "Temperature",
                    false,
                    Some("ºC"),
                    None,
                ),
                Property::integer(
                    Self::PROPERTY_ID_HUMIDITY,
                    "Humidity",
                    false,
                    Some("%"),
                    None,
                ),
                Property::integer(
                    Self::PROPERTY_ID_BATTERY,
                    "Battery level",
                    false,
                    Some("%"),
                    None,
                ),
            ],
        )
    }

    async fn publish_readings(
        &mut self,
        homie: &HomieDevice,
        readings: &Readings,
    ) -> Result<(), eyre::Report> {
        println!("{} {} ({})", self.mac_address, readings, self.name);

        let node_id = self.node_id();
        self.last_update_timestamp = Instant::now();
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

    async fn mark_connected(&mut self, homie: &mut HomieDevice) -> Result<(), eyre::Report> {
        homie.add_node(self.as_node()).await?;
        self.connection_status = ConnectionStatus::Connected;
        Ok(())
    }

    fn name_with_adapter(&self) -> String {
        format!("{}/{}", self.name, self.id.adapter())
    }
}

async fn run_sensor_system(
    mut homie: HomieDevice,
    session: &MijiaSession,
) -> Result<(), eyre::Report> {
    let sensor_names = hashmap_from_file(SENSOR_NAMES_FILENAME)
        .wrap_err(format!("reading {}", SENSOR_NAMES_FILENAME))?;

    homie.ready().await?;

    let state = Arc::new(Mutex::new(SensorState {
        sensors: HashMap::new(),
        homie,
    }));

    let connection_loop_handle = bluetooth_connection_loop(state.clone(), session, &sensor_names);
    let event_loop_handle = service_bluetooth_event_queue(state.clone(), session);
    try_join!(connection_loop_handle, event_loop_handle).map(|((), ())| ())
}

/// Read the given file of key-value pairs into a hashmap.
/// Returns an empty hashmap if the file doesn't exist, or an error if it is malformed.
fn hashmap_from_file(filename: &str) -> Result<HashMap<MacAddress, String>, eyre::Report> {
    let mut map: HashMap<MacAddress, String> = HashMap::new();
    if let Ok(file) = File::open(filename) {
        for line in BufReader::new(file).lines() {
            let line = line?;
            if !line.starts_with('#') {
                let parts: Vec<&str> = line.splitn(2, '=').collect();
                if parts.len() != 2 {
                    eyre::bail!("Invalid line '{}'", line);
                }
                map.insert(parts[0].parse()?, parts[1].to_string());
            }
        }
    }
    Ok(map)
}

async fn bluetooth_connection_loop(
    state: Arc<Mutex<SensorState>>,
    session: &MijiaSession,
    sensor_names: &HashMap<MacAddress, String>,
) -> Result<(), eyre::Report> {
    let mut next_scan_due = Instant::now();
    loop {
        // Print count and list of sensors in each state.
        {
            let counts = state
                .lock()
                .await
                .sensors
                .values()
                .map(|sensor| (sensor.connection_status, sensor.name_with_adapter()))
                .into_group_map();
            for (state, names) in counts.iter().sorted() {
                println!("{:?}: {} {:?}", state, names.len(), names);
            }
        }

        // Look for more sensors if enough time has elapsed since last time we tried.
        let now = Instant::now();
        if now > next_scan_due && state.lock().await.sensors.len() < sensor_names.len() {
            next_scan_due = now + SCAN_INTERVAL;
            check_for_sensors(state.clone(), session, &sensor_names).await?;
        }

        // Check the state of each sensor and act on it if appropriate.
        {
            let ids: Vec<DeviceId> = state.lock().await.sensors.keys().cloned().collect();
            for id in ids {
                let connection_status = state
                    .lock()
                    .await
                    .sensors
                    .get(&id)
                    .map(|sensor| {
                        log::trace!(
                            "State of {} is {:?}",
                            sensor.name_with_adapter(),
                            sensor.connection_status
                        );
                        sensor.connection_status
                    })
                    .expect("sensors cannot be deleted");
                action_sensor(state.clone(), session, id, connection_status).await?;
            }
        }
        time::delay_for(CONNECT_INTERVAL).await;
    }
}

#[derive(Debug)]
struct SensorState {
    sensors: HashMap<DeviceId, Sensor>,
    homie: HomieDevice,
}

async fn action_sensor(
    state: Arc<Mutex<SensorState>>,
    session: &MijiaSession,
    id: DeviceId,
    status: ConnectionStatus,
) -> Result<(), eyre::Report> {
    match status {
        ConnectionStatus::Connecting { reserved_until } if reserved_until > Instant::now() => {
            Ok(())
        }
        ConnectionStatus::Unknown
        | ConnectionStatus::Connecting { .. }
        | ConnectionStatus::Disconnected
        | ConnectionStatus::MarkedDisconnected => {
            connect_sensor_with_id(state, session, id).await?;
            Ok(())
        }
        ConnectionStatus::Connected => {
            check_for_stale_sensor(state, session, id).await?;
            Ok(())
        }
    }
}

async fn check_for_sensors(
    state: Arc<Mutex<SensorState>>,
    session: &MijiaSession,
    sensor_names: &HashMap<MacAddress, String>,
) -> Result<(), eyre::Report> {
    session.bt_session.start_discovery().await?;

    let sensors = session.get_sensors().await?;
    let state = &mut *state.lock().await;
    for props in sensors {
        if sensor_names.contains_key(&props.mac_address)
            && !state
                .sensors
                .values()
                .any(|s| s.mac_address == props.mac_address)
        {
            let sensor = Sensor::new(props, &sensor_names);
            state.sensors.insert(sensor.id.clone(), sensor);
        }
    }
    Ok(())
}

async fn connect_sensor_with_id(
    state: Arc<Mutex<SensorState>>,
    session: &MijiaSession,
    id: DeviceId,
) -> Result<(), eyre::Report> {
    // Update the state of the sensor to `Connecting`.
    {
        let mut state = state.lock().await;
        let sensor = state.sensors.get_mut(&id).unwrap();
        println!(
            "Trying to connect to {} from status: {:?}",
            sensor.name_with_adapter(),
            sensor.connection_status
        );
        sensor.connection_status = ConnectionStatus::Connecting {
            reserved_until: Instant::now() + SENSOR_CONNECT_RESERVATION_TIMEOUT,
        };
    };
    let result = connect_and_subscribe_sensor_or_disconnect(session, &id).await;

    let state = &mut *state.lock().await;
    let sensor = state.sensors.get_mut(&id).unwrap();
    match result {
        Ok(()) => {
            println!(
                "Connected to {} and started notifications",
                sensor.name_with_adapter()
            );
            sensor.mark_connected(&mut state.homie).await?;
            sensor.last_update_timestamp = Instant::now();
        }
        Err(e) => {
            println!(
                "Failed to connect to {}: {:?}",
                sensor.name_with_adapter(),
                e
            );
            sensor.connection_status = ConnectionStatus::Disconnected;
        }
    }
    Ok(())
}

async fn connect_and_subscribe_sensor_or_disconnect<'a>(
    session: &MijiaSession,
    id: &DeviceId,
) -> Result<(), eyre::Report> {
    session
        .bt_session
        .connect(id)
        .await
        .wrap_err_with(|| format!("connecting to {:?}", id))?;

    let mut backoff = ExponentialBackoff::default();
    backoff.max_elapsed_time = Some(SENSOR_CONNECT_RETRY_TIMEOUT);

    FutureOperation::retry(
        || session.start_notify_sensor(id).map_err(Into::into),
        backoff,
    )
    .or_else(|e| async {
        session
            .bt_session
            .disconnect(id)
            .await
            .wrap_err_with(|| format!("disconnecting from {:?}", id))?;
        Err(e.into())
    })
    .await
}

/// If the sensor hasn't sent any updates in a while, disconnect it so we will try to reconnect.
async fn check_for_stale_sensor(
    state: Arc<Mutex<SensorState>>,
    session: &MijiaSession,
    id: DeviceId,
) -> Result<(), eyre::Report> {
    let state = &mut *state.lock().await;
    let sensor = state.sensors.get_mut(&id).unwrap();
    let now = Instant::now();
    if now - sensor.last_update_timestamp > UPDATE_TIMEOUT {
        println!(
            "No update from {} for {:?}, reconnecting",
            sensor.name_with_adapter(),
            now - sensor.last_update_timestamp
        );
        sensor.connection_status = ConnectionStatus::Disconnected;
        state.homie.remove_node(&sensor.node_id()).await?;
        // We could drop our state lock at this point, if it ends up taking
        // too long. As it is, it's quite nice that we can't attempt to connect
        // while we're in the middle of disconnecting.
        session
            .bt_session
            .disconnect(&id)
            .await
            .wrap_err_with(|| format!("disconnecting from {:?}", id))?;
    }
    Ok(())
}

async fn service_bluetooth_event_queue(
    state: Arc<Mutex<SensorState>>,
    session: &MijiaSession,
) -> Result<(), eyre::Report> {
    println!("Subscribing to events");
    let (msg_match, mut events) = session.event_stream().await?;
    println!("Processing events");

    while let Some(event) = events.next().await {
        handle_bluetooth_event(state.clone(), event).await?
    }

    session
        .bt_session
        .connection
        .remove_match(msg_match.token())
        .await?;
    // This should be unreachable, because the events Stream should never end,
    // unless something has gone horribly wrong (or msg_match got dropped?)
    panic!("no more events");
}

async fn handle_bluetooth_event(
    state: Arc<Mutex<SensorState>>,
    event: MijiaEvent,
) -> Result<(), eyre::Report> {
    let state = &mut *state.lock().await;
    let homie = &mut state.homie;
    let sensors = &mut state.sensors;
    match event {
        MijiaEvent::Readings { id, readings } => {
            if let Some(sensor) = sensors.get_mut(&id) {
                sensor.publish_readings(homie, &readings).await?;
                match sensor.connection_status {
                    ConnectionStatus::Connected | ConnectionStatus::Connecting { .. } => {}
                    _ => {
                        println!("Got update from disconnected device {:?}. Connecting.", id);
                        sensor.mark_connected(homie).await?;
                        // TODO: Make sure the connection interval is set.
                    }
                }
            } else {
                println!("Got update from unknown device {:?}.", id);
            }
        }
        MijiaEvent::Disconnected { id } => {
            if let Some(sensor) = sensors.get_mut(&id) {
                if sensor.connection_status == ConnectionStatus::Connected {
                    println!("{} disconnected", sensor.name_with_adapter());
                    sensor.connection_status = ConnectionStatus::MarkedDisconnected;
                    homie.remove_node(&sensor.node_id()).await?;
                } else {
                    println!("{:?} disconnected but wasn't known to be connected.", id);
                }
            } else {
                println!("Unknown device {:?} disconnected.", id);
            }
        }
        _ => {}
    };

    Ok(())
}
