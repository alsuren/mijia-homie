//! `homie-controller` is a library for creating controllers to interact via an MQTT broker with IoT
//! devices implementing the [Homie convention](https://homieiot.github.io/).

use rumqttc::{
    AsyncClient, ClientError, ConnectionError, Event, EventLoop, Incoming, MqttOptions, Publish,
    QoS,
};
use std::collections::HashMap;
use std::str;
use thiserror::Error;
use tokio::task::JoinError;

mod types;
pub use types::{Datatype, DatatypeParseError, Device, Node, Property, State, StateParseError};

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

/// A Homie controller, which connects to an MQTT broker and interacts with Homie devices.
#[derive(Debug)]
pub struct HomieController {
    mqtt_client: AsyncClient,
    base_topic: String,
    pub devices: HashMap<String, Device>,
}

impl HomieController {
    pub fn new(mqtt_options: MqttOptions, base_topic: &str) -> (HomieController, EventLoop) {
        let (mqtt_client, event_loop) = AsyncClient::new(mqtt_options, REQUESTS_CAP);
        let controller = HomieController {
            mqtt_client,
            base_topic: base_topic.to_string(),
            devices: HashMap::new(),
        };
        (controller, event_loop)
    }

    /// Poll the EventLoop.
    pub async fn poll(&mut self, event_loop: &mut EventLoop) -> Result<(), PollError> {
        let notification = event_loop.poll().await?;
        log::trace!("Notification = {:?}", notification);

        if let Event::Incoming(incoming) = notification {
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
                        Ok(()) => {}
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    async fn handle_publish(&mut self, publish: Publish) -> Result<(), HandleError> {
        let base_topic = format!("{}/", self.base_topic);
        let payload = str::from_utf8(&publish.payload)
            .map_err(|e| format!("Payload not valid UTF-8: {}", e))?;
        let subtopic = publish
            .topic
            .strip_prefix(&base_topic)
            .ok_or_else(|| format!("Publish with unexpected topic: {:?}", publish))?;
        let parts = subtopic.split("/").collect::<Vec<&str>>();
        match parts.as_slice() {
            [device_id, "$homie"] => {
                if !self.devices.contains_key(*device_id) {
                    self.new_device(device_id, payload).await?;
                }
            }
            [device_id, "$name"] => {
                get_mut_device_for(&mut self.devices, "Got name for", device_id)?.name =
                    Some(payload.to_owned());
            }
            [device_id, "$state"] => {
                let state = payload.parse()?;
                get_mut_device_for(&mut self.devices, "Got state for", device_id)?.state = state;
            }
            [device_id, "$implementation"] => {
                get_mut_device_for(&mut self.devices, "Got implementation for", device_id)?
                    .implementation = Some(payload.to_owned());
            }
            [device_id, "$nodes"] => {
                let nodes: Vec<_> = payload.split(",").collect();
                let device = get_mut_device_for(&mut self.devices, "Got nodes for", device_id)?;
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
            }
            [device_id, node_id, "$name"] => {
                let node =
                    get_mut_node_for(&mut self.devices, "Got node name for", device_id, node_id)?;
                node.name = Some(payload.to_owned());
            }
            [device_id, node_id, "$type"] => {
                let node =
                    get_mut_node_for(&mut self.devices, "Got node type for", device_id, node_id)?;
                node.node_type = Some(payload.to_owned());
            }
            [device_id, node_id, "$properties"] => {
                let properties: Vec<_> = payload.split(",").collect();
                let node =
                    get_mut_node_for(&mut self.devices, "Got properties for", device_id, node_id)?;
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
            }
            [device_id, node_id, property_id, "$name"] => {
                let property = get_mut_property_for(
                    &mut self.devices,
                    "Got property name for",
                    device_id,
                    node_id,
                    property_id,
                )?;
                property.name = Some(payload.to_owned());
            }
            [device_id, node_id, property_id, "$datatype"] => {
                let datatype = payload.parse()?;
                let property = get_mut_property_for(
                    &mut self.devices,
                    "Got property datatype for",
                    device_id,
                    node_id,
                    property_id,
                )?;
                property.datatype = Some(datatype);
            }
            [device_id, node_id, property_id, "$unit"] => {
                let property = get_mut_property_for(
                    &mut self.devices,
                    "Got property unit for",
                    device_id,
                    node_id,
                    property_id,
                )?;
                property.unit = Some(payload.to_owned());
            }
            [device_id, node_id, property_id, "$format"] => {
                let property = get_mut_property_for(
                    &mut self.devices,
                    "Got property format for",
                    device_id,
                    node_id,
                    property_id,
                )?;
                property.format = Some(payload.to_owned());
            }
            [device_id, node_id, property_id, "$settable"] => {
                let settable = payload
                    .parse()
                    .map_err(|_| format!("Invalid boolean '{}' for $settable.", payload))?;
                let property = get_mut_property_for(
                    &mut self.devices,
                    "Got property settable for",
                    device_id,
                    node_id,
                    property_id,
                )?;
                property.settable = settable;
            }
            [device_id, node_id, property_id, "$retained"] => {
                let retained = payload
                    .parse()
                    .map_err(|_| format!("Invalid boolean '{}' for $retained.", payload))?;
                let property = get_mut_property_for(
                    &mut self.devices,
                    "Got property retained for",
                    device_id,
                    node_id,
                    property_id,
                )?;
                property.retained = retained;
            }
            [device_id, node_id, property_id] if !property_id.starts_with("$") => {
                let property = get_mut_property_for(
                    &mut self.devices,
                    "Got property value for",
                    device_id,
                    node_id,
                    property_id,
                )?;
                property.value = Some(payload.to_owned());
            }
            _ => log::warn!("Unexpected subtopic {} = {}", subtopic, payload),
        }
        Ok(())
    }

    async fn new_device(
        &mut self,
        device_id: &str,
        homie_version: &str,
    ) -> Result<(), ClientError> {
        log::trace!("Homie device '{}' version '{}'", device_id, homie_version);
        self.devices
            .insert(device_id.to_owned(), Device::new(device_id, homie_version));
        let topic = format!("{}/{}/+", self.base_topic, device_id);
        log::trace!("Subscribe to {}", topic);
        self.mqtt_client.subscribe(topic, QoS::AtLeastOnce).await
    }

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

impl From<StateParseError> for HandleError {
    fn from(e: StateParseError) -> Self {
        HandleError::Warning(e.to_string())
    }
}

impl From<DatatypeParseError> for HandleError {
    fn from(e: DatatypeParseError) -> Self {
        HandleError::Warning(e.to_string())
    }
}
