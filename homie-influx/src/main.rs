use futures::future::try_join_all;
use futures::FutureExt;
use homie_controller::{Event, HomieController, HomieEventLoop, PollError};
use influx_db_client::reqwest::Url;
use influx_db_client::Client;
use rumqttc::MqttOptions;
use rustls::ClientConfig;
use stable_eyre::eyre;
use stable_eyre::eyre::WrapErr;
use std::sync::Arc;
use tokio::task::{self, JoinHandle};

const DEFAULT_MQTT_CLIENT_NAME: &str = "homie-influx";
const DEFAULT_MQTT_HOST: &str = "test.mosquitto.org";
const DEFAULT_MQTT_PORT: u16 = 1883;
const DEFAULT_INFLUXDB_URL: &str = "http://localhost:8086";

/// A mapping from a Homie prefix to monitor to an InfluxDB database where its data should be
/// stored.
struct Mapping {
    pub homie_prefix: String,
    pub influxdb_database: String,
}

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    stable_eyre::install()?;
    dotenv::dotenv().wrap_err("reading .env")?;
    pretty_env_logger::init();
    color_backtrace::install();

    let mappings = vec![Mapping {
        homie_prefix: "homie".to_owned(),
        influxdb_database: "test".to_owned(),
    }];

    let influxdb_url: Url = std::env::var("INFLUXDB_URL")
        .unwrap_or_else(|_| DEFAULT_INFLUXDB_URL.to_string())
        .parse()?;

    let mqtt_options = get_mqtt_options();

    let mut join_handles: Vec<_> = Vec::new();
    for mapping in &mappings {
        let (controller, event_loop) =
            HomieController::new(mqtt_options.clone(), &mapping.homie_prefix);
        let controller = Arc::new(controller);
        let influxdb_client = Client::new(influxdb_url.clone(), &mapping.influxdb_database);

        let handle = spawn_homie_poll_loop(event_loop, controller.clone());
        controller.start().await?;
        join_handles.push(handle.map(|res| Ok(res??)));
    }

    simplify_unit_vec(try_join_all(join_handles).await)
}

/// Construct the `MqttOptions` for connecting to the MQTT broker based on configuration options or
/// defaults.
fn get_mqtt_options() -> MqttOptions {
    let client_name =
        std::env::var("MQTT_CLIENT_NAME").unwrap_or_else(|_| DEFAULT_MQTT_CLIENT_NAME.to_string());

    let mqtt_host = std::env::var("MQTT_HOST").unwrap_or_else(|_| DEFAULT_MQTT_HOST.to_string());

    let mqtt_port = std::env::var("MQTT_PORT")
        .ok()
        .and_then(|val| val.parse::<u16>().ok())
        .unwrap_or(DEFAULT_MQTT_PORT);

    let mut mqtt_options = MqttOptions::new(client_name, mqtt_host, mqtt_port);
    mqtt_options.set_keep_alive(5);

    let mqtt_username = std::env::var("MQTT_USERNAME").ok();
    let mqtt_password = std::env::var("MQTT_PASSWORD").ok();
    if let (Some(username), Some(password)) = (mqtt_username, mqtt_password) {
        mqtt_options.set_credentials(username, password);
    }

    if std::env::var("MQTT_USE_TLS").is_ok() {
        let mut client_config = ClientConfig::new();
        client_config.root_store = rustls_native_certs::load_native_certs()
            .expect("Failed to load platform certificates.");
        mqtt_options.set_tls_client_config(Arc::new(client_config));
    }

    mqtt_options
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
