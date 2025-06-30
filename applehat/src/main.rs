mod config;
mod ui;

use config::{Config, get_mqtt_options};
use eyre::Report;
use futures::future::try_join;
use homie_controller::{Event, HomieController, HomieEventLoop, PollError};
use log::{error, info, trace};
use rainbow_hat_rs::{alphanum4::Alphanum4, apa102::APA102, touch::Buttons};
use rumqttc::ConnectionError;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    task::{self, JoinHandle},
    time::sleep,
};
use ui::{UiState, spawn_button_poll_loop};

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
    let mut pixels = APA102::new()?;
    pixels.setup()?;
    let buttons = Buttons::new()?;
    let ui_state = Arc::new(Mutex::new(UiState::new(
        controller.clone(),
        alphanum,
        pixels,
    )));

    // Display initial state.
    ui_state.lock().unwrap().update_display();

    let handle = spawn_homie_poll_loop(
        event_loop,
        controller.clone(),
        ui_state.clone(),
        reconnect_interval,
    );
    let button_handle = spawn_button_poll_loop(buttons, ui_state);

    try_join(handle, button_handle).await?;

    Ok(())
}

fn spawn_homie_poll_loop(
    mut event_loop: HomieEventLoop,
    controller: Arc<HomieController>,
    ui_state: Arc<Mutex<UiState>>,
    reconnect_interval: Duration,
) -> JoinHandle<()> {
    task::spawn(async move {
        loop {
            match controller.poll(&mut event_loop).await {
                Ok(events) => {
                    for event in events {
                        handle_event(controller.as_ref(), &ui_state, event);
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

fn handle_event(controller: &HomieController, ui_state: &Mutex<UiState>, event: Event) {
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
                println!("Fresh property value {device_id}/{node_id}/{property_id}={value}");
                ui_state.lock().unwrap().update_display();
            }
        }
        _ => {
            info!("{} Event: {:?}", controller.base_topic(), event);
        }
    }
}
