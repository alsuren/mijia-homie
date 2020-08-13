use async_channel::{SendError, Sender};
use rumqttc::{self, EventLoop, LastWill, MqttOptions, Publish, QoS, Request};
use std::error::Error;
use tokio::task::{self, JoinHandle};

const HOMIE_VERSION: &str = "4.0";

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

#[derive(Clone, Debug)]
pub struct Property {
    id: String,
    name: String,
    datatype: Datatype,
    unit: Option<String>,
}

impl Property {
    pub fn new(id: &str, name: &str, datatype: Datatype, unit: Option<&str>) -> Property {
        Property {
            id: id.to_string(),
            name: name.to_string(),
            datatype: datatype,
            unit: unit.map(|s| s.to_string()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    id: String,
    name: String,
    node_type: String,
    properties: Vec<Property>,
}

impl Node {
    pub fn new(id: String, name: String, node_type: String, properties: Vec<Property>) -> Node {
        Node {
            id,
            name,
            node_type,
            properties,
        }
    }
}

pub(crate) struct HomieDevice {
    requests_tx: Sender<Request>,
    device_base: String,
    device_name: String,
    nodes: Vec<Node>,
}

impl HomieDevice {
    pub async fn new(
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

        let homie = HomieDevice {
            requests_tx: event_loop.handle(),
            device_base: device_base.to_string(),
            device_name: device_name.to_string(),
            nodes: vec![],
        };

        let join_handle: JoinHandle<Result<(), Box<dyn Error + Send + Sync>>> =
            task::spawn(async move {
                loop {
                    let (incoming, outgoing) = event_loop.poll().await?;
                    log::trace!("Incoming = {:?}, Outgoing = {:?}", incoming, outgoing);
                }
            });

        (homie, join_handle)
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
