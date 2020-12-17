mod config;
mod influx;

use crate::config::{get_influxdb_client, get_mqtt_options, read_mappings, Config};
use crate::influx::send_property_value;
use futures::future::try_join_all;
use homie_controller::{Event, HomieController, HomieEventLoop, PollError};
use influx_db_client::Client;
use rumqttc::ConnectionError;
use stable_eyre::eyre;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::{self, JoinHandle};
use tokio::time::delay_for;

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    stable_eyre::install()?;
    pretty_env_logger::init();
    color_backtrace::install();

    let config = Config::from_file()?;
    let mappings = read_mappings(&config.homie)?;

    // Start a task per mapping to poll the Homie MQTT connection and send values to InfluxDB.
    let mut join_handles: Vec<_> = Vec::new();
    for mapping in &mappings {
        // Include Homie base topic in client name, because client name must be unique.
        let mqtt_options = get_mqtt_options(&config.mqtt, &mapping.homie_prefix);
        let (controller, event_loop) = HomieController::new(mqtt_options, &mapping.homie_prefix);
        let controller = Arc::new(controller);

        let influxdb_client = get_influxdb_client(&config.influxdb, &mapping.influxdb_database)?;

        let handle = spawn_homie_poll_loop(
            event_loop,
            controller.clone(),
            influxdb_client,
            config.mqtt.reconnect_interval,
        );
        controller.start().await?;
        join_handles.push(handle);
    }

    try_join_all(join_handles).await?;
    Ok(())
}

fn spawn_homie_poll_loop(
    mut event_loop: HomieEventLoop,
    controller: Arc<HomieController>,
    influx_db_client: Client,
    reconnect_interval: Duration,
) -> JoinHandle<()> {
    task::spawn(async move {
        loop {
            match controller.poll(&mut event_loop).await {
                Ok(Some(event)) => {
                    handle_event(controller.as_ref(), &influx_db_client, event).await;
                }
                Ok(None) => {}
                Err(e) => {
                    log::error!(
                        "Failed to poll HomieController for base topic '{}': {}",
                        controller.base_topic(),
                        e
                    );
                    if let PollError::Connection(ConnectionError::Io(_)) = e {
                        delay_for(reconnect_interval).await;
                    }
                }
            }
        }
    })
}

async fn handle_event(controller: &HomieController, influx_db_client: &Client, event: Event) {
    match event {
        Event::PropertyValueChanged {
            device_id,
            node_id,
            property_id,
            value,
            fresh,
        } => {
            log::trace!(
                "{}/{}/{}/{} = {} ({})",
                controller.base_topic(),
                device_id,
                node_id,
                property_id,
                value,
                fresh
            );
            if fresh {
                if let Err(e) = send_property_value(
                    controller,
                    influx_db_client,
                    device_id,
                    node_id,
                    property_id,
                )
                .await
                {
                    log::error!("{:?}", e);
                }
            }
        }
        _ => {
            log::info!("{} Event: {:?}", controller.base_topic(), event);
        }
    }
}
