use eyre::Report;
use influx_db_client::reqwest::Url;
use influx_db_client::Client;
use rumqttc::MqttOptions;
use rustls::ClientConfig;
use serde::{de, Deserialize, Deserializer};
use stable_eyre::eyre;
use stable_eyre::eyre::WrapErr;
use std::fmt::Display;
use std::fs::read_to_string;
use std::str::FromStr;
use std::sync::Arc;

const DEFAULT_MQTT_CLIENT_PREFIX: &str = "homie-influx";
const DEFAULT_MQTT_HOST: &str = "test.mosquitto.org";
const DEFAULT_MQTT_PORT: u16 = 1883;
const DEFAULT_INFLUXDB_URL: &str = "http://localhost:8086";
const CONFIG_FILENAME: &str = "homie_influx.toml";
const DEFAULT_MAPPINGS_FILENAME: &str = "mappings.toml";

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub mqtt: MqttConfig,
    pub homie: HomieConfig,
    pub influxdb: InfluxDBConfig,
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
    pub client_prefix: String,
}

impl Default for MqttConfig {
    fn default() -> MqttConfig {
        MqttConfig {
            host: DEFAULT_MQTT_HOST.to_owned(),
            port: DEFAULT_MQTT_PORT,
            use_tls: false,
            username: None,
            password: None,
            client_prefix: DEFAULT_MQTT_CLIENT_PREFIX.to_owned(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct HomieConfig {
    pub mappings_filename: String,
}

impl Default for HomieConfig {
    fn default() -> HomieConfig {
        HomieConfig {
            mappings_filename: DEFAULT_MAPPINGS_FILENAME.to_owned(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct InfluxDBConfig {
    #[serde(deserialize_with = "de_from_str")]
    pub url: Url,
    pub username: Option<String>,
    pub password: Option<String>,
}

/// Deserialize a FromStr by deserializing it as a string then parsing it.
fn de_from_str<'de, D: Deserializer<'de>, T: FromStr>(d: D) -> Result<T, D::Error>
where
    T::Err: Display,
{
    let s = String::deserialize(d)?;
    s.parse::<T>().map_err(de::Error::custom)
}

impl Default for InfluxDBConfig {
    fn default() -> InfluxDBConfig {
        InfluxDBConfig {
            url: DEFAULT_INFLUXDB_URL.parse().unwrap(),
            username: None,
            password: None,
        }
    }
}

/// A mapping from a Homie prefix to monitor to an InfluxDB database where its data should be
/// stored.
#[derive(Clone, Debug, Deserialize)]
pub struct Mapping {
    pub homie_prefix: String,
    pub influxdb_database: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MappingsConfig {
    pub mappings: Vec<Mapping>,
}

/// Read mappings from the configured file, and make sure there is at least one.
pub fn read_mappings(config: &HomieConfig) -> Result<Vec<Mapping>, Report> {
    mappings_from_file(&config.mappings_filename)
}

/// Read mappings of the form "homie_prefix:influxdb_database" from the given file, ignoring any
/// lines starting with '#'.
fn mappings_from_file(filename: &str) -> Result<Vec<Mapping>, Report> {
    let mappings_file =
        read_to_string(filename).wrap_err_with(|| format!("Reading {}", filename))?;
    let mappings = toml::from_str::<MappingsConfig>(&mappings_file)?;
    if mappings.mappings.is_empty() {
        eyre::bail!("At least one mapping must be configured in {}.", filename);
    }
    Ok(mappings.mappings)
}

/// Construct a new InfluxDB `Client` based on the given configuration options, for the given
/// database.
pub fn get_influxdb_client(config: &InfluxDBConfig, database: &str) -> Result<Client, Report> {
    let mut influxdb_client = Client::new(config.url.to_owned(), database);
    if let (Some(username), Some(password)) = (&config.username, &config.password) {
        influxdb_client = influxdb_client.set_authentication(username, password);
    }
    Ok(influxdb_client)
}

/// Construct the `MqttOptions` for connecting to the MQTT broker based on configuration options or
/// defaults.
pub fn get_mqtt_options(config: &MqttConfig, client_name_suffix: &str) -> MqttOptions {
    let client_name = format!("{}-{}", config.client_prefix, client_name_suffix);
    let mut mqtt_options = MqttOptions::new(client_name, &config.host, config.port);
    mqtt_options.set_keep_alive(5);

    if let (Some(username), Some(password)) = (&config.username, &config.password) {
        mqtt_options.set_credentials(username, password);
    }

    if config.use_tls {
        let mut client_config = ClientConfig::new();
        client_config.root_store = rustls_native_certs::load_native_certs()
            .expect("Failed to load platform certificates.");
        mqtt_options.set_tls_client_config(Arc::new(client_config));
    }

    mqtt_options
}
