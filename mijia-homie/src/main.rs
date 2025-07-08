#![type_length_limit = "1138969"]

mod config;

use crate::config::{Config, get_mqtt_options, read_sensor_names};
use backoff::ExponentialBackoff;
use backoff::future::retry;
use btsensor::Reading;
use btsensor::bthome::{self, v1::Element};
use eyre::{Report, eyre};
use futures::TryFutureExt;
use futures::stream::StreamExt;
use homie_device::{HomieDevice, Node, Property};
use itertools::Itertools;
use log::{debug, info};
use mijia::bluetooth::{
    BluetoothError, BluetoothEvent, BluetoothSession, DeviceEvent, DeviceId, MacAddress,
};
use mijia::{MijiaEvent, MijiaSession, Readings, SensorProps};
use stable_eyre::eyre;
use stable_eyre::eyre::WrapErr;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::{
    sync::Mutex,
    time::{self, sleep},
    try_join,
};

const SCAN_INTERVAL: Duration = Duration::from_secs(20);
const CONNECT_INTERVAL: Duration = Duration::from_secs(1);
const UPDATE_TIMEOUT: Duration = Duration::from_secs(60);
// SENSOR_CONNECT_RETRY_TIMEOUT must be smaller than
// SENSOR_CONNECT_RESERVATION_TIMEOUT by at least a couple of dbus timeouts in
// order to avoid races.
const SENSOR_CONNECT_RESERVATION_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const SENSOR_CONNECT_RETRY_TIMEOUT: Duration = Duration::from_secs(60);
const BLUETOOTH_RESTART_DELAY: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    stable_eyre::install()?;
    pretty_env_logger::init();
    color_backtrace::install();

    let config = Config::from_file()?;
    let sensor_names = read_sensor_names(&config.homie.sensor_names_filename)?;

    let mqtt_options = get_mqtt_options(config.mqtt, &config.homie.device_id);
    let device_base = format!("{}/{}", config.homie.prefix, config.homie.device_id);
    let mut homie_builder =
        HomieDevice::builder(&device_base, &config.homie.device_name, mqtt_options);
    homie_builder.set_firmware(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    let (homie, homie_handle) = homie_builder.spawn().await?;

    // Connect a Bluetooth session.
    let (dbus_handle, session) = MijiaSession::new().await?;

    let min_update_period = config.homie.min_update_period;
    let sensor_handle = run_sensor_system(
        homie,
        &session,
        &sensor_names,
        min_update_period,
        config.homie.auto_restart_bluetooth,
    );

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

#[derive(Debug, Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
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
    Connected { id: DeviceId },
    /// This sensor gives updates by advertisements only, there is no need to connect.
    AdvertisementOnly,
}

#[derive(Debug, Clone)]
struct Sensor {
    mac_address: MacAddress,
    name: String,
    /// The last time an update was received from the sensor.
    last_update_timestamp: Instant,
    /// The last time an update from the sensor was sent to the server. This may be earlier than
    /// `last_update_timestamp` if the `min_update_time` config parameter is set.
    last_sent_timestamp: Instant,
    connection_status: ConnectionStatus,
    ids: Vec<DeviceId>,
}

impl Sensor {
    const PROPERTY_ID_TEMPERATURE: &'static str = "temperature";
    const PROPERTY_ID_HUMIDITY: &'static str = "humidity";
    const PROPERTY_ID_BATTERY: &'static str = "battery";

    pub fn new(
        props: SensorProps,
        sensor_names: &HashMap<MacAddress, String>,
        connection_status: ConnectionStatus,
    ) -> Self {
        let name = sensor_names
            .get(&props.mac_address)
            .cloned()
            .unwrap_or_else(|| props.mac_address.to_string());
        Self {
            mac_address: props.mac_address,
            name,
            last_update_timestamp: Instant::now(),
            // This should really be something like Instant::MIN, but there is no such constant so
            // one hour in the past should be more than enough.
            last_sent_timestamp: Instant::now() - Duration::from_secs(3600),
            connection_status,
            ids: vec![props.id],
        }
    }

