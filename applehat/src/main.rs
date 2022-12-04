mod config;

use config::{get_mqtt_options, Config};
use eyre::Report;
use homie_controller::{Event, HomieController, HomieEventLoop, PollError};
use log::{error, info, trace};
use rumqttc::ConnectionError;
use std::{sync::Arc, time::Duration};
use tokio::{
    task::{self, JoinHandle},
    time::sleep,
};

#[tokio::main]
async fn main() -> Result<(), Report> {
    stable_eyre::install()?;
    pretty_env_logger::init();
    color_backtrace::install();

    let config = Config::from_file()?;

    let reconnect_interval = config.mqtt.reconnect_interval;
    let mqtt_options = get_mqtt_options(config.mqtt);
    let (controller, event_loop) = HomieController::new(mqtt_options, &config.homie.prefix);
    let controller = Arc::new(controller);

    let handle = spawn_homie_poll_loop(event_loop, controller.clone(), reconnect_interval);

    handle.await?;

    Ok(())
}

fn spawn_homie_poll_loop(
    mut event_loop: HomieEventLoop,
    controller: Arc<HomieController>,
    reconnect_interval: Duration,
) -> JoinHandle<()> {
    task::spawn(async move {
        loop {
            match controller.poll(&mut event_loop).await {
                Ok(events) => {
                    for event in events {
                        handle_event(controller.as_ref(), event).await;
                    }
                }
                Err(e) => {
                    error!(
                        "Failed to poll HomieController for base topic '{}': {}",
                        controller.base_topic(),
                        e
                    );
                    if let PollError::Connection(ConnectionError::Io(_)) = e {
                        sleep(reconnect_interval).await;
                    }
                }
            }
        }
    })
}

async fn handle_event(controller: &HomieController, event: Event) {
    match event {
        Event::PropertyValueChanged {
            device_id,
            node_id,
            property_id,
            value,
            fresh,
        } => {
            trace!(
                "{}/{}/{}/{} = {} ({})",
                controller.base_topic(),
                device_id,
                node_id,
                property_id,
                value,
                fresh
            );
            if fresh {
                println!("Fresh property value {}/{}/{}={}", device_id, node_id, property_id, value);
            }
        }
        _ => {
            info!("{} Event: {:?}", controller.base_topic(), event);
        }
    }
}
