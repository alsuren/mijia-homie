use bluez_generated::bluetooth_event::BluetoothEvent;
use bluez_generated::generated::adapter1::OrgBluezAdapter1;
use dbus::arg::RefArg;
use dbus::nonblock::stdintf::org_freedesktop_dbus::ObjectManager;
use dbus::nonblock::SyncConnection;
use futures::FutureExt;
use futures::StreamExt;
use mijia::{decode_value, MIJIA_SERVICE_DATA_UUID, SERVICE_CHARACTERISTIC_PATH};
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

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
        Duration::from_secs(10),
        conn.clone(),
    );

    adapter.set_powered(true).await?;
    adapter.start_discovery().await?;

    let bluez_root =
        dbus::nonblock::Proxy::new("org.bluez", "/", Duration::from_secs(10), conn.clone());
    let devices = bluez_root.get_managed_objects().await?;

    println!("{:?}", devices);
    devices
        .iter()
        .map(|(path, interfaces)| {
            // FIXME: can we generate a strongly typed deserialiser for this,
            // based on the introspection data?
            let properties = interfaces.get("org.bluez.Device1")?;
            // FIXME: UUIDs don't get populated until we connect. Use:
            //     "ServiceData": Variant(InternalDict { data: [
            //         ("0000fe95-0000-1000-8000-00805f9b34fb", Variant([48, 88, 91, 5, 1, 23, 33, 215, 56, 193, 164, 40, 1, 0])
            //     )], outer_sig: Signature("a{sv}") })
            // instead?
            let uuids = dbg!(properties.get("UUIDs"))?;

            uuids
                .as_iter()?
                .filter_map(|ids| {
                    dbg!(ids)
                        .as_iter()?
                        .find(|id| dbg!(id).as_str() == Some(MIJIA_SERVICE_DATA_UUID))
                })
                .next()
                .and_then(|_| Some(path))
        })
        .filter(Option::is_some)
        .for_each(|id| println!("path: {:?}", id));

    let bluetooth_handle = service_bluetooth_event_queue(conn);

    let res: Result<_, Box<dyn Error + Send + Sync>> = tokio::try_join! {
        dbus_handle.map(|res| Ok(res??)),
        // Bluetooth finished first. Convert error and get on with your life.
        bluetooth_handle.map(|res| Ok(res?)),
    };
    res?;
    Ok(())
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
