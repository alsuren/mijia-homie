use bluez_generated::bluetooth_event::BluetoothEvent;
use dbus::nonblock::SyncConnection;
use futures::FutureExt;
use futures::StreamExt;
use mijia::{decode_value, SERVICE_CHARACTERISTIC_PATH};
use std::error::Error;
use std::sync::Arc;

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
