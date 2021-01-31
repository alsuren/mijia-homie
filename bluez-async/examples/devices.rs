//! Example to scan for a short time and then list all known devices.

use bluez_async::BluetoothSession;
use std::time::Duration;
use tokio::time;

const SCAN_DURATION: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    pretty_env_logger::init();

    let (_, session) = BluetoothSession::new().await?;

    // Start scanning for Bluetooth devices, and wait a while for some to be discovered.
    session.start_discovery().await?;
    time::sleep(SCAN_DURATION).await;
    session.stop_discovery().await?;

    // Get the list of all devices which BlueZ knows about.
    let devices = session.get_devices().await?;
    println!("Devices: {:#?}", devices);

    Ok(())
}
