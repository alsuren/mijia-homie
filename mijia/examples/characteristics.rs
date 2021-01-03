use mijia::bluetooth::BluetoothSession;

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    pretty_env_logger::init();

    let (_, session) = BluetoothSession::new().await?;

    // Get the list of devices whose services are currently known and print them with their
    // characteristics.
    let devices = session.get_devices().await?;
    println!("Devices:");
    for device in devices {
        let services = session.get_services(&device.id).await?;
        if !services.is_empty() {
            println!("{}: {:?}", device.mac_address, device.id);
            println!("Services: {:#?}", services);
            for service in services {
                let characteristics = session.get_characteristics(&service.id).await?;
                println!(
                    "Service {:?} characteristics: {:#?}",
                    service, characteristics
                );
            }
        }
    }

    Ok(())
}
