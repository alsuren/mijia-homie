use async_channel::{SendError, Sender};
use blurz::{
    BluetoothAdapter, BluetoothDevice, BluetoothDiscoverySession, BluetoothEvent, BluetoothSession,
};
use futures::FutureExt;
use mijia::{
    connect_sensors, decode_value, find_sensors, hashmap_from_file, print_sensors,
    start_notify_sensors, SERVICE_CHARACTERISTIC_PATH,
};
use rumqttc::{self, EventLoop, LastWill, MqttOptions, Publish, QoS, Request};
use rustls::ClientConfig;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::{task, time, try_join};

const DEFAULT_MQTT_PREFIX: &str = "homie";
const DEFAULT_DEVICE_ID: &str = "mijia-bridge";
const DEFAULT_DEVICE_NAME: &str = "Mijia bridge";
const DEFAULT_HOST: &str = "test.mosquitto.org";
const DEFAULT_PORT: u16 = 1883;
const SCAN_DURATION: Duration = Duration::from_secs(15);
const INCOMING_TIMEOUT_MS: u32 = 10_000;
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

#[tokio::main(core_threads = 2)]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    dotenv::dotenv()?;
    pretty_env_logger::init();
    color_backtrace::install();

    let device_id = std::env::var("DEVICE_ID").unwrap_or_else(|_| DEFAULT_DEVICE_ID.to_string());
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

    mqttoptions.set_last_will(LastWill {
        topic: format!("{}/$state", device_base),
        message: "lost".to_string(),
        qos: QoS::AtLeastOnce,
        retain: true,
    });

    let local = task::LocalSet::new();

    let mut eventloop = EventLoop::new(mqttoptions, 10).await;
    let requests_tx = eventloop.handle();
    let bluetooth_handle = local.spawn_local(async move {
        requests(requests_tx, &device_base).await.unwrap();
    });

    let mqtt_handle: JoinHandle<Result<(), Box<dyn Error + Send + Sync>>> =
        task::spawn(async move {
            loop {
                let (incoming, outgoing) = eventloop.poll().await?;
                log::trace!("Incoming = {:?}, Outgoing = {:?}", incoming, outgoing);
            }
        });

    // Poll everything to completion, until the first one bombs out.
    let res: Result<_, Box<dyn Error + Send + Sync>> = try_join! {
        // LocalSet finished first. Colour me confused.
        local.map(|()| Ok(println!("WTF?"))),
        // Bluetooth finished first. Convert error and get on with your life.
        bluetooth_handle.map(|res| Ok(res?)),
        // MQTT event loop finished first.
        // Unwrap the JoinHandle Result to get to the real Result.
        mqtt_handle.map(|res| Ok(res??)),
    };
    res?;
    Ok(())
}

async fn publish_retained(
    requests_tx: &Sender<Request>,
    name: String,
    value: &str,
) -> Result<(), SendError<Request>> {
    let mut publish = Publish::new(name, QoS::AtLeastOnce, value);
    publish.set_retain(true);
    requests_tx.send(publish.into()).await
}

