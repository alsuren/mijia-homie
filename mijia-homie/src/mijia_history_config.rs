use eyre::Report;
use influx_db_client::{reqwest::Url, Client};
use serde_derive::Deserialize;
use stable_eyre::eyre::WrapErr;
use std::fs::read_to_string;

const DEFAULT_DATABASE: &str = "mijia_history";
const DEFAULT_MEASUREMENT: &str = "mijia_history";
const DEFAULT_INFLUXDB_URL: &str = "http://localhost:8086";
const DEFAULT_SENSOR_NAMES_FILENAME: &str = "sensor-names.toml";
const CONFIG_FILENAME: &str = "mijia-history-influx.toml";

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub sensor_names_filename: String,
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

impl Default for Config {
    fn default() -> Config {
        Config {
            sensor_names_filename: DEFAULT_SENSOR_NAMES_FILENAME.to_owned(),
            influxdb: Default::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InfluxDBConfig {
    pub url: Url,
    pub username: Option<String>,
    pub password: Option<String>,
    pub database: String,
    pub measurement: String,
}

impl Default for InfluxDBConfig {
    fn default() -> InfluxDBConfig {
        InfluxDBConfig {
            url: DEFAULT_INFLUXDB_URL.parse().unwrap(),
            username: None,
            password: None,
            database: DEFAULT_DATABASE.to_owned(),
            measurement: DEFAULT_MEASUREMENT.to_owned(),
        }
    }
}

/// Construct a new InfluxDB `Client` based on the given configuration options, for the given
/// database.
pub fn get_influxdb_client(config: &InfluxDBConfig) -> Result<Client, Report> {
    let mut influxdb_client = Client::new(config.url.to_owned(), &config.database);
    if let (Some(username), Some(password)) = (&config.username, &config.password) {
        influxdb_client = influxdb_client.set_authentication(username, password);
    }
    Ok(influxdb_client)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parsing the example config file should not give any errors.
    #[test]
    fn example_config() {
        Config::read("mijia-history-influx.example.toml").unwrap();
    }

    /// Parsing an empty config file should not give any errors.
    #[test]
    fn empty_config() {
        toml::from_str::<Config>("").unwrap();
    }
}
