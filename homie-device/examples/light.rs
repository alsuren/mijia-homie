use homie_device::{Datatype, HomieDevice, Node, Property, SpawnError};
use rumqttc::MqttOptions;

#[tokio::main(core_threads = 2)]
async fn main() -> Result<(), SpawnError> {
    pretty_env_logger::init();

    let mqttoptions = MqttOptions::new("homie_example", "test.mosquitto.org", 1883);

    let mut builder = HomieDevice::builder("homie/example", "Homie light example", mqttoptions);
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
            Property::new("power", "On", Datatype::Boolean, true, None, None),
            Property::new("colour", "Colour", Datatype::Color, true, None, Some("rgb")),
        ],
    );
    homie.add_node(node).await?;

    homie.ready().await?;
    println!("Ready");

    // This will only resolve (with an error) if we lose connection to the MQTT broker.
    homie_handle.await
}
