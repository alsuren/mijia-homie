use homie::{Datatype, HomieDevice, Node, Property};
use rumqttc::MqttOptions;
use std::error::Error;
use tokio::try_join;

#[tokio::main(core_threads = 2)]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    pretty_env_logger::init();

    let mqttoptions = MqttOptions::new("homie_example", "test.mosquitto.org", 1883);

    let mut builder = HomieDevice::builder("homie/example", "Homie light example", mqttoptions);
    builder.set_update_callback(|node_id, property_id, value| async move {
        println!("{}/{} is now {}", node_id, property_id, value);
        true
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

    // Poll everything to completion, until the first one returns an error.
    let res: Result<_, Box<dyn Error + Send + Sync>> = try_join! {
        homie_handle,
    };
    res?;
    Ok(())
}
