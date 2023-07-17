use bluez_async::{BluetoothEvent, BluetoothSession, DeviceEvent};
use btsensor::atc::{SensorReading, UUID};
use futures::stream::StreamExt;

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    pretty_env_logger::init();

    let (_, session) = BluetoothSession::new().await?;
    let mut events = session.event_stream().await?;

    // Start scanning for Bluetooth devices.
    session.start_discovery().await?;

    // Wait for events.
    while let Some(event) = events.next().await {
        if let BluetoothEvent::Device {
            id,
            event: DeviceEvent::ServiceData { service_data },
        } = event
        {
            println!("{}: {:?}", id, service_data);
            if let Some(data) = service_data.get(&UUID) {
                if let Some(reading) = SensorReading::decode(data) {
                    println!("  {:?}", reading);
                } else {
                    println!("  (Failed to decode.)");
                }
            }
        }
    }

    Ok(())
}
