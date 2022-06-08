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
        _ => property.value.to_owned()?.into(),
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
        .add_tag("device_id", device.id.to_owned())
        .add_tag("node_id", node.id.to_owned())
        .add_tag("property_id", property.id.to_owned());
    if let Some(device_name) = device.name.to_owned() {
        point = point.add_tag("device_name", device_name);
    }
    if let Some(node_name) = node.name.to_owned() {
        point = point.add_tag("node_name", node_name);
    }
    if let Some(property_name) = property.name.to_owned() {
        point = point.add_tag("property_name", property_name);
    }
    if let Some(unit) = property.unit.to_owned() {
        point = point.add_tag("unit", unit);
    }
    if let Some(node_type) = node.node_type.to_owned() {
        point = point.add_tag("node_type", node_type)
    }
    if let Some(Datatype::Boolean) = property.datatype {
        // Grafana is unable to display booleans directly, so add an integer for convenience.
        // https://github.com/grafana/grafana/issues/8152
        // https://github.com/grafana/grafana/issues/24929
        point = point.add_field(
            "value_int",
            Value::Integer(if property.value().ok()? { 1 } else { 0 }),
        )
    }

    Some(point)
}

#[cfg(test)]
mod tests {
    use super::*;
    use homie_controller::State;
    use std::collections::HashMap;
    use std::time::Duration;

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
            Value::from("abc".to_owned()),
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
            Value::from("abc".to_owned()),
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
            Value::from("12,34,56".to_owned()),
        );
    }

    fn property_set(properties: Vec<Property>) -> HashMap<String, Property> {
        properties
            .into_iter()
            .map(|property| (property.id.clone(), property))
            .collect()
    }

    fn node_set(nodes: Vec<Node>) -> HashMap<String, Node> {
        nodes
            .into_iter()
            .map(|node| (node.id.clone(), node))
            .collect()
    }

    #[test]
    fn point_for_minimal_property() {
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
        let node = Node {
            id: "node_id".to_owned(),
            name: None,
            node_type: None,
            properties: property_set(vec![property.clone()]),
        };
        let device = Device {
            id: "device_id".to_owned(),
            homie_version: "4.0".to_owned(),
            name: None,
            state: State::Unknown,
            implementation: None,
            nodes: node_set(vec![node.clone()]),
            extensions: Vec::default(),
            local_ip: None,
            mac: None,
            firmware_name: None,
            firmware_version: None,
            stats_interval: None,
            stats_uptime: None,
            stats_signal: None,
            stats_cputemp: None,
            stats_cpuload: None,
            stats_battery: None,
            stats_freeheap: None,
            stats_supply: None,
        };
        let timestamp_millis = 123456789;
        let timestamp = SystemTime::UNIX_EPOCH + Duration::from_millis(timestamp_millis as u64);
        let point = point_for_property_value(&device, &node, &property, timestamp).unwrap();
        assert_eq!(
            point,
            Point::new("integer")
                .add_timestamp(timestamp_millis)
                .add_tag("device_id", "device_id".to_owned())
                .add_tag("node_id", "node_id".to_owned())
                .add_tag("property_id", "property_id".to_owned())
                .add_field("value", 42),
        );
    }

    #[test]
    fn point_for_full_property() {
        let property = Property {
            id: "property_id".to_owned(),
            name: Some("Property name".to_owned()),
            datatype: Some(Datatype::Integer),
            settable: false,
            retained: true,
            unit: None,
            format: None,
            value: Some("42".to_owned()),
        };
        let node = Node {
            id: "node_id".to_owned(),
            name: Some("Node name".to_owned()),
            node_type: Some("node type".to_owned()),
            properties: property_set(vec![property.clone()]),
        };
        let device = Device {
            id: "device_id".to_owned(),
            homie_version: "4.0".to_owned(),
            name: Some("Device name".to_owned()),
            state: State::Unknown,
            implementation: None,
            nodes: node_set(vec![node.clone()]),
            extensions: Vec::default(),
            local_ip: None,
            mac: None,
            firmware_name: None,
            firmware_version: None,
            stats_interval: None,
            stats_uptime: None,
            stats_signal: None,
            stats_cputemp: None,
            stats_cpuload: None,
            stats_battery: None,
            stats_freeheap: None,
            stats_supply: None,
        };

        let timestamp_millis = 123456789;
        let timestamp = SystemTime::UNIX_EPOCH + Duration::from_millis(timestamp_millis as u64);
        let point = point_for_property_value(&device, &node, &property, timestamp).unwrap();
        assert_eq!(
            point,
            Point::new("integer")
                .add_timestamp(timestamp_millis)
                .add_tag("device_id", "device_id".to_owned())
                .add_tag("node_id", "node_id".to_owned())
                .add_tag("property_id", "property_id".to_owned())
                .add_tag("node_type", "node type".to_owned())
                .add_tag("device_name", "Device name".to_owned())
                .add_tag("node_name", "Node name".to_owned())
                .add_tag("property_name", "Property name".to_owned())
                .add_field("value", Value::Integer(42)),
        );
    }

    #[test]
    fn point_for_boolean_property() {
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
        let node = Node {
            id: "node_id".to_owned(),
            name: None,
            node_type: None,
            properties: property_set(vec![property.clone()]),
        };
        let device = Device {
            id: "device_id".to_owned(),
            homie_version: "4.0".to_owned(),
            name: None,
            state: State::Unknown,
            implementation: None,
            nodes: node_set(vec![node.clone()]),
            extensions: Vec::default(),
            local_ip: None,
            mac: None,
            firmware_name: None,
            firmware_version: None,
            stats_interval: None,
            stats_uptime: None,
            stats_signal: None,
            stats_cputemp: None,
            stats_cpuload: None,
            stats_battery: None,
            stats_freeheap: None,
            stats_supply: None,
        };
        let timestamp_millis = 123456789;
        let timestamp = SystemTime::UNIX_EPOCH + Duration::from_millis(timestamp_millis as u64);
        let point = point_for_property_value(&device, &node, &property, timestamp).unwrap();
        assert_eq!(
            point,
            Point::new("boolean")
                .add_timestamp(timestamp_millis)
                .add_tag("device_id", "device_id".to_owned())
                .add_tag("node_id", "node_id".to_owned())
                .add_tag("property_id", "property_id".to_owned())
                .add_field("value", true)
                .add_field("value_int", 1),
        );
    }
}
