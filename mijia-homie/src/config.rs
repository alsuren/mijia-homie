use eyre::Report;
use mijia::MacAddress;
use rumqttc::MqttOptions;
use rustls::ClientConfig;
use serde_derive::Deserialize;
use stable_eyre::eyre::WrapErr;
use std::collections::HashMap;
use std::fs::{read_to_string, File};
use std::io::{BufRead, BufReader};
use std::sync::Arc;

const DEFAULT_MQTT_PREFIX: &str = "homie";
const DEFAULT_DEVICE_ID: &str = "mijia-bridge";
const DEFAULT_DEVICE_NAME: &str = "Mijia bridge";
const DEFAULT_HOST: &str = "test.mosquitto.org";
const DEFAULT_PORT: u16 = 1883;
const CONFIG_FILENAME: &str = "mijia_homie.toml";
const SENSOR_NAMES_FILENAME: &str = "sensor_names.conf";

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub mqtt: MqttConfig,
    pub homie: HomieConfig,
}

impl Config {
    pub fn from_file() -> Result<Config, Report> {
        let config_file = read_to_string(CONFIG_FILENAME)
            .wrap_err_with(|| format!("Reading {}", CONFIG_FILENAME))?;
        Ok(toml::from_str(&config_file)?)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct MqttConfig {
    pub host: String,
    pub port: u16,
    pub use_tls: bool,
    pub username: Option<String>,
    pub password: Option<String>,
    pub client_name: Option<String>,
}

impl Default for MqttConfig {
    fn default() -> MqttConfig {
        MqttConfig {
            host: DEFAULT_HOST.to_owned(),
            port: DEFAULT_PORT,
            use_tls: false,
            username: None,
            password: None,
            client_name: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct HomieConfig {
    pub device_id: String,
    pub device_name: String,
    pub prefix: String,
}

impl Default for HomieConfig {
    fn default() -> HomieConfig {
        HomieConfig {
            device_id: DEFAULT_DEVICE_ID.to_owned(),
            device_name: DEFAULT_DEVICE_NAME.to_owned(),
            prefix: DEFAULT_MQTT_PREFIX.to_owned(),
        }
    }
}

/// Construct the `MqttOptions` for connecting to the MQTT broker based on configuration options or
/// defaults.
pub fn get_mqtt_options(config: MqttConfig, device_id: &str) -> MqttOptions {
    let client_name = config.client_name.unwrap_or_else(|| device_id.to_owned());

    let mut mqtt_options = MqttOptions::new(client_name, config.host, config.port);

    mqtt_options.set_keep_alive(5);
    if let (Some(username), Some(password)) = (config.username, config.password) {
        mqtt_options.set_credentials(username, password);
    }

    if config.use_tls {
        let mut client_config = ClientConfig::new();
        client_config.root_store =
            rustls_native_certs::load_native_certs().expect("could not load platform certs");
        mqtt_options.set_tls_client_config(Arc::new(client_config));
    }
    mqtt_options
}

/// Read the given file of key-value pairs into a hashmap.
/// Returns an empty hashmap if the file doesn't exist, or an error if it is malformed.
fn hashmap_from_file(filename: &str) -> Result<HashMap<MacAddress, String>, Report> {
    let mut map: HashMap<MacAddress, String> = HashMap::new();
    if let Ok(file) = File::open(filename) {
        for line in BufReader::new(file).lines() {
            let line = line?;
            if !line.is_empty() && !line.starts_with('#') {
                let parts: Vec<&str> = line.splitn(2, '=').collect();
                if parts.len() != 2 {
                    eyre::bail!("Invalid line '{}'", line);
                }
                map.insert(parts[0].parse()?, parts[1].to_string());
            }
        }
    }
    Ok(map)
}

pub fn read_sensor_names() -> Result<HashMap<MacAddress, String>, Report> {
    Ok(hashmap_from_file(SENSOR_NAMES_FILENAME)
        .wrap_err(format!("reading {}", SENSOR_NAMES_FILENAME))?)
}
