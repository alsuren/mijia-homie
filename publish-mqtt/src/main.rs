use async_channel::{SendError, Sender};
use blurz::{
    BluetoothAdapter, BluetoothDiscoverySession, BluetoothGATTCharacteristic, BluetoothSession,
};
use futures::FutureExt;
use mijia::{connect_sensors, decode_value, find_sensors, print_sensors};
use rumqttc::{self, EventLoop, LastWill, MqttOptions, Publish, QoS, Request};
use std::error::Error;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::{task, time, try_join};

const MQTT_PREFIX: &str = "homie";
const DEVICE_NAME: &str = "mijia-bridge";
const SCAN_DURATION: Duration = Duration::from_secs(5);
const UPDATE_PERIOD: Duration = Duration::from_secs(20);

async fn scan<'a>(bt_session: &'a BluetoothSession) -> Result<Vec<String>, Box<dyn Error>> {
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
    pretty_env_logger::init();
    color_backtrace::install();

    let mut mqttoptions = MqttOptions::new("rumqtt-async", "test.mosquitto.org", 1883);
    mqttoptions.set_keep_alive(5);
    let device_base = format!("{}/{}", MQTT_PREFIX, DEVICE_NAME);
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
        requests(requests_tx).await.unwrap();
    });

    let mqtt_handle: JoinHandle<Result<(), Box<dyn Error + Send + Sync>>> =
        task::spawn(async move {
            loop {
                let (incoming, outgoing) = eventloop.poll().await?;
                println!("Incoming = {:?}, Outgoing = {:?}", incoming, outgoing);
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

async fn requests(requests_tx: Sender<Request>) -> Result<(), Box<dyn Error>> {
    let device_base = format!("{}/{}", MQTT_PREFIX, DEVICE_NAME);
    publish_retained(&requests_tx, format!("{}/$homie", device_base), "4.0").await?;
    publish_retained(&requests_tx, format!("{}/$extensions", device_base), "").await?;
    publish_retained(
        &requests_tx,
        format!("{}/$name", device_base),
        "Mijia bridge",
    )
    .await?;
    publish_retained(&requests_tx, format!("{}/$state", device_base), "init").await?;

    let bt_session = &BluetoothSession::create_session(None)?;
    let device_list = scan(&bt_session).await?;
    let sensors = find_sensors(&bt_session, &device_list);
    print_sensors(&sensors);
    let connected_sensors = connect_sensors(&sensors);

    let mut nodes = vec![];
    for sensor in &connected_sensors {
        let mac_address = sensor.get_address()?;
        let node_id = mac_address.replace(":", "-");
        let node_base = format!("{}/{}", device_base, node_id);
        nodes.push(node_id);
        publish_retained(&requests_tx, format!("{}/$name", node_base), &mac_address).await?;
        publish_retained(&requests_tx, format!("{}/$type", node_base), "Mijia sensor").await?;
        publish_retained(
            &requests_tx,
            format!("{}/$properties", node_base),
            "temperature,humidity",
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
            "float",
        )
        .await?;
        publish_retained(
            &requests_tx,
            format!("{}/humidity/$unit", node_base),
            "%",
        )
        .await?;
    }
    publish_retained(
        &requests_tx,
        format!("{}/$nodes", device_base),
        &nodes.join(","),
    )
    .await?;
    publish_retained(&requests_tx, format!("{}/$state", device_base), "ready").await?;

    loop {
        println!();
        time::delay_for(UPDATE_PERIOD).await;
        for device in &connected_sensors {
            let temp_humidity = BluetoothGATTCharacteristic::new(
                bt_session,
                device.get_id() + "/service0021/char0035",
            );
            match temp_humidity.get_value() {
                Err(e) => println!("Failed to get value from {}: {:?}", device.get_id(), e),
                Ok(value) => {
                    if let Some((temperature, humidity)) = decode_value(&value) {
                        println!(
                            "{} Temperature: {:.2}ºC Humidity: {:?}%",
                            device.get_id(),
                            temperature,
                            humidity
                        );

                        let mac_address = device.get_address()?;
                        let node_id = mac_address.replace(":", "-");
                        let node_base = format!("{}/{}", device_base, node_id);
                        publish_retained(
                            &requests_tx,
                            format!("{}/temperature", node_base),
                            &temperature.to_string(),
                        )
                        .await?;
                        publish_retained(
                            &requests_tx,
                            format!("{}/humidity", node_base),
                            &humidity.to_string(),
                        )
                        .await?;
                    } else {
                        println!("Invalid value from {}", device.get_id());
                    }
                }
            }
        }
    }
}
