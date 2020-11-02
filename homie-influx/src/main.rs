use futures::future::try_join_all;
use futures::FutureExt;
use homie_controller::{Event, HomieController, HomieEventLoop, PollError};
use rumqttc::MqttOptions;
use rustls::ClientConfig;
use stable_eyre::eyre;
use stable_eyre::eyre::WrapErr;
use std::sync::Arc;
use tokio::task::{self, JoinHandle};

const DEFAULT_CLIENT_NAME: &str = "homie-influx";
const DEFAULT_HOST: &str = "test.mosquitto.org";
const DEFAULT_PORT: u16 = 1883;

const HOMIE_PREFIXES: [&str; 1] = ["homie"];

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    stable_eyre::install()?;
    dotenv::dotenv().wrap_err("reading .env")?;
    pretty_env_logger::init();
    color_backtrace::install();

    let client_name =
        std::env::var("CLIENT_NAME").unwrap_or_else(|_| DEFAULT_CLIENT_NAME.to_string());

    let host = std::env::var("HOST").unwrap_or_else(|_| DEFAULT_HOST.to_string());

    let port = std::env::var("PORT")
        .ok()
        .and_then(|val| val.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);

    let mut mqttoptions = MqttOptions::new(client_name, host, port);

    let username = std::env::var("USERNAME").ok();
    let password = std::env::var("PASSWORD").ok();

    mqttoptions.set_keep_alive(5);
    if let (Some(u), Some(p)) = (username, password) {
        mqttoptions.set_credentials(u, p);
    }

    // Use `env -u USE_TLS` to unset this variable if you need to clear it.
    if std::env::var("USE_TLS").is_ok() {
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
                    } => {
                        println!("{}/{}/{} = {}", device_id, node_id, property_id, value);
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