async fn requests(requests_tx: Sender<Request>, device_base: &str) -> Result<(), Box<dyn Error>> {
    let device_name =
        std::env::var("DEVICE_NAME").unwrap_or_else(|_| DEFAULT_DEVICE_NAME.to_string());
    let sensor_names = hashmap_from_file(SENSOR_NAMES_FILENAME)?;

    publish_retained(&requests_tx, format!("{}/$homie", device_base), "4.0").await?;
    publish_retained(&requests_tx, format!("{}/$extensions", device_base), "").await?;
    publish_retained(&requests_tx, format!("{}/$name", device_base), &device_name).await?;
    publish_retained(&requests_tx, format!("{}/$state", device_base), "init").await?;

    let bt_session = &BluetoothSession::create_session(None)?;
    let device_list = scan(&bt_session).await?;
    let sensors = find_sensors(&bt_session, &device_list);
    print_sensors(&sensors, &sensor_names);
    let (named_sensors, unnamed_sensors): (Vec<_>, Vec<_>) = sensors
        .into_iter()
        .partition(|sensor| sensor_names.contains_key(&sensor.get_address().unwrap()));
    println!("Connecting to named sensors first");
    let mut connected_sensors = connect_sensors(&named_sensors);
    println!("Connecting to unnamed sensors");
    connected_sensors.extend(connect_sensors(&unnamed_sensors));

    let mut nodes = vec![];
    for sensor in &connected_sensors {
        let mac_address = sensor.get_address()?;
        let node_id = mac_address.replace(":", "");
        let node_base = format!("{}/{}", device_base, node_id);
        let node_name = sensor_names.get(&mac_address).unwrap_or(&mac_address);
        nodes.push(node_id);
        publish_retained(&requests_tx, format!("{}/$name", node_base), node_name).await?;
        publish_retained(&requests_tx, format!("{}/$type", node_base), "Mijia sensor").await?;
        publish_retained(
            &requests_tx,
            format!("{}/$properties", node_base),
            "temperature,humidity,battery",
        )
        .await?;
        publish_retained(
            &requests_tx,
            format!("{}/temperature/$name", node_base),
            "Temperature",
        )
        .await?;
        publish_retained(
            &requests_tx,
            format!("{}/temperature/$datatype", node_base),
            "float",
        )
        .await?;
        publish_retained(
            &requests_tx,
            format!("{}/temperature/$unit", node_base),
            "ºC",
        )
        .await?;
        publish_retained(
            &requests_tx,
            format!("{}/humidity/$name", node_base),
            "Humidity",
        )
        .await?;
        publish_retained(
            &requests_tx,
            format!("{}/humidity/$datatype", node_base),
            "integer",
        )
        .await?;
        publish_retained(&requests_tx, format!("{}/humidity/$unit", node_base), "%").await?;
        publish_retained(
            &requests_tx,
            format!("{}/battery/$name", node_base),
            "Battery level",
        )
        .await?;
        publish_retained(
            &requests_tx,
            format!("{}/battery/$datatype", node_base),
            "integer",
        )
        .await?;
        publish_retained(&requests_tx, format!("{}/battery/$unit", node_base), "%").await?;
    }
    publish_retained(
        &requests_tx,
        format!("{}/$nodes", device_base),
        &nodes.join(","),
    )
    .await?;
    publish_retained(&requests_tx, format!("{}/$state", device_base), "ready").await?;

    start_notify_sensors(bt_session, &connected_sensors);

    // currently there is no way to set INCOMING_TIMEOUT_MS to -1 (which is how
    // you specify "infinite" according to
    // https://dbus.freedesktop.org/doc/api/html/group__DBusConnection.html#ga580d8766c23fe5f49418bc7d87b67dc6)
    // so we wrap the event loop in an infinite loop.
    loop {
        for event in bt_session
            .incoming(INCOMING_TIMEOUT_MS)
            .map(BluetoothEvent::from)
        {
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

            if let Some((temperature, humidity, battery_voltage, battery_percent)) =
                decode_value(&value)
            {
                let mac_address = device.get_address()?;
                let name = sensor_names.get(&mac_address).unwrap_or(&mac_address);
                println!(
                    "{} ({}) Temperature: {:.2}ºC Humidity: {:?}% Battery {} mV ({} %)",
                    device.get_id(),
                    name,
                    temperature,
                    humidity,
                    battery_voltage,
                    battery_percent
                );

                let node_id = mac_address.replace(":", "");
                let node_base = format!("{}/{}", device_base, node_id);
                publish_retained(
                    &requests_tx,
                    format!("{}/temperature", node_base),
                    &format!("{:.2}", temperature),
                )
                .await?;
                publish_retained(
                    &requests_tx,
                    format!("{}/humidity", node_base),
                    &humidity.to_string(),
                )
                .await?;
                publish_retained(
                    &requests_tx,
                    format!("{}/battery", node_base),
                    &battery_percent.to_string(),
                )
                .await?;
            } else {
                println!("Invalid value from {}", device.get_id());
            }
        }
    }
}
