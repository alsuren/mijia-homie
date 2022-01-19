//! Example to discover all Homie devices, and log whenever a property value changes.

use homie_controller::{Event, HomieController, PollError};
use rumqttc::MqttOptions;

#[tokio::main]
async fn main() -> Result<(), PollError> {
    pretty_env_logger::init();

    let mut mqttoptions = MqttOptions::new("homie_controller", "test.mosquitto.org", 1883);
    mqttoptions.set_keep_alive(5);

    let (controller, mut event_loop) = HomieController::new(mqttoptions, "homie");
    loop {
        match controller.poll(&mut event_loop).await {
            Ok(Some(Event::PropertyValueChanged {
                device_id,
                node_id,
                property_id,
                value,
                fresh,
            })) => {
                println!(
                    "{}/{}/{} = {} ({})",
                    device_id, node_id, property_id, value, fresh
                );
            }
            Ok(Some(event)) => {
                println!("Event: {:?}", event);
                println!("Devices:");
                for device in controller.devices().values() {
                    if device.has_required_attributes() {
                        println!(" * {:?}", device);
                    } else {
                        println!(" * {} not ready.", device.id);
                    }
                }
            }
            Ok(None) => {}
            Err(e) => log::error!("Error: {:?}", e),
        }
    }
}
