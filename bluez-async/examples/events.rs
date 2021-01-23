//! Example to log Bluetooth events, including duplicate manufacturer-specific advertisement data.

use bluez_async::{BluetoothSession, DiscoveryFilter};
use futures::stream::StreamExt;

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    pretty_env_logger::init();

    let (_, session) = BluetoothSession::new().await?;
    let mut events = session.event_stream().await?;
    session
        .start_discovery_with_filter(&DiscoveryFilter {
            duplicate_data: Some(true),
            ..DiscoveryFilter::default()
        })
        .await?;

    println!("Events:");
    while let Some(event) = events.next().await {
        println!("{:?}", event);
    }

    Ok(())
}
