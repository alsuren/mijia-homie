//! `homie-controller` is a library for creating controllers to interact via an MQTT broker with IoT
//! devices implementing the [Homie convention](https://homieiot.github.io/).

use rumqttc::{
    AsyncClient, ClientError, ConnectionError, Event, EventLoop, Incoming, MqttOptions, Publish,
    QoS,
};
use std::str;
use thiserror::Error;
use tokio::task::JoinError;

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
}

impl HomieController {
    pub fn new(mqtt_options: MqttOptions, base_topic: &str) -> (HomieController, EventLoop) {
        let (mqtt_client, event_loop) = AsyncClient::new(mqtt_options, REQUESTS_CAP);
        let controller = HomieController {
            mqtt_client,
            base_topic: base_topic.to_string(),
        };
        (controller, event_loop)
    }

    /// Poll the EventLoop.
    pub async fn poll(&mut self, event_loop: &mut EventLoop) -> Result<(), PollError> {
        let base_topic = format!("{}/", self.base_topic);

        let notification = event_loop.poll().await?;
        log::trace!("Notification = {:?}", notification);

        if let Event::Incoming(incoming) = notification {
            log::trace!("Incoming: {:?}", incoming);
            match incoming {
                Incoming::Publish(publish) => {
                    match handle_publish(publish, &base_topic, &self.mqtt_client).await {
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

async fn handle_publish(
    publish: Publish,
    base_topic: &str,
    client: &AsyncClient,
) -> Result<(), HandleError> {
    let payload =
        str::from_utf8(&publish.payload).map_err(|e| format!("Payload not valid UTF-8: {}", e))?;
    let subtopic = publish
        .topic
        .strip_prefix(&base_topic)
        .ok_or_else(|| format!("Publish with unexpected topic: {:?}", publish))?;
    let parts = subtopic.split("/").collect::<Vec<&str>>();
    match parts.as_slice() {
        [device_id, "$homie"] => {
            log::trace!("Homie device '{}' version '{}'", device_id, payload);
            let topic = format!("{}{}/+", base_topic, device_id);
            log::trace!("Subscribe to {}", topic);
            client.subscribe(topic, QoS::AtLeastOnce).await?;
        }
        _ => log::warn!("Unexpected subtopic {}", subtopic),
    }
    Ok(())
}
