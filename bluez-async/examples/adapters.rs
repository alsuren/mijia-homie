//! Example to show information about the Bluetooth adapters on the system.

use bluez_async::BluetoothSession;

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    pretty_env_logger::init();

    let (_, session) = BluetoothSession::new().await?;

    // Get the list of all Bluetooh adapters on the system.
    let adapters = session.get_adapters().await?;
    println!("Adapters: {:#?}", adapters);

    Ok(())
}
