use futures::future::try_join_all;
use futures::FutureExt;
use homie_controller::{
    Datatype, Device, Event, HomieController, HomieEventLoop, Node, PollError, Property,
};
use influx_db_client::reqwest::Url;
use influx_db_client::{Client, Point, Precision, Value};
use rumqttc::MqttOptions;
use rustls::ClientConfig;
use stable_eyre::eyre;
use stable_eyre::eyre::WrapErr;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::task::{self, JoinHandle};

const DEFAULT_MQTT_CLIENT_NAME: &str = "homie-influx";
const DEFAULT_MQTT_HOST: &str = "test.mosquitto.org";
const DEFAULT_MQTT_PORT: u16 = 1883;
const DEFAULT_INFLUXDB_URL: &str = "http://localhost:8086";

const INFLUXDB_PRECISION: Option<Precision> = Some(Precision::Milliseconds);

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

        let handle = spawn_homie_poll_loop(event_loop, controller.clone(), influxdb_client);
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
    influx_db_client: Client,
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
                        if fresh {
                            send_property_value(
                                controller.as_ref(),
                                &influx_db_client,
                                device_id,
                                node_id,
                                property_id,
                            )
                            .await;
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

async fn send_property_value(
    controller: &HomieController,
    influx_db_client: &Client,
    device_id: String,
    node_id: String,
    property_id: String,
) {
    if let Some(device) = controller.devices().get(&device_id) {
        if let Some(node) = device.nodes.get(&node_id) {
            if let Some(property) = node.properties.get(&property_id) {
                if let Some(point) =
                    point_for_property_value(device, node, property, SystemTime::now())
                {
                    // TODO: What should rp be?
                    // TODO: Handle errors
                    influx_db_client
                        .write_point(point, INFLUXDB_PRECISION, None)
                        .await
                        .unwrap();
                }
            }
        }
    }
}

/// Convert the value of the given Homie property to an InfluxDB value of the appropriate type, if
/// possible. Returns None if the datatype of the property is unknown, or there was an error parsing
/// the value.
fn influx_value_for_homie_property(property: &Property) -> Option<Value> {
    let datatype = property.datatype?;
    Some(match datatype {
        Datatype::Integer => Value::Integer(property.value().ok()?),
        Datatype::Float => Value::Float(property.value().ok()?),
        Datatype::Boolean => Value::Boolean(property.value().ok()?),
        _ => Value::String(property.value.to_owned()?),
    })
}

/// Construct an InfluxDB `Point` corresponding to the given Homie property value update.
fn point_for_property_value(
    device: &Device,
    node: &Node,
    property: &Property,
    timestamp: SystemTime,
) -> Option<Point> {
    let datatype = property.datatype?;
    let value = influx_value_for_homie_property(property)?;

    let mut point = Point::new(&datatype.to_string())
        .add_timestamp(
            timestamp
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
        )
        .add_field("value", value)
        .add_tag("device_id", Value::String(device.id.to_owned()))
        .add_tag("node_id", Value::String(node.id.to_owned()))
        .add_tag("property_id", Value::String(property.id.to_owned()));
    if let Some(device_name) = device.name.to_owned() {
        point = point.add_field("device_name", Value::String(device_name));
    }
    if let Some(node_name) = node.name.to_owned() {
        point = point.add_field("node_name", Value::String(node_name));
    }
    if let Some(property_name) = property.name.to_owned() {
        point = point.add_field("property_name", Value::String(property_name));
    }
    if let Some(unit) = property.unit.to_owned() {
        point = point.add_tag("unit", Value::String(unit));
    }
    if let Some(node_type) = node.node_type.to_owned() {
        point = point.add_tag("node_type", Value::String(node_type))
    }

    Some(point)
}

fn simplify_unit_vec<E>(m: Result<Vec<()>, E>) -> Result<(), E> {
    m.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Debug;

    // TODO: Remove once Value implements PartialEq.
    fn assert_debug_eq(a: impl Debug, b: impl Debug) {
        assert_eq!(format!("{:?}", a), format!("{:?}", b));
    }

    #[test]
    fn influx_value_for_integer() {
        let property = Property {
            id: "property_id".to_owned(),
            name: None,
            datatype: Some(Datatype::Integer),
            settable: false,
            retained: true,
            unit: None,
            format: None,
            value: Some("42".to_owned()),
        };
        assert_debug_eq(
            influx_value_for_homie_property(&property).unwrap(),
            Value::Integer(42),
        );
    }

    #[test]
    fn influx_value_for_float() {
        let property = Property {
            id: "property_id".to_owned(),
            name: None,
            datatype: Some(Datatype::Float),
            settable: false,
            retained: true,
            unit: None,
            format: None,
            value: Some("42.3".to_owned()),
        };
        assert_debug_eq(
            influx_value_for_homie_property(&property).unwrap(),
            Value::Float(42.3),
        );
    }

    #[test]
    fn influx_value_for_boolean() {
        let property = Property {
            id: "property_id".to_owned(),
            name: None,
            datatype: Some(Datatype::Boolean),
            settable: false,
            retained: true,
            unit: None,
            format: None,
            value: Some("true".to_owned()),
        };
        assert_debug_eq(
            influx_value_for_homie_property(&property).unwrap(),
            Value::Boolean(true),
        );
    }

    #[test]
    fn influx_value_for_string() {
        let property = Property {
            id: "property_id".to_owned(),
            name: None,
            datatype: Some(Datatype::String),
            settable: false,
            retained: true,
            unit: None,
            format: None,
            value: Some("abc".to_owned()),
        };
        assert_debug_eq(
            influx_value_for_homie_property(&property).unwrap(),
            Value::String("abc".to_owned()),
        );
    }

    #[test]
    fn influx_value_for_enum() {
        let property = Property {
            id: "property_id".to_owned(),
            name: None,
            datatype: Some(Datatype::Enum),
            settable: false,
            retained: true,
            unit: None,
            format: None,
            value: Some("abc".to_owned()),
        };
        assert_debug_eq(
            influx_value_for_homie_property(&property).unwrap(),
            Value::String("abc".to_owned()),
        );
    }

    #[test]
    fn influx_value_for_color() {
        let property = Property {
            id: "property_id".to_owned(),
            name: None,
            datatype: Some(Datatype::Color),
            settable: false,
            retained: true,
            unit: None,
            format: None,
            value: Some("12,34,56".to_owned()),
        };
        assert_debug_eq(
            influx_value_for_homie_property(&property).unwrap(),
            Value::String("12,34,56".to_owned()),
        );
    }
}
