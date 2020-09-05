use bluez_generated::bluetooth_event::BluetoothEvent;
use bluez_generated::generated::adapter1::OrgBluezAdapter1;
use bluez_generated::generated::device1::OrgBluezDevice1;
use bluez_generated::generated::gattcharacteristic1::OrgBluezGattCharacteristic1;
use dbus::arg::RefArg;
use dbus::nonblock::stdintf::org_freedesktop_dbus::ObjectManager;
use dbus::nonblock::SyncConnection;
use dbus::Path;
use futures::FutureExt;
use futures::StreamExt;
use mijia::{
    decode_value, CONNECTION_INTERVAL_500_MS, CONNECTION_INTERVAL_CHARACTERISTIC_PATH,
    MIJIA_SERVICE_DATA_UUID, SERVICE_CHARACTERISTIC_PATH,
};
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use std::time::Instant;

#[tokio::main(core_threads = 3)]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    // Connect to the D-Bus session bus (this is blocking, unfortunately).
    let (dbus_resource, conn) = dbus_tokio::connection::new_system_sync()?;
    // The resource is a task that should be spawned onto a tokio compatible
    // reactor ASAP. If the resource ever finishes, you lost connection to D-Bus.
    let dbus_handle = tokio::spawn(async {
        let err = dbus_resource.await;
        Err::<(), Box<dyn Error + Send + Sync>>(err)
    });

    let adapter = dbus::nonblock::Proxy::new(
        "org.bluez",
        "/org/bluez/hci0",
        Duration::from_secs(30),
        conn.clone(),
    );

    adapter.set_powered(true).await?;
    adapter.start_discovery().await?;

    let mut sensors = get_sensors(conn.clone()).await?;
    println!("{:?}", sensors);

    connect_start_sensor(conn.clone(), &mut sensors[0]).await?;

    let bluetooth_handle = service_bluetooth_event_queue(conn);

    let res: Result<_, Box<dyn Error + Send + Sync>> = tokio::try_join! {
        dbus_handle.map(|res| Ok(res??)),
        // Bluetooth finished first. Convert error and get on with your life.
        bluetooth_handle.map(|res| Ok(res?)),
    };
    res?;
    Ok(())
}
async fn connect_start_sensor<'a>(
    conn: Arc<SyncConnection>,
    sensor: &mut Sensor,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let device = dbus::nonblock::Proxy::new(
        "org.bluez",
        sensor.device_path.to_owned(),
        Duration::from_secs(30),
        conn.clone(),
    );
    println!("Connecting");
    device.connect().await?;
    match start_notify_sensor(conn, sensor).await {
        Ok(()) => {
            sensor.last_update_timestamp = Instant::now();
            Ok(())
        }
        Err(e) => {
            // If starting notifications failed, disconnect so that we start again from a clean
            // state next time.
            device.disconnect().await?;
            Err(e)
        }
    }
}

async fn start_notify_sensor<'a>(
    conn: Arc<SyncConnection>,
    sensor: &Sensor,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let temp_humidity_path: String = sensor.device_path.to_string() + SERVICE_CHARACTERISTIC_PATH;
    let temp_humidity = dbus::nonblock::Proxy::new(
        "org.bluez",
        temp_humidity_path,
        Duration::from_secs(30),
        conn.clone(),
    );
    temp_humidity.start_notify().await?;

    let connection_interval_path: String =
        sensor.device_path.to_string() + CONNECTION_INTERVAL_CHARACTERISTIC_PATH;
    let connection_interval = dbus::nonblock::Proxy::new(
        "org.bluez",
        connection_interval_path,
        Duration::from_secs(30),
        conn.clone(),
    );
    connection_interval
        .write_value(CONNECTION_INTERVAL_500_MS.to_vec(), Default::default())
        .await?;
    Ok(())
}

#[derive(Debug)]
struct Sensor {
    device_path: Path<'static>,
    mac_address: String,
    name: String,
    last_update_timestamp: Instant,
}

async fn get_sensors(
    conn: Arc<SyncConnection>,
) -> Result<Vec<Sensor>, Box<dyn Error + Send + Sync>> {
    let bluez_root =
        dbus::nonblock::Proxy::new("org.bluez", "/", Duration::from_secs(30), conn.clone());
    let tree = bluez_root.get_managed_objects().await?;

    let paths = tree
        .into_iter()
        .filter_map(|(path, interfaces)| {
            // FIXME: can we generate a strongly typed deserialiser for this,
            // based on the introspection data?
            let device_properties = interfaces.get("org.bluez.Device1")?;

            let mac_address = device_properties
                .get("Address")?
                .as_iter()?
                .filter_map(|addr| addr.as_str())
                .next()?
                .to_string();
            // FIXME: UUIDs don't get populated until we connect. Use:
            //     "ServiceData": Variant(InternalDict { data: [
            //         ("0000fe95-0000-1000-8000-00805f9b34fb", Variant([48, 88, 91, 5, 1, 23, 33, 215, 56, 193, 164, 40, 1, 0])
            //     )], outer_sig: Signature("a{sv}") })
            // instead?
            let uuids = device_properties.get("UUIDs")?;

            if uuids
                // Mad hack to turn the Variant into an Array (like how option.as_iter() works?)
                .as_iter()?
                .filter_map(|ids| {
                    // we now have an Array. I promise.
                    ids.as_iter()?
                        .find(|id| id.as_str() == Some(MIJIA_SERVICE_DATA_UUID))
                })
                .next()
                .is_some()
            {
                Some(Sensor {
                    device_path: path.to_owned(),
                    mac_address,
                    // FIXME: use the sensor_names HashMap?
                    name: "".to_string(),
                    last_update_timestamp: Instant::now(),
                })
            } else {
                None
            }
        })
        .collect();
    Ok(paths)
}

async fn service_bluetooth_event_queue(
    conn: Arc<SyncConnection>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    println!("Subscribing to events");
    let mut rule = dbus::message::MatchRule::new();
    rule.msg_type = Some(dbus::message::MessageType::Signal);
    rule.sender = Some(dbus::strings::BusName::new("org.bluez")?);

    let (msg_match, mut events) = conn.add_match(rule).await?.msg_stream();
    println!("Processing events");
    // Process events until there are none available for the timeout.
    while let Some(raw_event) = events.next().await {
        if let Some(event) = BluetoothEvent::from(raw_event) {
            handle_bluetooth_event(event).await?
        }
    }
    // TODO: move this into a defer or scope guard or something.
    conn.remove_match(msg_match.token()).await?;
    Ok(())
}

async fn handle_bluetooth_event(event: BluetoothEvent) -> Result<(), Box<dyn Error + Send + Sync>> {
    match event {
        BluetoothEvent::Value { object_path, value } => {
            // TODO: Make this less hacky.
            let device_path = match object_path.strip_suffix(SERVICE_CHARACTERISTIC_PATH) {
                Some(path) => path,
                None => return Ok(()),
            };
            if let Some(readings) = decode_value(&value) {
                println!("{}: {}", device_path, readings);
            } else {
                println!("Invalid value from {}", device_path);
            }
        }
        _ => {}
    };
    Ok(())
}
