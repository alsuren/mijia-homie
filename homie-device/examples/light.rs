use homie_device::{ColorFormat, HomieDevice, Node, Property, SpawnError};
use rumqttc::MqttOptions;

#[tokio::main]
async fn main() -> Result<(), SpawnError> {
    pretty_env_logger::init();

    let mqttoptions = MqttOptions::new("homie_example", "test.mosquitto.org", 1883);

    let mut builder =
        HomieDevice::builder("homie/example_light", "Homie light example", mqttoptions);
    builder.set_update_callback(|node_id, property_id, value| async move {
        println!("{}/{} is now {}", node_id, property_id, value);
        Some(value)
    });
    let (mut homie, homie_handle) = builder.spawn().await?;

    let node = Node::new(
        "light",
        "Light",
        "light",
        vec![
            Property::boolean("power", "On", true, None),
            Property::color("colour", "Colour", true, None, ColorFormat::RGB),
        ],
    );
    homie.add_node(node).await?;

    homie.ready().await?;
    println!("Ready");

    // This will only resolve (with an error) if we lose connection to the MQTT broker.
    homie_handle.await
}