    pub fn node_id(&self) -> String {
        self.mac_address.to_string().replace(':', "")
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
                    true,
                    Some("ºC"),
                    None,
                ),
                Property::integer(
                    Self::PROPERTY_ID_HUMIDITY,
                    "Humidity",
                    false,
                    true,
                    Some("%"),
                    None,
                ),
                Property::integer(
                    Self::PROPERTY_ID_BATTERY,
                    "Battery level",
                    false,
                    true,
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
        min_update_period: Duration,
    ) -> Result<(), eyre::Report> {
        println!("{} {} ({})", self.mac_address, readings, self.name);
        let now = Instant::now();
        self.last_update_timestamp = now;

        if now > self.last_sent_timestamp + min_update_period {
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
            self.last_sent_timestamp = now;
        } else {
            log::trace!(
                "Not sending, as last update sent {} seconds ago.",
                (now - self.last_sent_timestamp).as_secs()
            );
        }

        Ok(())
    }

    async fn publish_reading(
        &mut self,
        homie: &HomieDevice,
        reading: &Reading,
        min_update_period: Duration,
    ) -> Result<(), eyre::Report> {
        println!("{} {} ({})", self.mac_address, reading, self.name);
        let now = Instant::now();
        self.last_update_timestamp = now;

        if now > self.last_sent_timestamp + min_update_period {
            let node_id = self.node_id();
            match reading {
                Reading::Atc(atc) => {
                    homie
                        .publish_value(
                            &node_id,
                            Self::PROPERTY_ID_TEMPERATURE,
                            format!("{:.2}", atc.temperature()),
                        )
                        .await?;
                    homie
                        .publish_value(&node_id, Self::PROPERTY_ID_HUMIDITY, atc.humidity() as i64)
                        .await?;
                    homie
                        .publish_value(&node_id, Self::PROPERTY_ID_BATTERY, atc.battery_percent())
                        .await?;
                }
                Reading::BtHomeV1(elements) => {
                    // TODO: Generate Homie properties based on elements.
                    for element in elements {
                        if let Element::Sensor(sensor) = element {
                            match sensor.property {
                                bthome::v1::Property::Temperature => {
                                    homie
                                        .publish_value(
                                            &node_id,
                                            Self::PROPERTY_ID_TEMPERATURE,
                                            format!("{:.2}", sensor.value_float()),
                                        )
                                        .await?;
                                }
                                bthome::v1::Property::Humidity
                                | bthome::v1::Property::HumidityShort => {
                                    homie
                                        .publish_value(
                                            &node_id,
                                            Self::PROPERTY_ID_HUMIDITY,
                                            sensor.value_float() as i64,
                                        )
                                        .await?;
                                }
                                bthome::v1::Property::Battery => {
                                    if let Some(value) = sensor.value_int() {
                                        homie
                                            .publish_value(
                                                &node_id,
                                                Self::PROPERTY_ID_BATTERY,
                                                value,
                                            )
                                            .await?;
                                    }
                                }
                                bthome::v1::Property::PacketId => {}
                                _ => {
                                    debug!(
                                        "Skipping unexpected BTHome v1 element {} on {} ({})",
                                        element, self.mac_address, self.name
                                    );
                                }
                            }
                        }
                    }
                }
                Reading::BtHomeV2(bthome) => {
                    // TODO: Generate Homie properties based on elements.
                    for element in &bthome.elements {
                        match element.name() {
                            "temperature" => {
                                homie
                                    .publish_value(
                                        &node_id,
                                        Self::PROPERTY_ID_TEMPERATURE,
                                        format!("{:.2}", element.value_float().unwrap()),
                                    )
                                    .await?;
                            }
                            "humidity" => {
                                homie
                                    .publish_value(
                                        &node_id,
                                        Self::PROPERTY_ID_HUMIDITY,
                                        element.value_float().unwrap() as i64,
                                    )
                                    .await?;
                            }
                            "battery" => {
                                homie
                                    .publish_value(
                                        &node_id,
                                        Self::PROPERTY_ID_BATTERY,
                                        element.value_int().unwrap(),
                                    )
                                    .await?;
                            }
                            _ => {
                                debug!(
                                    "Skipping unexpected BTHome v2 element {} on {} ({})",
                                    element, self.mac_address, self.name
                                );
                            }
                        }
                    }
                }
            }
            self.last_sent_timestamp = now;
        } else {
            log::trace!(
                "Not sending, as last update sent {} seconds ago.",
                (now - self.last_sent_timestamp).as_secs()
            );
        }

        Ok(())
    }

