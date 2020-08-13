use async_channel::{SendError, Sender};
use rumqttc::{self, EventLoop, LastWill, MqttOptions, Publish, QoS, Request};
use std::error::Error;
use tokio::task::{self, JoinHandle};

const HOMIE_VERSION: &str = "4.0";

/// The data type for a Homie property.
#[derive(Clone, Copy, Debug)]
pub enum Datatype {
    Integer,
    Float,
    Boolean,
    String,
    Enum,
    Color,
}

impl Datatype {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Integer => "integer",
            Self::Float => "float",
            Self::Boolean => "boolean",
            Self::String => "string",
            Self::Enum => "enum",
            Self::Color => "color",
        }
    }
}

/// A [property](https://homieiot.github.io/specification/#properties) of a Homie node.
#[derive(Clone, Debug)]
pub struct Property {
    id: String,
    name: String,
    datatype: Datatype,
    unit: Option<String>,
}

impl Property {
    /// Create a new property with the given attributes.
    ///
    /// # Arguments
    /// * `id`: The topic ID for the property. This must be unique per node, and follow the Homie
    ///   [ID format](https://homieiot.github.io/specification/#topic-ids).
    /// * `name`: The human-readable name of the property.
    /// * `datatype`: The data type of the property.
    /// * `unit`: The unit for the property, if any. This may be one of the
    ///   [recommended units](https://homieiot.github.io/specification/#property-attributes), or
    ///   any other custom unit.
    pub fn new(id: &str, name: &str, datatype: Datatype, unit: Option<&str>) -> Property {
        Property {
            id: id.to_owned(),
            name: name.to_owned(),
            datatype: datatype,
            unit: unit.map(|s| s.to_owned()),
        }
    }
}

/// A [node](https://homieiot.github.io/specification/#nodes) of a Homie device.
#[derive(Clone, Debug)]
pub struct Node {
    id: String,
    name: String,
    node_type: String,
    properties: Vec<Property>,
}

impl Node {
    /// Create a new node with the given attributes.
    ///
    /// # Arguments
    /// * `id`: The topic ID for the node. This must be unique per device, and follow the Homie
    ///   [ID format](https://homieiot.github.io/specification/#topic-ids).
    /// * `name`: The human-readable name of the node.
    /// * `type`: The type of the node. This is an arbitrary string.
    /// * `property`: The properties of the node. There should be at least one.
    pub fn new(id: String, name: String, node_type: String, properties: Vec<Property>) -> Node {
        Node {
            id,
            name,
            node_type,
            properties,
        }
    }
}

/// A Homie [device](https://homieiot.github.io/specification/#devices). This corresponds to a
/// single MQTT connection.
pub struct HomieDevice {
    requests_tx: Sender<Request>,
    device_base: String,
    device_name: String,
    nodes: Vec<Node>,
}

impl HomieDevice {
    /// Create a new Homie device, connect to the MQTT server, and start a task to handle the MQTT
    /// connection.
    ///
    /// # Arguments
    /// * `device_base`: The base topic ID for the device, including the Homie base topic. This
    ///   might be something like "homie/my-device-id" if you are using the default Homie
    ///   [base topic](https://homieiot.github.io/specification/#base-topic). This must be
    ///   unique per MQTT server.
    /// * `device_name`: The human-readable name of the device.
    /// * `mqtt_options`: Options for the MQTT connection, including which server to connect to.
    ///
    /// # Return value
    /// A pair of the `HomieDevice` itself, and a `JoinHandle` for the task which handles the MQTT
    /// connection. You should join on this handle to allow the connection to make progress, and
    /// handle any errors it returns.
    pub async fn spawn(
        device_base: &str,
        device_name: &str,
        mut mqtt_options: MqttOptions,
    ) -> (
        HomieDevice,
        JoinHandle<Result<(), Box<dyn Error + Send + Sync>>>,
    ) {
        mqtt_options.set_last_will(LastWill {
            topic: format!("{}/$state", device_base),
            message: "lost".to_string(),
            qos: QoS::AtLeastOnce,
            retain: true,
        });
        let mut event_loop = EventLoop::new(mqtt_options, 10).await;

        let homie = HomieDevice::new(
            event_loop.handle(),
            device_base.to_string(),
            device_name.to_string(),
        );

        let join_handle = task::spawn(async move {
            loop {
                let (incoming, outgoing) = event_loop.poll().await?;
                log::trace!("Incoming = {:?}, Outgoing = {:?}", incoming, outgoing);
            }
        });

        (homie, join_handle)
    }

