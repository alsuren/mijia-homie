use influx_db_client::reqwest::Url;
use influx_db_client::Client;
use rumqttc::MqttOptions;
use rustls::ClientConfig;
use stable_eyre::eyre;
use stable_eyre::eyre::WrapErr;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::Arc;

const DEFAULT_MQTT_CLIENT_PREFIX: &str = "homie-influx";
const DEFAULT_MQTT_HOST: &str = "test.mosquitto.org";
const DEFAULT_MQTT_PORT: u16 = 1883;
const DEFAULT_INFLUXDB_URL: &str = "http://localhost:8086";
const DEFAULT_MAPPINGS_FILENAME: &str = "mappings.conf";

/// A mapping from a Homie prefix to monitor to an InfluxDB database where its data should be
/// stored.
pub struct Mapping {
    pub homie_prefix: String,
    pub influxdb_database: String,
}

/// Read mappings from the configured file, and make sure there is at least one.
pub fn read_mappings() -> Result<Vec<Mapping>, eyre::Report> {
    let mappings_filename = std::env::var("MAPPINGS_FILENAME")
        .unwrap_or_else(|_| DEFAULT_MAPPINGS_FILENAME.to_string());
    let mappings = mappings_from_file(&mappings_filename)?;
    if mappings.len() == 0 {
        eyre::bail!(
            "At least one mapping must be configured in {}.",
            mappings_filename
        );
    }
    Ok(mappings)
}

/// Read mappings of the form "homie_prefix:influxdb_database" from the given file, ignoring any
/// lines starting with '#'.
fn mappings_from_file(filename: &str) -> Result<Vec<Mapping>, eyre::Report> {
    let mut mappings = Vec::new();
    let file = File::open(filename).wrap_err_with(|| format!("Failed to open {}", filename))?;
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.len() > 0 && !line.starts_with('#') {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() != 2 {
                eyre::bail!("Invalid line '{}'", line);
            }
            let homie_prefix = parts[0].to_owned();
            let influxdb_database = parts[1].to_owned();
            mappings.push(Mapping {
                homie_prefix,
                influxdb_database,
            });
        }
    }

    Ok(mappings)
}

/// Construct a new InfluxDB `Client` based on configuration options or defaults.
///
/// The database name is not set; make sure to set it before using the client.
pub fn get_influxdb_client(database: &str) -> Result<Client, eyre::Report> {
    let influxdb_url: Url = std::env::var("INFLUXDB_URL")
        .unwrap_or_else(|_| DEFAULT_INFLUXDB_URL.to_string())
        .parse()?;
    let influxdb_username = std::env::var("INFLUXDB_USERNAME").ok();
    let influxdb_password = std::env::var("INFLUXDB_PASSWORD").ok();

    let mut influxdb_client = Client::new(influxdb_url, database);
    if let (Some(username), Some(password)) = (influxdb_username, influxdb_password) {
        influxdb_client = influxdb_client.set_authentication(username, password);
    }
    Ok(influxdb_client)
}

/// Construct the `MqttOptions` for connecting to the MQTT broker based on configuration options or
/// defaults.
pub fn get_mqtt_options(client_name_suffix: &str) -> MqttOptions {
    let client_name_prefix = std::env::var("MQTT_CLIENT_PREFIX")
        .unwrap_or_else(|_| DEFAULT_MQTT_CLIENT_PREFIX.to_string());
    let client_name = format!("{}-{}", client_name_prefix, client_name_suffix);

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