    async fn mark_connected(
        &mut self,
        homie: &mut HomieDevice,
        id: DeviceId,
    ) -> Result<(), eyre::Report> {
        assert!(self.ids.contains(&id));
        homie.add_node(self.as_node()).await?;
        self.connection_status = ConnectionStatus::Connected { id };
        Ok(())
    }

    /// Adds a Homie node for an advertisement-only sensor.
    async fn publish_advertisement_only(
        &mut self,
        homie: &mut HomieDevice,
    ) -> Result<(), eyre::Report> {
        assert_eq!(self.connection_status, ConnectionStatus::AdvertisementOnly);
        homie.add_node(self.as_node()).await?;
        Ok(())
    }
}

async fn run_sensor_system(
    mut homie: HomieDevice,
    session: &MijiaSession,
    sensor_names: &HashMap<MacAddress, String>,
    min_update_period: Duration,
    auto_restart_bluetooth: bool,
) -> Result<(), eyre::Report> {
    homie.ready().await?;

    let state = Arc::new(Mutex::new(SensorState {
        sensors: HashMap::new(),
        homie,
        min_update_period,
    }));

    let connection_loop_handle =
        bluetooth_connection_loop(state.clone(), session, sensor_names, auto_restart_bluetooth);
    let bluetooth_event_loop_handle =
        service_bluetooth_event_queue(state.clone(), &session.bt_session, sensor_names);
    try_join!(connection_loop_handle, bluetooth_event_loop_handle).map(|((), ())| ())
}

async fn bluetooth_connection_loop(
    state: Arc<Mutex<SensorState>>,
    session: &MijiaSession,
    sensor_names: &HashMap<MacAddress, String>,
    auto_restart_bluetooth: bool,
) -> Result<(), eyre::Report> {
    let mut next_scan_due = Instant::now();
    loop {
        // Print count and list of sensors in each state.
        {
            let state = state.lock().await;
            let counts = state
                .sensors
                .values()
                .map(|sensor| (&sensor.connection_status, sensor.name.to_owned()))
                .into_group_map();
            for (state, names) in counts.iter().sorted() {
                println!("{:?}: {} {:?}", state, names.len(), names);
            }
        }

        // Look for more sensors if enough time has elapsed since last time we tried.
        let now = Instant::now();
        if now > next_scan_due && state.lock().await.sensors.len() < sensor_names.len() {
            next_scan_due = now + SCAN_INTERVAL;
            check_for_sensors(state.clone(), session, sensor_names, auto_restart_bluetooth).await?;
        }

        // Check the state of each sensor and act on it if appropriate.
        {
            let mac_addresses: Vec<MacAddress> =
                state.lock().await.sensors.keys().cloned().collect();
            for mac_address in mac_addresses {
                let connection_status = state
                    .lock()
                    .await
                    .sensors
                    .get(&mac_address)
                    .map(|sensor| {
                        log::trace!("State of {} is {:?}", sensor.name, sensor.connection_status);
                        sensor.connection_status.to_owned()
                    })
                    .expect("sensors cannot be deleted");
                action_sensor(state.clone(), session, &mac_address, connection_status).await?;
            }
        }
        time::sleep(CONNECT_INTERVAL).await;
    }
}