    fn new(requests_tx: Sender<Request>, device_base: String, device_name: String) -> HomieDevice {
        HomieDevice {
            requests_tx,
            device_base,
            device_name,
            nodes: vec![],
        }
    }

    pub async fn start(&self) -> Result<(), SendError<Request>> {
        publish_retained(
            &self.requests_tx,
            format!("{}/$homie", self.device_base),
            HOMIE_VERSION,
        )
        .await?;
        publish_retained(
            &self.requests_tx,
            format!("{}/$extensions", self.device_base),
            "",
        )
        .await?;
        publish_retained(
            &self.requests_tx,
            format!("{}/$name", self.device_base),
            &self.device_name,
        )
        .await?;
        publish_retained(
            &self.requests_tx,
            format!("{}/$state", self.device_base),
            "init",
        )
        .await?;
        Ok(())
    }

    pub fn add_node(&mut self, node: Node) {
        // First check that there isn't already a node with the same ID.
        if self.nodes.iter().any(|n| n.id == node.id) {
            panic!("Tried to add node with duplicate ID: {:?}", node);
        }
        self.nodes.push(node);
    }

    pub async fn publish_nodes(&self) -> Result<(), SendError<Request>> {
        let mut node_ids: Vec<&str> = vec![];
        for node in &self.nodes {
            let node_base = format!("{}/{}", self.device_base, node.id);
            node_ids.push(&node.id);
            publish_retained(
                &self.requests_tx,
                format!("{}/$name", node_base),
                &node.name,
            )
            .await?;
            publish_retained(
                &self.requests_tx,
                format!("{}/$type", node_base),
                &node.node_type,
            )
            .await?;
            let mut property_ids: Vec<&str> = vec![];
            for property in &node.properties {
                property_ids.push(&property.id);
                publish_retained(
                    &self.requests_tx,
                    format!("{}/{}/$name", node_base, property.id),
                    &property.name,
                )
                .await?;
                publish_retained(
                    &self.requests_tx,
                    format!("{}/{}/$datatype", node_base, property.id),
                    property.datatype.as_str(),
                )
                .await?;
                if let Some(unit) = &property.unit {
                    publish_retained(
                        &self.requests_tx,
                        format!("{}/{}/$unit", node_base, property.id),
                        &unit,
                    )
                    .await?;
                }
            }
            publish_retained(
                &self.requests_tx,
                format!("{}/$properties", node_base),
                &property_ids.join(","),
            )
            .await?;
        }
        publish_retained(
            &self.requests_tx,
            format!("{}/$nodes", self.device_base),
            &node_ids.join(","),
        )
        .await?;
        publish_retained(
            &self.requests_tx,
            format!("{}/$state", self.device_base),
            "ready",
        )
        .await?;
        Ok(())
    }

    pub async fn publish_value(
        &self,
        node_id: &str,
        property_id: &str,
        value: &str,
    ) -> Result<(), SendError<Request>> {
        publish_retained(
            &self.requests_tx,
            format!("{}/{}/{}", self.device_base, node_id, property_id),
            value,
        )
        .await
    }
}

async fn publish_retained(
    requests_tx: &Sender<Request>,
    name: String,
    value: &str,
) -> Result<(), SendError<Request>> {
    let mut publish = Publish::new(name, QoS::AtLeastOnce, value);
    publish.set_retain(true);
    requests_tx.send(publish.into()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic]
    fn test_duplicate_node() {
        let (tx, _) = async_channel::unbounded();
        let mut device = HomieDevice::new(
            tx,
            "homie/test-device".to_string(),
            "Test device".to_string(),
        );

        device.add_node(Node::new(
            "id".to_string(),
            "Name".to_string(),
            "type".to_string(),
            vec![],
        ));
        device.add_node(Node::new(
            "id".to_string(),
            "Name 2".to_string(),
            "type2".to_string(),
            vec![],
        ));
    }
}
