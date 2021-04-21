use homie_device::{ColorFormat, ColorRGB, HomieDevice, Node, Property, SpawnError};
use rumqttc::MqttOptions;

#[tokio::main]
async fn main() -> Result<(), SpawnError> {
    pretty_env_logger::init();

    let mqttoptions = MqttOptions::new("homie_example", "test.mosquitto.org", 1883);

    let mut builder =
        HomieDevice::builder("homie/example_light", "Homie light example", mqttoptions);
    builder.set_update_callback(update_callback);
    let (mut homie, homie_handle) = builder.spawn().await?;

    let node = Node::new(
        "light",
        "Light",
        "light",
        vec![
            Property::boolean("power", "On", true, true, None),
            Property::color("colour", "Colour", true, true, None, ColorFormat::RGB),
        ],
    );
    homie.add_node(node).await?;

    homie.ready().await?;
    println!("Ready");

    // This will only resolve (with an error) if we lose connection to the MQTT broker.
    homie_handle.await
}

async fn update_callback(node_id: String, property_id: String, value: String) -> Option<String> {
    match (node_id.as_ref(), property_id.as_ref()) {
        ("light", "power") => {
            set_power(value.parse().unwrap());
        }
        ("light", "colour") => {
            set_colour(value.parse().unwrap());
        }
        _ => {
            println!(
                "Unexpected property {}/{} is now {}",
                node_id, property_id, value
            );
        }
    }
    Some(value)
}

fn set_power(power: bool) {
    println!("Power {}", power)
}

fn set_colour(colour: ColorRGB) {
    println!("Colour {}", colour);
}