#[derive(Debug)]
struct SensorState {
    sensors: HashMap<MacAddress, Sensor>,
    homie: HomieDevice,
    min_update_period: Duration,
}

impl SensorState {
    /// Adds the given sensor to the list of sensors, if it's in `sensor_names` but not already
    /// present.
    ///
    /// If it's already present with a different ID, adds this ID.
    ///
    /// Returns true if the sensor was added to list of sensors, or false if it was already there or
    /// doesn't have a name.
    fn add_sensor_if_named(
        &mut self,
        sensor_names: &HashMap<MacAddress, String>,
        props: SensorProps,
        connection_status: ConnectionStatus,
    ) -> bool {
        if sensor_names.contains_key(&props.mac_address) {
            if let Some(sensor) = self.sensors.get_mut(&props.mac_address) {
                if !sensor.ids.contains(&props.id) {
                    // If we already know about the sensor but on a different Bluetooth adapter, add
                    // this one too.
                    sensor.ids.push(props.id);
                }
                false
            } else {
                // If we don't know about the sensor on any adapter, add it.
                let sensor = Sensor::new(props, sensor_names, connection_status);
                self.sensors.insert(sensor.mac_address, sensor);
                true
            }
        } else {
            false
        }
    }
}

/// Get the sensor entry for the given id, if any.
fn get_mut_sensor_by_id<'a>(
    sensors: &'a mut HashMap<MacAddress, Sensor>,
    id: &DeviceId,
) -> Option<&'a mut Sensor> {
    sensors.values_mut().find(|sensor| sensor.ids.contains(id))
}

async fn action_sensor(
    state: Arc<Mutex<SensorState>>,
    session: &MijiaSession,
    mac_address: &MacAddress,
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
            connect_sensor_with_id(state, session, mac_address).await?;
            Ok(())
        }
        ConnectionStatus::Connected { id } => {
            check_for_stale_sensor(state, session, mac_address, &id).await?;
            Ok(())
        }
        // TODO: Should we forget about these sensors if we don't see them for a while?
        ConnectionStatus::AdvertisementOnly => Ok(()),
    }
}

/// If an adapter seems not to be scanning properly, because it doesn't see any devices other than
/// those which are connected, then try powering it off and back on again.
///
/// This is supposed to work around a bug on Raspberry Pi devices.
async fn bluetooth_powercycle(session: &BluetoothSession) -> Result<(), eyre::Report> {
    for adapter in session.get_adapters().await? {
        let devices = session.get_devices_on_adapter(&adapter.id).await?;
        if !devices.is_empty() && devices.iter().all(|device| device.connected) {
            info!(
                "Scanning seems to have broken, powering off adapter {}",
                adapter.id
            );
            session.set_powered(&adapter.id, false).await?;
            sleep(BLUETOOTH_RESTART_DELAY).await;
            info!("Powering adapter {} on again", adapter.id);
            session.set_powered(&adapter.id, true).await?;
        }
    }
    Ok(())
}

async fn check_for_sensors(
    state: Arc<Mutex<SensorState>>,
    session: &MijiaSession,
    sensor_names: &HashMap<MacAddress, String>,
    auto_restart_bluetooth: bool,
) -> Result<(), eyre::Report> {
    if auto_restart_bluetooth {
        bluetooth_powercycle(&session.bt_session).await?;
    }

    session.bt_session.start_discovery().await?;

    let sensors = session.get_sensors().await?;
    let state = &mut *state.lock().await;
    for props in sensors {
        state.add_sensor_if_named(sensor_names, props, ConnectionStatus::Unknown);
    }
    Ok(())
}

