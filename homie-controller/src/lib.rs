//! `homie-controller` is a library for creating controllers to interact via an MQTT broker with IoT
//! devices implementing the [Homie convention](https://homieiot.github.io/).

use rumqttc::{
    AsyncClient, ClientError, ConnectionError, EventLoop, Incoming, MqttOptions, Publish, QoS,
};
use std::collections::HashMap;
use std::str;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;
use tokio::task::JoinError;

mod types;
pub use types::{Datatype, Device, Node, ParseDatatypeError, ParseStateError, Property, State};

const REQUESTS_CAP: usize = 10;

/// Error type for futures representing tasks spawned by this crate.
#[derive(Error, Debug)]
pub enum PollError {
    #[error("{0}")]
    Client(#[from] ClientError),
    #[error("{0}")]
    Connection(#[from] ConnectionError),
    #[error("Task failed: {0}")]
    Join(#[from] JoinError),
    #[error("Internal error: {0}")]
    Internal(&'static str),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    DeviceUpdated {
        device_id: String,
        has_required_attributes: bool,
    },
    NodeUpdated {
        device_id: String,
        node_id: String,
        has_required_attributes: bool,
    },
    PropertyUpdated {
        device_id: String,
        node_id: String,
        property_id: String,
        has_required_attributes: bool,
    },
    PropertyValueChanged {
        device_id: String,
        node_id: String,
        property_id: String,
        value: String,
    },
}

impl Event {
    fn device_updated(device: &Device) -> Self {
        Event::DeviceUpdated {
            device_id: device.id.to_owned(),
            has_required_attributes: device.has_required_attributes(),
        }
    }

    fn node_updated(device_id: &str, node: &Node) -> Self {
        Event::NodeUpdated {
            device_id: device_id.to_owned(),
            node_id: node.id.to_owned(),
            has_required_attributes: node.has_required_attributes(),
        }
    }

    fn property_updated(device_id: &str, node_id: &str, property: &Property) -> Self {
        Event::PropertyUpdated {
            device_id: device_id.to_owned(),
            node_id: node_id.to_owned(),
            property_id: property.id.to_owned(),
            has_required_attributes: property.has_required_attributes(),
        }
    }

    fn property_value(device_id: &str, node_id: &str, property: &Property) -> Self {
        Event::PropertyValueChanged {
            device_id: device_id.to_owned(),
            node_id: node_id.to_owned(),
            property_id: property.id.to_owned(),
            value: property.value.to_owned().unwrap(),
        }
    }
}

/// A Homie controller, which connects to an MQTT broker and interacts with Homie devices.
#[derive(Debug)]
pub struct HomieController {
    mqtt_client: AsyncClient,
    base_topic: String,
    /// The set of Homie devices which have been discovered so far, keyed by their IDs.
    pub devices: Arc<Mutex<HashMap<String, Device>>>,
}

impl HomieController {
    /// Create a new `HomieController` connected to an MQTT broker.
    ///
    /// # Arguments
    /// * `base_topic`: The Homie [base topic](https://homieiot.github.io/specification/#base-topic)
    ///   under which to look for Homie devices. "homie" is the recommended default.
    /// * `mqtt_options`: Options for the MQTT connection, including which broker to connect to.
    pub fn new(mqtt_options: MqttOptions, base_topic: &str) -> (HomieController, EventLoop) {
        let (mqtt_client, event_loop) = AsyncClient::new(mqtt_options, REQUESTS_CAP);
        let controller = HomieController {
            mqtt_client,
            base_topic: base_topic.to_string(),
            devices: Arc::new(Mutex::new(HashMap::new())),
        };
        (controller, event_loop)
    }

    /// Poll the `EventLoop`, and maybe return a Homie event.
    pub async fn poll(&self, event_loop: &mut EventLoop) -> Result<Option<Event>, PollError> {
        let notification = event_loop.poll().await?;
        log::trace!("Notification = {:?}", notification);

        if let rumqttc::Event::Incoming(incoming) = notification {
            log::trace!("Incoming: {:?}", incoming);
            match incoming {
                Incoming::Publish(publish) => {
                    match self.handle_publish(publish).await {
                        Err(HandleError::Warning(err)) => {
                            // These error strings indicate some issue with parsing the publish
                            // event from the network, perhaps due to a malfunctioning device,
                            // so should just be logged and ignored.
                            log::warn!("{}", err)
                        }
                        Err(HandleError::Fatal(e)) => Err(e)?,
                        Ok(event) => return Ok(event),
                    }
                }
                _ => {}
            }
        }
        Ok(None)
    }

    async fn handle_publish(&self, publish: Publish) -> Result<Option<Event>, HandleError> {
        let base_topic = format!("{}/", self.base_topic);
        let payload = str::from_utf8(&publish.payload)
            .map_err(|e| format!("Payload not valid UTF-8: {}", e))?;
        let subtopic = publish
            .topic
            .strip_prefix(&base_topic)
            .ok_or_else(|| format!("Publish with unexpected topic: {:?}", publish))?;
        let devices = &mut *self.devices.lock().await;
        let parts = subtopic.split("/").collect::<Vec<&str>>();
        match parts.as_slice() {
            [device_id, "$homie"] => {
                if !devices.contains_key(*device_id) {
                    self.new_device(devices, device_id, payload).await?;
                    return Ok(Some(Event::DeviceUpdated {
                        device_id: device_id.to_string(),
                        has_required_attributes: false,
                    }));
                }
            }
            [device_id, "$name"] => {
                let device = get_mut_device_for(devices, "Got name for", device_id)?;
                device.name = Some(payload.to_owned());
                return Ok(Some(Event::device_updated(device)));
            }
            [device_id, "$state"] => {
                let state = payload.parse()?;
                let device = get_mut_device_for(devices, "Got state for", device_id)?;
                device.state = state;
                return Ok(Some(Event::device_updated(device)));
            }
            [device_id, "$implementation"] => {
                let device = get_mut_device_for(devices, "Got implementation for", device_id)?;
                device.implementation = Some(payload.to_owned());
                return Ok(Some(Event::device_updated(device)));
            }
            [device_id, "$nodes"] => {
                let nodes: Vec<_> = payload.split(",").collect();
                let device = get_mut_device_for(devices, "Got nodes for", device_id)?;
                // Remove nodes which aren't in the new list.
                device.nodes.retain(|k, _| nodes.contains(&k.as_ref()));
                // Add new nodes.
                for node_id in nodes {
                    if !device.nodes.contains_key(node_id) {
                        device.nodes.insert(node_id.to_owned(), Node::new(node_id));
                        let topic = format!("{}/{}/{}/+", self.base_topic, device_id, node_id);
                        self.mqtt_client.subscribe(topic, QoS::AtLeastOnce).await?;
                    }
                }
                return Ok(Some(Event::device_updated(device)));
            }
            [device_id, node_id, "$name"] => {
                let node = get_mut_node_for(devices, "Got node name for", device_id, node_id)?;
                node.name = Some(payload.to_owned());
                return Ok(Some(Event::node_updated(device_id, node)));
            }
            [device_id, node_id, "$type"] => {
                let node = get_mut_node_for(devices, "Got node type for", device_id, node_id)?;
                node.node_type = Some(payload.to_owned());
                return Ok(Some(Event::node_updated(device_id, node)));
            }
            [device_id, node_id, "$properties"] => {
                let properties: Vec<_> = payload.split(",").collect();
                let node = get_mut_node_for(devices, "Got properties for", device_id, node_id)?;
                // Remove properties which aren't in the new list.
                node.properties
                    .retain(|k, _| properties.contains(&k.as_ref()));
                // Add new properties.
                for property_id in properties {
                    if !node.properties.contains_key(property_id) {
                        node.properties
                            .insert(property_id.to_owned(), Property::new(property_id));
                        let topic = format!(
                            "{}/{}/{}/{}/+",
                            self.base_topic, device_id, node_id, property_id
                        );
                        self.mqtt_client.subscribe(topic, QoS::AtLeastOnce).await?;
                    }
                }
                return Ok(Some(Event::node_updated(device_id, node)));
            }
            [device_id, node_id, property_id, "$name"] => {
                let property = get_mut_property_for(
                    devices,
                    "Got property name for",
                    device_id,
                    node_id,
                    property_id,
                )?;
                property.name = Some(payload.to_owned());
                return Ok(Some(Event::property_updated(device_id, node_id, property)));
            }
            [device_id, node_id, property_id, "$datatype"] => {
                let datatype = payload.parse()?;
                let property = get_mut_property_for(
                    devices,
                    "Got property datatype for",
                    device_id,
                    node_id,
                    property_id,
                )?;
                property.datatype = Some(datatype);
                return Ok(Some(Event::property_updated(device_id, node_id, property)));
            }
            [device_id, node_id, property_id, "$unit"] => {
                let property = get_mut_property_for(
                    devices,
                    "Got property unit for",
                    device_id,
                    node_id,
                    property_id,
                )?;
                property.unit = Some(payload.to_owned());
                return Ok(Some(Event::property_updated(device_id, node_id, property)));
            }
            [device_id, node_id, property_id, "$format"] => {
                let property = get_mut_property_for(
                    devices,
                    "Got property format for",
                    device_id,
                    node_id,
                    property_id,
                )?;
                property.format = Some(payload.to_owned());
                return Ok(Some(Event::property_updated(device_id, node_id, property)));
            }
            [device_id, node_id, property_id, "$settable"] => {
                let settable = payload
                    .parse()
                    .map_err(|_| format!("Invalid boolean '{}' for $settable.", payload))?;
                let property = get_mut_property_for(
                    devices,
                    "Got property settable for",
                    device_id,
                    node_id,
                    property_id,
                )?;
                property.settable = settable;
                return Ok(Some(Event::property_updated(device_id, node_id, property)));
            }
            [device_id, node_id, property_id, "$retained"] => {
                let retained = payload
                    .parse()
                    .map_err(|_| format!("Invalid boolean '{}' for $retained.", payload))?;
                let property = get_mut_property_for(
                    devices,
                    "Got property retained for",
                    device_id,
                    node_id,
                    property_id,
                )?;
                property.retained = retained;
                return Ok(Some(Event::property_updated(device_id, node_id, property)));
            }
            [device_id, node_id, property_id] if !property_id.starts_with("$") => {
                // TODO: What about values of properties we don't yet know about? They may arrive
                // before the $properties of the node, because the "homie/node_id/+" subscription
                // matches both.
                let property = get_mut_property_for(
                    devices,
                    "Got property value for",
                    device_id,
                    node_id,
                    property_id,
                )?;
                property.value = Some(payload.to_owned());
                return Ok(Some(Event::property_value(device_id, node_id, property)));
            }
            _ => log::warn!("Unexpected subtopic {} = {}", subtopic, payload),
        }
        Ok(None)
    }

    async fn new_device(
        &self,
        devices: &mut HashMap<String, Device>,
        device_id: &str,
        homie_version: &str,
    ) -> Result<(), ClientError> {
        log::trace!("Homie device '{}' version '{}'", device_id, homie_version);
        devices.insert(device_id.to_owned(), Device::new(device_id, homie_version));
        let topic = format!("{}/{}/+", self.base_topic, device_id);
        log::trace!("Subscribe to {}", topic);
        self.mqtt_client.subscribe(topic, QoS::AtLeastOnce).await
    }

    /// Start discovering Homie devices.
    pub async fn start(&self) -> Result<(), ClientError> {
        let topic = format!("{}/+/$homie", self.base_topic);
        log::trace!("Subscribe to {}", topic);
        self.mqtt_client.subscribe(topic, QoS::AtLeastOnce).await
    }
}

fn get_mut_device_for<'a>(
    devices: &'a mut HashMap<String, Device>,
    err_prefix: &str,
    device_id: &str,
) -> Result<&'a mut Device, String> {
    devices
        .get_mut(device_id)
        .ok_or_else(|| format!("{} unknown device '{}'", err_prefix, device_id))
}

fn get_mut_node_for<'a>(
    devices: &'a mut HashMap<String, Device>,
    err_prefix: &str,
    device_id: &str,
    node_id: &str,
) -> Result<&'a mut Node, String> {
    let device = get_mut_device_for(devices, err_prefix, device_id)?;
    device
        .nodes
        .get_mut(node_id)
        .ok_or_else(|| format!("{} unknown node '{}/{}'", err_prefix, device_id, node_id))
}

fn get_mut_property_for<'a>(
    devices: &'a mut HashMap<String, Device>,
    err_prefix: &str,
    device_id: &str,
    node_id: &str,
    property_id: &str,
) -> Result<&'a mut Property, String> {
    let node = get_mut_node_for(devices, err_prefix, device_id, node_id)?;
    node.properties.get_mut(property_id).ok_or_else(|| {
        format!(
            "{} unknown property '{}/{}/{}'",
            err_prefix, device_id, node_id, property_id
        )
    })
}

#[derive(Error, Debug)]
enum HandleError {
    #[error("{0}")]
    Warning(String),
    #[error("{0}")]
    Fatal(#[from] ClientError),
}

impl From<String> for HandleError {
    fn from(s: String) -> Self {
        HandleError::Warning(s)
    }
}

impl From<ParseStateError> for HandleError {
    fn from(e: ParseStateError) -> Self {
        HandleError::Warning(e.to_string())
    }
}

impl From<ParseDatatypeError> for HandleError {
    fn from(e: ParseDatatypeError) -> Self {
        HandleError::Warning(e.to_string())
    }
}
