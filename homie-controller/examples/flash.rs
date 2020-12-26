//! Example to turn all Homie devices on and off every 5 seconds, 4 times.

use futures::FutureExt;
use homie_controller::{Datatype, Event, HomieController, HomieEventLoop, PollError};
use rumqttc::MqttOptions;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::{self, JoinHandle};
use tokio::{time, try_join};

fn spawn_poll_loop(
    mut event_loop: HomieEventLoop,
    controller: Arc<HomieController>,
) -> JoinHandle<Result<(), PollError>> {
    task::spawn(async move {
        loop {
            if let Some(event) = controller.poll(&mut event_loop).await? {
                match event {
                    Event::PropertyValueChanged {
                        device_id,
                        node_id,
                        property_id,
                        value,
                        fresh,
                    } => {
                        println!(
                            "{}/{}/{} = {} ({})",
                            device_id, node_id, property_id, value, fresh
                        );
                    }
                    _ => {
                        log::info!("Event: {:?}", event);
                    }
                }
            }
        }
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    pretty_env_logger::init();

    let mqttoptions = MqttOptions::new("homie_controller", "test.mosquitto.org", 1883);

    let (controller, event_loop) = HomieController::new(mqttoptions, "homie");
    let controller = Arc::new(controller);
    let handle = spawn_poll_loop(event_loop, controller.clone());
    controller.start().await?;

    for _ in 0..3 {
        for &value in [true, false].iter() {
            time::sleep(Duration::from_secs(5)).await;
            println!("Turning everything {}", if value { "on" } else { "off" });
            for device in controller.devices().values() {
                for node in device.nodes.values() {
                    for property in node.properties.values() {
                        if property.settable && property.datatype == Some(Datatype::Boolean) {
                            println!("{}/{}/{} set to {}", device.id, node.id, property.id, value);
                            controller
                                .set(&device.id, &node.id, &property.id, value)
                                .await?;
                        }
                    }
                }
            }
        }
    }

    controller.disconnect().await?;

    try_join!(handle.map(|res| Ok::<_, Box<dyn Error>>(res??)))?;

    Ok(())
}