async fn connect_sensor_with_id(
    state: Arc<Mutex<SensorState>>,
    session: &MijiaSession,
    mac_address: &MacAddress,
) -> Result<(), eyre::Report> {
    let (name, ids) = {
        let mut state = state.lock().await;
        let sensor = state.sensors.get_mut(mac_address).unwrap();

        // Update the state of the sensor to `Connecting`.
        println!(
            "Trying to connect to {} from status: {:?}",
            sensor.name, sensor.connection_status
        );
        sensor.connection_status = ConnectionStatus::Connecting {
            reserved_until: Instant::now() + SENSOR_CONNECT_RESERVATION_TIMEOUT,
        };
        (sensor.name.clone(), sensor.ids.clone())
    };
    let result = connect_and_subscribe_sensor_or_disconnect(session, &name, ids).await;

    let state = &mut *state.lock().await;
    let sensor = state.sensors.get_mut(mac_address).unwrap();
    match result {
        Ok(id) => {
            println!("Connected to {} and started notifications", sensor.name);
            sensor.mark_connected(&mut state.homie, id).await?;
            sensor.last_update_timestamp = Instant::now();
        }
        Err(e) => {
            println!("Failed to connect to {}: {:?}", sensor.name, e);
            sensor.connection_status = ConnectionStatus::Disconnected;
        }
    }
    Ok(())
}

/// Try to connect to the ids in turn, and get the first one that succeeds. If they all fail then
/// return an error.
async fn try_connect_all(
    session: &BluetoothSession,
    ids: Vec<DeviceId>,
) -> Result<DeviceId, Vec<BluetoothError>> {
    let mut errors = vec![];
    for id in ids {
        if let Err(e) = session.connect(&id).await {
            errors.push(e);
        } else {
            return Ok(id);
        }
    }
    Err(errors)
}

async fn connect_and_subscribe_sensor_or_disconnect(
    session: &MijiaSession,
    name: &str,
    ids: Vec<DeviceId>,
) -> Result<DeviceId, eyre::Report> {
    let id = try_connect_all(&session.bt_session, ids)
        .await
        .map_err(|e| eyre!("Error connecting to {}: {:?}", name, e))?;

    // We managed to connect to the sensor via some id, now try to start notifications for readings.
    retry(
        ExponentialBackoff {
            max_elapsed_time: Some(SENSOR_CONNECT_RETRY_TIMEOUT),
            ..Default::default()
        },
        || session.start_notify_sensor(&id).map_err(Into::into),
    )
    .or_else(|e| async {
        session
            .bt_session
            .disconnect(&id)
            .await
            .wrap_err_with(|| format!("Disconnecting from {name} ({id})"))?;
        Err(Report::new(e).wrap_err(format!("Starting notifications on {name} ({id})")))
    })
    .await?;

    Ok(id)
}

/// If the sensor hasn't sent any updates in a while, disconnect it so we will try to reconnect.
async fn check_for_stale_sensor(
    state: Arc<Mutex<SensorState>>,
    session: &MijiaSession,
    mac_address: &MacAddress,
    id: &DeviceId,
) -> Result<(), eyre::Report> {
    let state = &mut *state.lock().await;
    let sensor = state.sensors.get_mut(mac_address).unwrap();
    let now = Instant::now();
    if now - sensor.last_update_timestamp > UPDATE_TIMEOUT {
        println!(
            "No update from {} for {:?}, reconnecting",
            sensor.name,
            now - sensor.last_update_timestamp
        );
        sensor.connection_status = ConnectionStatus::Disconnected;
        state.homie.remove_node(&sensor.node_id()).await?;
        // We could drop our state lock at this point, if it ends up taking
        // too long. As it is, it's quite nice that we can't attempt to connect
        // while we're in the middle of disconnecting.
        session
            .bt_session
            .disconnect(id)
            .await
            .wrap_err_with(|| format!("disconnecting from {id}"))?;
    }
    Ok(())
}

