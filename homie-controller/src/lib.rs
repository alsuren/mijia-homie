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
pub use types::{Datatype, Node, State, StateParseError};

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

#[derive(Debug)]
pub struct Device {
    pub id: String,
    pub homie_version: String,
    pub name: Option<String>,
    pub state: State,
    pub nodes: HashMap<String, Node>,
}

impl Device {
    fn new(id: &str, homie_version: &str) -> Device {
        Device {
            id: id.to_owned(),
            homie_version: homie_version.to_owned(),
            name: None,
            state: State::Unknown,
            nodes: HashMap::new(),
        }
    }
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
                self.devices
                    .get_mut(*device_id)
                    .ok_or_else(|| format!("Got name for unknown device '{}'", device_id))?
                    .name = Some(payload.to_owned());
            }
            [device_id, "$state"] => {
                let state = payload.parse()?;
                self.devices
                    .get_mut(*device_id)
                    .ok_or_else(|| format!("Got state for unknown device '{}'", device_id))?
                    .state = state;
            }
            [device_id, "$nodes"] => {
                let nodes = payload.split(",");
                let device = self
                    .devices
                    .get_mut(*device_id)
                    .ok_or_else(|| format!("Got nodes for unknown device '{}'", device_id))?;
                for node_id in nodes {
                    if !device.nodes.contains_key(node_id) {
                        device.nodes.insert(node_id.to_owned(), Node::new(node_id));
                    }
                }
            }
            _ => log::warn!("Unexpected subtopic {}", subtopic),
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
