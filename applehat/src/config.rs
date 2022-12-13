use eyre::{Report, WrapErr};
use rumqttc::{MqttOptions, Transport};
use rustls::{ClientConfig, RootCertStore};
use serde::{Deserialize, Deserializer};
use std::{fs::read_to_string, time::Duration};

const DEFAULT_HOST: &str = "test.mosquitto.org";
const DEFAULT_PORT: u16 = 1883;
const DEFAULT_CLIENT_NAME: &str = "applehat";
const DEFAULT_MQTT_RECONNECT_INTERVAL: Duration = Duration::from_secs(5);
const DEFAULT_MQTT_PREFIX: &str = "homie";
const CONFIG_FILENAME: &str = "applehat.toml";
const KEEP_ALIVE: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub mqtt: MqttConfig,
    pub homie: HomieConfig,
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
    pub client_name: String,
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
            host: DEFAULT_HOST.to_owned(),
            port: DEFAULT_PORT,
            use_tls: false,
            username: None,
            password: None,
            client_name: DEFAULT_CLIENT_NAME.to_owned(),
            reconnect_interval: DEFAULT_MQTT_RECONNECT_INTERVAL,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HomieConfig {
    pub prefix: String,
}

impl Default for HomieConfig {
    fn default() -> HomieConfig {
        HomieConfig {
            prefix: DEFAULT_MQTT_PREFIX.to_owned(),
        }
    }
}

/// Construct the `MqttOptions` for connecting to the MQTT broker based on configuration options or
/// defaults.
pub fn get_mqtt_options(config: MqttConfig) -> MqttOptions {
    let mut mqtt_options = MqttOptions::new(config.client_name, config.host, config.port);

    mqtt_options.set_keep_alive(KEEP_ALIVE);
    if let (Some(username), Some(password)) = (config.username, config.password) {
        mqtt_options.set_credentials(username, password);
    }

    if config.use_tls {
        let mut root_store = RootCertStore::empty();
        for cert in
            rustls_native_certs::load_native_certs().expect("Failed to load platform certificates.")
        {
            root_store.add(&rustls::Certificate(cert.0)).unwrap();
        }
        let client_config = ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_store)
            .with_no_client_auth();
        mqtt_options.set_transport(Transport::tls_with_config(client_config.into()));
    }
    mqtt_options
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parsing the example config file should not give any errors.
    #[test]
    fn example_config() {
        Config::read("applehat.example.toml").unwrap();
    }

    /// Parsing an empty config file should not give any errors.
    #[test]
    fn empty_config() {
        toml::from_str::<Config>("").unwrap();
    }
}