/// Waits for and handles events from the `BluetoothSession`.
async fn service_bluetooth_event_queue(
    state: Arc<Mutex<SensorState>>,
    session: &BluetoothSession,
    sensor_names: &HashMap<MacAddress, String>,
) -> Result<(), eyre::Report> {
    debug!("Subscribing to events");
    let mut events = session.event_stream().await?;
    debug!("Processing events");

    while let Some(event) = events.next().await {
        handle_bluetooth_event(state.clone(), event, session, sensor_names).await?;
    }

    // This should be unreachable, because the events Stream should never end,
    // unless something has gone horribly wrong.
    panic!("No more events");
}

async fn handle_bluetooth_event(
    state: Arc<Mutex<SensorState>>,
    event: BluetoothEvent,
    session: &BluetoothSession,
    sensor_names: &HashMap<MacAddress, String>,
) -> Result<(), eyre::Report> {
    if let Some(mijia_event) = MijiaEvent::from(event.clone(), session.clone()).await {
        handle_mijia_event(state.clone(), mijia_event).await?;
    }

    if let BluetoothEvent::Device {
        id,
        event: DeviceEvent::ServiceData { service_data },
    } = event
    {
        if let Some(reading) = Reading::decode(&service_data) {
            info!("{id}: {reading}");
            let mac_address = session.get_device_info(&id).await?.mac_address;
            let state = &mut *state.lock().await;
            let is_new = state.add_sensor_if_named(
                sensor_names,
                SensorProps {
                    id: id.clone(),
                    mac_address,
                },
                ConnectionStatus::AdvertisementOnly,
            );
            let homie = &mut state.homie;
            let sensors = &mut state.sensors;
            // This will only return None if the sensor doesn't have a name.
            if let Some(sensor) = get_mut_sensor_by_id(sensors, &id) {
                assert_eq!(
                    sensor.connection_status,
                    ConnectionStatus::AdvertisementOnly
                );
                if is_new {
                    sensor.publish_advertisement_only(homie).await?;
                }
                sensor
                    .publish_reading(homie, &reading, state.min_update_period)
                    .await?;
            }
        }
    }

    Ok(())
}

async fn handle_mijia_event(
    state: Arc<Mutex<SensorState>>,
    event: MijiaEvent,
) -> Result<(), eyre::Report> {
    let state = &mut *state.lock().await;
    let homie = &mut state.homie;
    let sensors = &mut state.sensors;
    match event {
        MijiaEvent::Readings { id, readings } => {
            if let Some(sensor) = get_mut_sensor_by_id(sensors, &id) {
                sensor
                    .publish_readings(homie, &readings, state.min_update_period)
                    .await?;
                match &sensor.connection_status {
                    ConnectionStatus::Connected { id: connected_id } => {
                        if id != *connected_id {
                            log::info!(
                                "Got update from device on unexpected id {id} (expected {connected_id})",
                            );
                        }
                    }
                    ConnectionStatus::Connecting { .. } => {}
                    _ => {
                        println!("Got update from disconnected device {id}. Connecting.");
                        sensor.mark_connected(homie, id).await?;
                        // TODO: Make sure the connection interval is set.
                    }
                }
            } else {
                println!("Got update from unknown device {id}.");
            }
        }
        MijiaEvent::Disconnected { id } => {
            if let Some(sensor) = get_mut_sensor_by_id(sensors, &id) {
                if let ConnectionStatus::Connected { id: connected_id } = &sensor.connection_status
                {
                    if id == *connected_id {
                        println!("{} disconnected", sensor.name);
                        sensor.connection_status = ConnectionStatus::MarkedDisconnected;
                        homie.remove_node(&sensor.node_id()).await?;
                    } else {
                        println!(
                            "{} ({}) disconnected but was connected as {}.",
                            sensor.name, id, connected_id
                        );
                    }
                } else {
                    println!(
                        "{} ({}) disconnected but wasn't known to be connected.",
                        sensor.name, id
                    );
                }
            } else {
                println!("Unknown device {id} disconnected.");
            }
        }
        _ => {}
    };

    Ok(())
}
