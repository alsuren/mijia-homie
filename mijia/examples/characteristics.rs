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
            for service in services {
                println!(
                    "Service {} ({}): {:?}",
                    service.uuid,
                    if service.primary {
                        "primary"
                    } else {
                        "secondary"
                    },
                    service.id
                );
                let characteristics = session.get_characteristics(&service.id).await?;
                for characteristic in characteristics {
                    println!(
                        "  Characteristic {}: {:?}",
                        characteristic.uuid, characteristic.id
                    );
                    let descriptors = session.get_descriptors(&characteristic.id).await?;
                    for descriptor in descriptors {
                        println!("    Descriptor {} : {:?}", descriptor.uuid, descriptor.id);
                        if let Ok(value) = session.read_descriptor_value(&descriptor.id).await {
                            println!("      {:?}", value);
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
