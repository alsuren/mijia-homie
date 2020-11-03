use futures::future::try_join_all;
use futures::FutureExt;
use homie_controller::{Event, HomieController, HomieEventLoop, PollError};
use rumqttc::MqttOptions;
use rustls::ClientConfig;
use stable_eyre::eyre;
use stable_eyre::eyre::WrapErr;
use std::sync::Arc;
use tokio::task::{self, JoinHandle};

const DEFAULT_MQTT_CLIENT_NAME: &str = "homie-influx";
const DEFAULT_MQTT_HOST: &str = "test.mosquitto.org";
const DEFAULT_MQTT_PORT: u16 = 1883;

const HOMIE_PREFIXES: [&str; 1] = ["homie"];

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    stable_eyre::install()?;
    dotenv::dotenv().wrap_err("reading .env")?;
    pretty_env_logger::init();
    color_backtrace::install();

    let client_name =
        std::env::var("MQTT_CLIENT_NAME").unwrap_or_else(|_| DEFAULT_MQTT_CLIENT_NAME.to_string());

    let mqtt_host = std::env::var("MQTT_HOST").unwrap_or_else(|_| DEFAULT_MQTT_HOST.to_string());

    let mqtt_port = std::env::var("MQTT_PORT")
        .ok()
        .and_then(|val| val.parse::<u16>().ok())
        .unwrap_or(DEFAULT_MQTT_PORT);

    let mut mqttoptions = MqttOptions::new(client_name, mqtt_host, mqtt_port);

    let mqtt_username = std::env::var("MQTT_USERNAME").ok();
    let mqtt_password = std::env::var("MQTT_PASSWORD").ok();

    mqttoptions.set_keep_alive(5);
    if let (Some(u), Some(p)) = (mqtt_username, mqtt_password) {
        mqttoptions.set_credentials(u, p);
    }

    if std::env::var("MQTT_USE_TLS").is_ok() {
        let mut client_config = ClientConfig::new();
        client_config.root_store = rustls_native_certs::load_native_certs()
            .expect("Failed to load platform certificates.");
        mqttoptions.set_tls_client_config(Arc::new(client_config));
    }

    let mut join_handles: Vec<_> = Vec::new();
    for homie_prefix in &HOMIE_PREFIXES {
        let (controller, event_loop) = HomieController::new(mqttoptions.clone(), homie_prefix);
        let controller = Arc::new(controller);
        let handle = spawn_homie_poll_loop(event_loop, controller.clone());
        controller.start().await?;
        join_handles.push(handle.map(|res| Ok(res??)));
    }

    simplify_unit_vec(try_join_all(join_handles).await)
}

fn spawn_homie_poll_loop(
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

fn simplify_unit_vec<E>(m: Result<Vec<()>, E>) -> Result<(), E> {
    m.map(|_| ())
}
