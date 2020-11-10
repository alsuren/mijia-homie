use eyre::WrapErr;
use homie_controller::{Datatype, Device, HomieController, Node, Property};
use influx_db_client::{Client, Point, Precision, Value};
use std::time::SystemTime;

const INFLUXDB_PRECISION: Option<Precision> = Some(Precision::Milliseconds);

pub async fn send_property_value(
    controller: &HomieController,
    influx_db_client: &Client,
    device_id: String,
    node_id: String,
    property_id: String,
) -> Result<(), eyre::Report> {
    if let Some(device) = controller.devices().get(&device_id) {
        if let Some(node) = device.nodes.get(&node_id) {
            if let Some(property) = node.properties.get(&property_id) {
                if let Some(point) =
                    point_for_property_value(device, node, property, SystemTime::now())
                {
                    // Passing None for rp should use the default retention policy for the database.
                    influx_db_client
                        .write_point(point, INFLUXDB_PRECISION, None)
                        .await
                        .wrap_err("Failed to send property value update to InfluxDB")?;
                }
            }
        }
    }
    Ok(())
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
        point = point.add_tag("device_name", Value::String(device_name));
    }
    if let Some(node_name) = node.name.to_owned() {
        point = point.add_tag("node_name", Value::String(node_name));
    }
    if let Some(property_name) = property.name.to_owned() {
        point = point.add_tag("property_name", Value::String(property_name));
    }
    if let Some(unit) = property.unit.to_owned() {
        point = point.add_tag("unit", Value::String(unit));
    }
    if let Some(node_type) = node.node_type.to_owned() {
        point = point.add_tag("node_type", Value::String(node_type))
    }

    Some(point)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
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
        assert_eq!(
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
        assert_eq!(
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
        assert_eq!(
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
        assert_eq!(
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
        assert_eq!(
            influx_value_for_homie_property(&property).unwrap(),
            Value::String("12,34,56".to_owned()),
        );
    }
}
