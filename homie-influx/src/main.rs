mod config;
mod influx;

use crate::config::{get_influxdb_client, get_mqtt_options, read_mappings};
use crate::influx::send_property_value;
use futures::future::try_join_all;
use futures::FutureExt;
use homie_controller::{Event, HomieController, HomieEventLoop};
use influx_db_client::Client;
use stable_eyre::eyre;
use stable_eyre::eyre::WrapErr;
use std::sync::Arc;
use tokio::task::{self, JoinHandle};

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    stable_eyre::install()?;
    dotenv::dotenv().wrap_err("reading .env")?;
    pretty_env_logger::init();
    color_backtrace::install();

    let mappings = read_mappings()?;

    // Start a task per mapping to poll the Homie MQTT connection and send values to InfluxDB.
    let mut join_handles: Vec<_> = Vec::new();
    for mapping in &mappings {
        // Include Homie base topic in client name, because client name must be unique.
        let mqtt_options = get_mqtt_options(&mapping.homie_prefix);
        let (controller, event_loop) = HomieController::new(mqtt_options, &mapping.homie_prefix);
        let controller = Arc::new(controller);

        let influxdb_client = get_influxdb_client(&mapping.influxdb_database)?;

        let handle = spawn_homie_poll_loop(event_loop, controller.clone(), influxdb_client);
        controller.start().await?;
        join_handles.push(handle.map(|res| Ok(res??)));
    }

    simplify_unit_vec(try_join_all(join_handles).await)
}

fn spawn_homie_poll_loop(
    mut event_loop: HomieEventLoop,
    controller: Arc<HomieController>,
    influx_db_client: Client,
) -> JoinHandle<Result<(), eyre::Report>> {
    task::spawn(async move {
        loop {
            if let Some(event) = controller.poll(&mut event_loop).await.wrap_err_with(|| {
                format!(
                    "Failed to poll HomieController for base topic '{}'.",
                    controller.base_topic()
                )
            })? {
                match event {
                    Event::PropertyValueChanged {
                        device_id,
                        node_id,
                        property_id,
                        value,
                        fresh,
                    } => {
                        log::trace!(
                            "{}/{}/{} = {} ({})",
                            device_id,
                            node_id,
                            property_id,
                            value,
                            fresh
                        );
                        if fresh {
                            send_property_value(
                                controller.as_ref(),
                                &influx_db_client,
                                device_id,
                                node_id,
                                property_id,
                            )
                            .await?;
                        }
                    }
                    _ => {
                        log::info!("Event: {:?}", event);
                    }
                }
            }
        }
    })
}

fn simplify_unit_vec<E>(m: Result<Vec<()>, E>) -> Result<(), E> {
    m.map(|_| ())
}
