use mijia::bluetooth::{BleUuid, BluetoothSession};
use std::str;

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
            println!("{}: {}", device.mac_address, device.id);
            for service in services {
                println!(
                    "Service {} ({}): {}",
                    service.uuid.succinctly(),
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
                        "  Characteristic {}: {}",
                        characteristic.uuid.succinctly(),
                        characteristic.id
                    );
                    let descriptors = session.get_descriptors(&characteristic.id).await?;
                    for descriptor in descriptors {
                        println!(
                            "    Descriptor {}: {}",
                            descriptor.uuid.succinctly(),
                            descriptor.id
                        );
                        let value = session.read_descriptor_value(&descriptor.id).await?;
                        println!("      {}", debug_format_maybe_string(&value));
                    }
                }
            }
        }
    }

    Ok(())
}

/// Guesses whether the given descriptor value might be a string, and if returns it formatted either
/// as a string or as a list of numbers.
fn debug_format_maybe_string(value: &[u8]) -> String {
    if value.len() > 1 && value[value.len() - 1] == 0 {
        match str::from_utf8(&value[0..value.len() - 1]) {
            Ok(string) if value.len() > 1 && !string.chars().any(|c| c.is_control()) => {
                format!("{:?}", string)
            }
            _ => {
                format!("{:?}", value)
            }
        }
    } else {
        format!("{:?}", value)
    }
}
