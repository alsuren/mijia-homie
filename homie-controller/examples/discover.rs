//! Example to discover all Homie devices, and log whenever a property value changes.

use homie_controller::{Event, HomieController, PollError};
use rumqttc::MqttOptions;

#[tokio::main]
async fn main() -> Result<(), PollError> {
    pretty_env_logger::init();

    let mqttoptions = MqttOptions::new("homie_controller", "test.mosquitto.org", 1883);

    let (controller, mut event_loop) = HomieController::new(mqttoptions, "homie");
    controller.start().await?;
    loop {
        if let Some(event) = controller.poll(&mut event_loop).await? {
            match event {
                Event::PropertyValueChanged {
                    device_id,
                    node_id,
                    property_id,
                    value,
                } => {
                    println!("{}/{}/{} = {}", device_id, node_id, property_id, value);
                }
                _ => {
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
            }
        }
    }
}
