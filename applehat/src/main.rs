mod config;
mod ui;

use config::{get_mqtt_options, Config};
use eyre::Report;
use homie_controller::{Event, HomieController, HomieEventLoop, PollError};
use log::{error, info, trace};
use rainbow_hat_rs::alphanum4::Alphanum4;
use rumqttc::ConnectionError;
use std::{sync::Arc, time::Duration};
use tokio::{
    task::{self, JoinHandle},
    time::sleep,
};
use ui::UiState;

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

    let alphanum = Alphanum4::new()?;
    let ui_state = UiState {
        alphanum,
        selected_device_id: "mijia-bridge-cottagepi".to_string(),
        selected_node_id: "A4C138E98330".to_string(),
        selected_property_id: "temperature".to_string(),
    };

    let handle =
        spawn_homie_poll_loop(event_loop, controller.clone(), ui_state, reconnect_interval);

    handle.await?;

    Ok(())
}

fn spawn_homie_poll_loop(
    mut event_loop: HomieEventLoop,
    controller: Arc<HomieController>,
    mut ui_state: UiState,
    reconnect_interval: Duration,
) -> JoinHandle<()> {
    task::spawn(async move {
        loop {
            match controller.poll(&mut event_loop).await {
                Ok(events) => {
                    for event in events {
                        handle_event(controller.as_ref(), &mut ui_state, event).await;
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

async fn handle_event(controller: &HomieController, ui_state: &mut UiState, event: Event) {
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
                println!(
                    "Fresh property value {}/{}/{}={}",
                    device_id, node_id, property_id, value
                );
                ui_state.update_display(controller);
            }
        }
        _ => {
            info!("{} Event: {:?}", controller.base_topic(), event);
        }
    }
}
