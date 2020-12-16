use eyre::Report;
use influx_db_client::reqwest::Url;
use influx_db_client::Client;
use rumqttc::MqttOptions;
use rustls::ClientConfig;
use serde::{Deserialize as _, Deserializer};
use serde_derive::Deserialize;
use stable_eyre::eyre;
use stable_eyre::eyre::WrapErr;
use std::fs::read_to_string;
use std::sync::Arc;
use std::time::Duration;

const DEFAULT_MQTT_CLIENT_PREFIX: &str = "homie-influx";
const DEFAULT_MQTT_HOST: &str = "test.mosquitto.org";
const DEFAULT_MQTT_PORT: u16 = 1883;
const DEFAULT_MQTT_RECONNECT_INTERVAL: Duration = Duration::from_secs(5);
const DEFAULT_INFLUXDB_URL: &str = "http://localhost:8086";
const CONFIG_FILENAME: &str = "homie-influx.toml";
const DEFAULT_MAPPINGS_FILENAME: &str = "mappings.toml";

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub mqtt: MqttConfig,
    pub homie: HomieConfig,
    pub influxdb: InfluxDBConfig,
}

impl Config {
    pub fn from_file() -> Result<Config, Report> {
        Config::read(CONFIG_FILENAME)
    }

    fn read(filename: &str) -> Result<Config, Report> {
        let config_file =
            read_to_string(filename).wrap_err_with(|| format!("Reading {}", filename))?;
        Ok(toml::from_str(&config_file)?)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MqttConfig {
    pub host: String,
    pub port: u16,
    pub use_tls: bool,
    pub username: Option<String>,
    pub password: Option<String>,
    pub client_prefix: String,
    #[serde(
        deserialize_with = "de_duration_seconds",
        rename = "reconnect_interval_seconds"
    )]
    pub reconnect_interval: Duration,
}

/// Deserialize an integer as a number of seconds.
fn de_duration_seconds<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
    let seconds = u64::deserialize(d)?;
    Ok(Duration::from_secs(seconds))
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
            reconnect_interval: DEFAULT_MQTT_RECONNECT_INTERVAL,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
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
#[serde(default, deny_unknown_fields)]
pub struct InfluxDBConfig {
    pub url: Url,
    pub username: Option<String>,
    pub password: Option<String>,
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
#[serde(deny_unknown_fields)]
pub struct Mapping {
    pub homie_prefix: String,
    pub influxdb_database: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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
    mqtt_options.set_clean_session(false);

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

#[cfg(test)]
mod tests {
    use super::*;

    /// Parsing the example config file should not give any errors.
    #[test]
    fn example_config() {
        Config::read("homie-influx.example.toml").unwrap();
    }

    /// Parsing an empty config file should not give any errors.
    #[test]
    fn empty_config() {
        toml::from_str::<Config>("").unwrap();
    }

    /// Parsing the example mappings file should not give any errors.
    #[test]
    fn example_mappings() {
        let mappings = mappings_from_file("mappings.example.toml").unwrap();
        assert_eq!(mappings.len(), 1);
    }
}
