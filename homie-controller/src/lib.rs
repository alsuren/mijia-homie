//! `homie-controller` is a library for creating controllers to interact via an MQTT broker with IoT
//! devices implementing the [Homie convention](https://homieiot.github.io/).

use rumqttc::{
    AsyncClient, ClientError, ConnectionError, EventLoop, Incoming, MqttOptions, Publish, QoS,
};
use std::collections::HashMap;
use std::num::{ParseFloatError, ParseIntError};
use std::str;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;

mod types;
pub use types::{Datatype, Device, Extension, Node, Property, State};
use types::{ParseDatatypeError, ParseExtensionError, ParseStateError};

mod values;
pub use values::{
    ColorFormat, ColorHsv, ColorRgb, EnumValue, ParseColorError, ParseEnumError, Value, ValueError,
};

const REQUESTS_CAP: usize = 1000;

/// An error encountered while polling a `HomieController`.
#[derive(Error, Debug)]
pub enum PollError {
    /// Error sending to the MQTT broker.
    #[error("{0}")]
    Client(#[from] ClientError),
    /// Error connecting to or communicating with the MQTT broker.
    #[error("{0}")]
    Connection(#[from] ConnectionError),
}

/// An event from a Homie device, either because of a property change or because something new has
/// been discovered.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    /// A new device has been discovered, or an attribute of the device has been updated.
    DeviceUpdated {
        device_id: String,
        has_required_attributes: bool,
    },
    /// An attribute of a node on a device has been updated.
    NodeUpdated {
        device_id: String,
        node_id: String,
        has_required_attributes: bool,
    },
    /// An attribute of a property on a node has been updated.
    PropertyUpdated {
        device_id: String,
        node_id: String,
        property_id: String,
        has_required_attributes: bool,
    },
    /// The value of a property has changed.
    PropertyValueChanged {
        device_id: String,
        node_id: String,
        property_id: String,
        /// The new value.
        value: String,
        /// Whether the new value is fresh, i.e. it has just been sent by the device, as opposed to
        /// being the initial value because the controller just connected to the MQTT broker.
        fresh: bool,
    },
    /// Connected to the MQTT broker. This could be either the initial connection or a reconnection
    /// after the connection was dropped for some reason.
    Connected,
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

    fn property_value(device_id: &str, node_id: &str, property: &Property, fresh: bool) -> Self {
        Event::PropertyValueChanged {
            device_id: device_id.to_owned(),
            node_id: node_id.to_owned(),
            property_id: property.id.to_owned(),
            value: property.value.to_owned().unwrap(),
            fresh,
        }
    }
}

/// A Homie controller, which connects to an MQTT broker and interacts with Homie devices.
#[derive(Debug)]
pub struct HomieController {
    mqtt_client: AsyncClient,
    base_topic: String,
    /// The set of Homie devices which have been discovered so far, keyed by their IDs.
    // TODO: Consider using Mutex<im::HashMap<...>> instead.
    devices: Mutex<Arc<HashMap<String, Device>>>,
    /// temporarily holds retained property payloads that were received before their nodes'
    /// $properties. The stored payloads are consumed when $properties is received.
    early_property_values: Mutex<HashMap<String, String>>,
}

pub struct HomieEventLoop {
    event_loop: EventLoop,
}

impl HomieEventLoop {
    fn new(event_loop: EventLoop) -> HomieEventLoop {
        HomieEventLoop { event_loop }
    }
}

/// Internal struct for the return value of HomieController::handle_publish_sync()
struct PublishResponse {
    events: Vec<Event>,
    topics_to_subscribe: Vec<String>,
    topics_to_unsubscribe: Vec<String>,
}

impl HomieController {
    /// Create a new `HomieController` connected to an MQTT broker.
    ///
    /// # Arguments
    /// * `base_topic`: The Homie [base topic](https://homieiot.github.io/specification/#base-topic)
    ///   under which to look for Homie devices. "homie" is the recommended default.
    /// * `mqtt_options`: Options for the MQTT connection, including which broker to connect to.
    pub fn new(mqtt_options: MqttOptions, base_topic: &str) -> (HomieController, HomieEventLoop) {
        let (mqtt_client, event_loop) = AsyncClient::new(mqtt_options, REQUESTS_CAP);
        let controller = HomieController {
            mqtt_client,
            base_topic: base_topic.to_string(),
            devices: Mutex::new(Arc::new(HashMap::new())),
            early_property_values: Mutex::new(HashMap::new()),
        };
        (controller, HomieEventLoop::new(event_loop))
    }

    /// Get a snapshot of the set of Homie devices which have been discovered so far, keyed by their
    /// IDs.
    pub fn devices(&self) -> Arc<HashMap<String, Device>> {
        self.devices.lock().unwrap().clone()
    }

    /// Get the Homie base topic which the controller was configured to use.
    pub fn base_topic(&self) -> &str {
        &self.base_topic
    }

    /// Poll the `EventLoop`, and maybe return a Homie event.
    pub async fn poll(&self, event_loop: &mut HomieEventLoop) -> Result<Vec<Event>, PollError> {
        let notification = event_loop.event_loop.poll().await?;
        log::trace!("Notification = {:?}", notification);

        if let rumqttc::Event::Incoming(incoming) = notification {
            self.handle_event(incoming).await
        } else {
            Ok(vec![])
        }
    }

    async fn handle_event(&self, incoming: Incoming) -> Result<Vec<Event>, PollError> {
        match incoming {
            Incoming::Publish(publish) => match self.handle_publish(publish).await {
                Err(HandleError::Warning(err)) => {
                    // These error strings indicate some issue with parsing the publish
                    // event from the network, perhaps due to a malfunctioning device,
                    // so should just be logged and ignored.
                    log::warn!("{}", err);
                    Ok(vec![])
                }
                Err(HandleError::Fatal(e)) => Err(e.into()),
                Ok(events) => Ok(events),
            },
            Incoming::ConnAck(_) => {
                // We have connected or reconnected, so make our initial subscription to start
                // discovering Homie devices.
                self.start().await?;
                Ok(vec![Event::Connected])
            }
            _ => Ok(vec![]),
        }
    }

    /// Handle a publish event received from the MQTT broker, updating the devices and our
    /// subscriptions as appropriate and possibly returning an event to send back to the controller
    /// application.
    async fn handle_publish(&self, publish: Publish) -> Result<Vec<Event>, HandleError> {
        let PublishResponse {
            events,
            topics_to_subscribe,
            topics_to_unsubscribe,
        } = self.handle_publish_sync(publish)?;

        for topic in topics_to_subscribe {
            log::trace!("Subscribe to {}", topic);
            self.mqtt_client.subscribe(topic, QoS::AtLeastOnce).await?;
        }
        for topic in topics_to_unsubscribe {
            log::trace!("Unsubscribe from {}", topic);
            self.mqtt_client.unsubscribe(topic).await?;
        }

        Ok(events)
    }

    /// Handle a publish event, update the devices, and return any event and any new topics which
    /// should be subscribed to or unsubscribed from.
    ///
    /// This is separate from `handle_publish` because it takes the `devices` lock, to ensure that
    /// no async operations are awaited while the lock is held.
    fn handle_publish_sync(&self, publish: Publish) -> Result<PublishResponse, HandleError> {
        let base_topic = format!("{}/", self.base_topic);
        let payload = str::from_utf8(&publish.payload)
            .map_err(|e| format!("Payload not valid UTF-8: {}", e))?;
        let subtopic = publish
            .topic
            .strip_prefix(&base_topic)
            .ok_or_else(|| format!("Publish with unexpected topic: {:?}", publish))?;

        // If there are no other references to the devices this will give us a mutable reference
        // directly. If there are other references it will clone the underlying HashMap and update
        // our Arc to point to that, so that it is now a unique reference.
        let devices = &mut *self.devices.lock().unwrap();
        let devices = Arc::make_mut(devices);

        let early_property_values = &mut *self.early_property_values.lock().unwrap();

        // Collect MQTT topics to which we need to subscribe or unsubscribe here, so that the
        // subscription can happen after the devices lock has been released.
        let mut topics_to_subscribe: Vec<String> = vec![];
        let mut topics_to_unsubscribe: Vec<String> = vec![];

        let parts = subtopic.split('/').collect::<Vec<&str>>();
        let events = match parts.as_slice() {
            [device_id, "$homie"] => {
                if !devices.contains_key(*device_id) {
                    log::trace!("Homie device '{}' version '{}'", device_id, payload);
                    devices.insert((*device_id).to_owned(), Device::new(device_id, payload));
                    topics_to_subscribe.push(format!("{}/{}/+", self.base_topic, device_id));
                    topics_to_subscribe.push(format!("{}/{}/$fw/+", self.base_topic, device_id));
                    topics_to_subscribe.push(format!("{}/{}/$stats/+", self.base_topic, device_id));
                    vec![Event::DeviceUpdated {
                        device_id: (*device_id).to_owned(),
                        has_required_attributes: false,
                    }]
                } else {
                    vec![]
                }
            }
            [device_id, "$name"] => {
                let device = get_mut_device_for(devices, "Got name for", device_id)?;
                device.name = Some(payload.to_owned());
                vec![Event::device_updated(device)]
            }
            [device_id, "$state"] => {
                let state = payload.parse()?;
                let device = get_mut_device_for(devices, "Got state for", device_id)?;
                device.state = state;
                vec![Event::device_updated(device)]
            }
            [device_id, "$implementation"] => {
                let device = get_mut_device_for(devices, "Got implementation for", device_id)?;
                device.implementation = Some(payload.to_owned());
                vec![Event::device_updated(device)]
            }
            [device_id, "$extensions"] => {
                let device = get_mut_device_for(devices, "Got extensions for", device_id)?;
                device.extensions = payload
                    .split(',')
                    .map(|part| part.parse())
                    .collect::<Result<Vec<_>, _>>()?;
                vec![Event::device_updated(device)]
            }
            [device_id, "$localip"] => {
                let device = get_mut_device_for(devices, "Got localip for", device_id)?;
                device.local_ip = Some(payload.to_owned());
                vec![Event::device_updated(device)]
            }
            [device_id, "$mac"] => {
                let device = get_mut_device_for(devices, "Got mac for", device_id)?;
                device.mac = Some(payload.to_owned());
                vec![Event::device_updated(device)]
            }
            [device_id, "$fw", "name"] => {
                let device = get_mut_device_for(devices, "Got fw/name for", device_id)?;
                device.firmware_name = Some(payload.to_owned());
                vec![Event::device_updated(device)]
            }
            [device_id, "$fw", "version"] => {
                let device = get_mut_device_for(devices, "Got fw/version for", device_id)?;
                device.firmware_version = Some(payload.to_owned());
                vec![Event::device_updated(device)]
            }
            [_device_id, "$stats"] => {
                // Homie 3.0 list of available stats. We don't need this, so ignore it without
                // logging a warning.
                vec![]
            }
            [device_id, "$stats", "interval"] => {
                let interval = payload.parse()?;
                let device = get_mut_device_for(devices, "Got stats/interval for", device_id)?;
                device.stats_interval = Some(Duration::from_secs(interval));
                vec![Event::device_updated(device)]
            }
            [device_id, "$stats", "uptime"] => {
                let uptime = payload.parse()?;
                let device = get_mut_device_for(devices, "Got stats/uptime for", device_id)?;
                device.stats_uptime = Some(Duration::from_secs(uptime));
                vec![Event::device_updated(device)]
            }
            [device_id, "$stats", "signal"] => {
                let signal = payload.parse()?;
                let device = get_mut_device_for(devices, "Got stats/signal for", device_id)?;
                device.stats_signal = Some(signal);
                vec![Event::device_updated(device)]
            }
            [device_id, "$stats", "cputemp"] => {
                let cputemp = payload.parse()?;
                let device = get_mut_device_for(devices, "Got stats/cputemp for", device_id)?;
                device.stats_cputemp = Some(cputemp);
                vec![Event::device_updated(device)]
            }
            [device_id, "$stats", "cpuload"] => {
                let cpuload = payload.parse()?;
                let device = get_mut_device_for(devices, "Got stats/cpuload for", device_id)?;
                device.stats_cpuload = Some(cpuload);
                vec![Event::device_updated(device)]
            }
            [device_id, "$stats", "battery"] => {
                let battery = payload.parse()?;
                let device = get_mut_device_for(devices, "Got stats/battery for", device_id)?;
                device.stats_battery = Some(battery);
                vec![Event::device_updated(device)]
            }
            [device_id, "$stats", "freeheap"] => {
                let freeheap = payload.parse()?;
                let device = get_mut_device_for(devices, "Got stats/freeheap for", device_id)?;
                device.stats_freeheap = Some(freeheap);
                vec![Event::device_updated(device)]
            }
            [device_id, "$stats", "supply"] => {
                let supply = payload.parse()?;
                let device = get_mut_device_for(devices, "Got stats/supply for", device_id)?;
                device.stats_supply = Some(supply);
                vec![Event::device_updated(device)]
            }
            [device_id, "$nodes"] => {
                let nodes: Vec<_> = payload.split(',').collect();
                let device = get_mut_device_for(devices, "Got nodes for", device_id)?;

                // Remove nodes which aren't in the new list.
                device.nodes.retain(|node_id, node| {
                    let kept = nodes.contains(&node_id.as_ref());
                    if !kept {
                        // The node has been removed, so unsubscribe from its topics and those of its properties
                        let node_topic = format!("{}/{}/{}/+", self.base_topic, device_id, node_id);
                        topics_to_unsubscribe.push(node_topic);
                        for property_id in node.properties.keys() {
                            let topic = format!(
                                "{}/{}/{}/{}/+",
                                self.base_topic, device_id, node_id, property_id
                            );
                            topics_to_unsubscribe.push(topic);
                        }
                    }
                    kept
                });

                // Add new nodes.
                for node_id in nodes {
                    if !device.nodes.contains_key(node_id) {
                        device.add_node(Node::new(node_id));
                        let topic = format!("{}/{}/{}/+", self.base_topic, device_id, node_id);
                        topics_to_subscribe.push(topic);
                    }
                }

                vec![Event::device_updated(device)]
            }
            [device_id, node_id, "$name"] => {
                let node = get_mut_node_for(devices, "Got node name for", device_id, node_id)?;
                node.name = Some(payload.to_owned());
                vec![Event::node_updated(device_id, node)]
            }
            [device_id, node_id, "$type"] => {
                let node = get_mut_node_for(devices, "Got node type for", device_id, node_id)?;
                node.node_type = Some(payload.to_owned());
                vec![Event::node_updated(device_id, node)]
            }
            [device_id, node_id, "$properties"] => {
                let properties: Vec<_> = payload.split(',').collect();
                let node = get_mut_node_for(devices, "Got properties for", device_id, node_id)?;

                // Remove properties which aren't in the new list.
                node.properties.retain(|property_id, _| {
                    let kept = properties.contains(&property_id.as_ref());
                    if !kept {
                        // The property has been removed, so unsubscribe from its topics.
                        let topic = format!(
                            "{}/{}/{}/{}/+",
                            self.base_topic, device_id, node_id, property_id
                        );
                        topics_to_unsubscribe.push(topic);
                    }
                    kept
                });

                let mut events = vec![Event::node_updated(device_id, node)];

                // Add new properties.
                for property_id in properties {
                    if !node.properties.contains_key(property_id) {
                        let mut new_prop = Property::new(property_id);

                        let key = format!("{}/{}/{}", device_id, node_id, property_id);
                        new_prop.value = early_property_values.remove(&key);

                        if let Some(value) = new_prop.value.clone() {
                            events.push(Event::PropertyValueChanged {
                                device_id: device_id.to_string(),
                                node_id: node_id.to_string(),
                                property_id: property_id.to_string(),
                                value,
                                fresh: false,
                            });
                        }

                        node.add_property(new_prop);
                        let topic = format!(
                            "{}/{}/{}/{}/+",
                            self.base_topic, device_id, node_id, property_id
                        );
                        topics_to_subscribe.push(topic);
                    }
                }

                events
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
                vec![Event::property_updated(device_id, node_id, property)]
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
                vec![Event::property_updated(device_id, node_id, property)]
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
                vec![Event::property_updated(device_id, node_id, property)]
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
                vec![Event::property_updated(device_id, node_id, property)]
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
                vec![Event::property_updated(device_id, node_id, property)]
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
                vec![Event::property_updated(device_id, node_id, property)]
            }
            [device_id, node_id, property_id]
                if !device_id.starts_with('$')
                    && !node_id.starts_with('$')
                    && !property_id.starts_with('$') =>
            {
                match get_mut_property_for(
                    devices,
                    "Got property value for",
                    device_id,
                    node_id,
                    property_id,
                ) {
                    Ok(mut property) => {
                        property.value = Some(payload.to_owned());
                        vec![Event::property_value(
                            device_id,
                            node_id,
                            property,
                            !publish.retain,
                        )]
                    }

                    Err(_) if publish.retain => {
                        // temporarily store payloads for unknown properties to prevent
                        // a race condition when the broker sends out the property
                        // payloads before $properties
                        early_property_values.insert(subtopic.to_owned(), payload.to_owned());

                        vec![]
                    }

                    Err(e) => return Err(e.into()),
                }
            }
            [_device_id, _node_id, _property_id, "set"] => {
                // Value set message may have been sent by us or another controller. Either way,
                // ignore it, it is only for the device.
                vec![]
            }
            _ => {
                log::warn!("Unexpected subtopic {} = {}", subtopic, payload);
                vec![]
            }
        };

        Ok(PublishResponse {
            events,
            topics_to_subscribe,
            topics_to_unsubscribe,
        })
    }

    /// Start discovering Homie devices.
    async fn start(&self) -> Result<(), ClientError> {
        // Clear set of known devices so that we correctly subscribe to their topics again.
        *self.devices.lock().unwrap() = Arc::new(HashMap::new());

        let topic = format!("{}/+/$homie", self.base_topic);
        log::trace!("Subscribe to {}", topic);
        self.mqtt_client.subscribe(topic, QoS::AtLeastOnce).await
    }

    /// Attempt to set the state of a settable property of a device. If this succeeds the device
    /// will update the value of the property.
    pub async fn set(
        &self,
        device_id: &str,
        node_id: &str,
        property_id: &str,
        value: impl Value,
    ) -> Result<(), ClientError> {
        let topic = format!(
            "{}/{}/{}/{}/set",
            self.base_topic, device_id, node_id, property_id
        );
        self.mqtt_client
            .publish(topic, QoS::AtLeastOnce, false, value.to_string())
            .await
    }

    /// Disconnect from the MQTT broker.
    pub async fn disconnect(&self) -> Result<(), ClientError> {
        self.mqtt_client.disconnect().await
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

impl From<ParseExtensionError> for HandleError {
    fn from(e: ParseExtensionError) -> Self {
        HandleError::Warning(e.to_string())
    }
}

impl From<ParseIntError> for HandleError {
    fn from(e: ParseIntError) -> Self {
        HandleError::Warning(format!("Invalid integer: {}", e))
    }
}

impl From<ParseFloatError> for HandleError {
    fn from(e: ParseFloatError) -> Self {
        HandleError::Warning(format!("Invalid float: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flume::Receiver;
    use rumqttc::{ConnAck, Packet, Request, Subscribe};

    fn make_test_controller() -> (HomieController, Receiver<Request>) {
        let (requests_tx, requests_rx) = flume::unbounded();
        let mqtt_client = AsyncClient::from_senders(requests_tx);
        let controller = HomieController {
            base_topic: "base_topic".to_owned(),
            mqtt_client,
            devices: Mutex::new(Arc::new(HashMap::new())),
            early_property_values: Mutex::new(HashMap::new()),
        };
        (controller, requests_rx)
    }

    fn expect_subscriptions(requests_rx: &Receiver<Request>, subscription_topics: &[&str]) {
        let requests: Vec<_> = subscription_topics
            .iter()
            .map(|_| requests_rx.try_recv().unwrap())
            .collect();

        for topic in subscription_topics {
            let expected = Request::Subscribe(Subscribe::new(*topic, QoS::AtLeastOnce));
            assert!(requests.contains(&expected));
        }
    }

    async fn connect(controller: &HomieController) -> Result<Vec<Event>, PollError> {
        controller
            .handle_event(Packet::ConnAck(ConnAck::new(
                rumqttc::ConnectReturnCode::Success,
                false,
            )))
            .await
    }

    async fn publish(
        controller: &HomieController,
        topic: &str,
        payload: &str,
    ) -> Result<Vec<Event>, PollError> {
        controller
            .handle_event(Packet::Publish(Publish::new(
                topic,
                QoS::AtLeastOnce,
                payload,
            )))
            .await
    }

    async fn publish_retained(
        controller: &HomieController,
        topic: &str,
        payload: &str,
    ) -> Result<Vec<Event>, PollError> {
        let mut publish = Publish::new(topic, QoS::AtLeastOnce, payload);

        publish.retain = true;

        controller.handle_event(Packet::Publish(publish)).await
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

    #[tokio::test]
    async fn subscribes_to_things() -> Result<(), Box<dyn std::error::Error>> {
        let (controller, requests_rx) = make_test_controller();

        // Connecting should start discovering.
        connect(&controller).await?;
        expect_subscriptions(&requests_rx, &["base_topic/+/$homie"]);

        // Discover a new device.
        publish(&controller, "base_topic/device_id/$homie", "4.0").await?;
        expect_subscriptions(
            &requests_rx,
            &[
                "base_topic/device_id/+",
                "base_topic/device_id/$fw/+",
                "base_topic/device_id/$stats/+",
            ],
        );

        // Discover a node on the device.
        publish(&controller, "base_topic/device_id/$nodes", "node_id").await?;
        expect_subscriptions(&requests_rx, &["base_topic/device_id/node_id/+"]);

        // Discover a property on the node.
        publish(
            &controller,
            "base_topic/device_id/node_id/$properties",
            "property_id",
        )
        .await?;
        expect_subscriptions(
            &requests_rx,
            &["base_topic/device_id/node_id/property_id/+"],
        );

        // No more subscriptions.
        assert!(requests_rx.is_empty());

        Ok(())
    }

    #[tokio::test]
    async fn retained_payloads_before_properties() -> Result<(), Box<dyn std::error::Error>> {
        let (controller, _requests_rx) = make_test_controller();

        // Connecting should start discovering.
        connect(&controller).await?;

        // Discover a new device.
        publish_retained(&controller, "base_topic/device_id/$homie", "4.0").await?;

        // Discover a node on the device.
        publish_retained(
            &controller,
            "base_topic/device_id/$nodes",
            "node_id,second_node",
        )
        .await?;

        // Get the property payload before $properties
        publish_retained(
            &controller,
            "base_topic/device_id/node_id/property_id",
            "HELLO WORLD",
        )
        .await?;

        // discover the property after its payload
        publish_retained(
            &controller,
            "base_topic/device_id/node_id/$properties",
            "property_id",
        )
        .await?;

        publish_retained(
            &controller,
            "base_topic/device_id/second_node/property_id",
            "hello again",
        )
        .await?;

        // discover the property after its payload
        publish_retained(
            &controller,
            "base_topic/device_id/second_node/$properties",
            "property_id",
        )
        .await?;

        assert_eq!(
            controller
                .devices()
                .get("device_id")
                .unwrap()
                .nodes
                .get("node_id")
                .unwrap()
                .properties
                .get("property_id")
                .unwrap()
                .value
                .as_deref(),
            Some("HELLO WORLD")
        );

        assert_eq!(
            controller
                .devices()
                .get("device_id")
                .unwrap()
                .nodes
                .get("second_node")
                .unwrap()
                .properties
                .get("property_id")
                .unwrap()
                .value
                .as_deref(),
            Some("hello again")
        );

        Ok(())
    }

    #[tokio::test]
    async fn emits_appropriate_events() -> Result<(), Box<dyn std::error::Error>> {
        let (controller, _requests_rx) = make_test_controller();

        // Start discovering.
        assert_eq!(connect(&controller).await?, vec![Event::Connected]);

        // Discover a new device.
        assert_eq!(
            publish(&controller, "base_topic/device_id/$homie", "4.0").await?,
            vec![Event::DeviceUpdated {
                device_id: "device_id".to_owned(),
                has_required_attributes: false
            }]
        );
        assert_eq!(
            publish(&controller, "base_topic/device_id/$name", "Device name").await?,
            vec![Event::DeviceUpdated {
                device_id: "device_id".to_owned(),
                has_required_attributes: false
            }]
        );
        assert_eq!(
            publish(&controller, "base_topic/device_id/$state", "ready").await?,
            vec![Event::DeviceUpdated {
                device_id: "device_id".to_owned(),
                has_required_attributes: true
            }]
        );
        let mut expected_device = Device::new("device_id", "4.0");
        expected_device.state = State::Ready;
        expected_device.name = Some("Device name".to_owned());
        assert_eq!(
            controller.devices().get("device_id").unwrap().to_owned(),
            expected_device
        );

        // A node on the device.
        assert_eq!(
            publish(&controller, "base_topic/device_id/$nodes", "node_id").await?,
            vec![Event::DeviceUpdated {
                device_id: "device_id".to_owned(),
                has_required_attributes: false
            }]
        );
        assert_eq!(
            publish(
                &controller,
                "base_topic/device_id/node_id/$name",
                "Node name"
            )
            .await?,
            vec![Event::NodeUpdated {
                device_id: "device_id".to_owned(),
                node_id: "node_id".to_owned(),
                has_required_attributes: false
            }]
        );
        assert_eq!(
            publish(
                &controller,
                "base_topic/device_id/node_id/$type",
                "Node type"
            )
            .await?,
            vec![Event::NodeUpdated {
                device_id: "device_id".to_owned(),
                node_id: "node_id".to_owned(),
                has_required_attributes: false
            }]
        );

        // A property on the node.
        assert_eq!(
            publish(
                &controller,
                "base_topic/device_id/node_id/$properties",
                "property_id"
            )
            .await?,
            vec![Event::NodeUpdated {
                device_id: "device_id".to_owned(),
                node_id: "node_id".to_owned(),
                has_required_attributes: false
            }]
        );
        assert_eq!(
            publish(
                &controller,
                "base_topic/device_id/node_id/property_id/$name",
                "Property name"
            )
            .await?,
            vec![Event::PropertyUpdated {
                device_id: "device_id".to_owned(),
                node_id: "node_id".to_owned(),
                property_id: "property_id".to_owned(),
                has_required_attributes: false
            }]
        );
        assert_eq!(
            publish(
                &controller,
                "base_topic/device_id/node_id/property_id/$datatype",
                "integer"
            )
            .await?,
            vec![Event::PropertyUpdated {
                device_id: "device_id".to_owned(),
                node_id: "node_id".to_owned(),
                property_id: "property_id".to_owned(),
                has_required_attributes: true
            }]
        );

        Ok(())
    }

    #[tokio::test]
    async fn constructs_device_tree() -> Result<(), Box<dyn std::error::Error>> {
        let (controller, _requests_rx) = make_test_controller();

        // Discover a new device with a node with a property.

        connect(&controller).await?;
        publish(&controller, "base_topic/device_id/$homie", "4.0").await?;
        publish(&controller, "base_topic/device_id/$name", "Device name").await?;
        publish(&controller, "base_topic/device_id/$state", "ready").await?;
        publish(&controller, "base_topic/device_id/$nodes", "node_id").await?;

        publish(
            &controller,
            "base_topic/device_id/node_id/$name",
            "Node name",
        )
        .await?;
        publish(
            &controller,
            "base_topic/device_id/node_id/$type",
            "Node type",
        )
        .await?;
        publish(
            &controller,
            "base_topic/device_id/node_id/$properties",
            "property_id",
        )
        .await?;

        publish(
            &controller,
            "base_topic/device_id/node_id/property_id/$name",
            "Property name",
        )
        .await?;
        publish(
            &controller,
            "base_topic/device_id/node_id/property_id/$datatype",
            "integer",
        )
        .await?;

        let expected_property = Property {
            name: Some("Property name".to_owned()),
            datatype: Some(Datatype::Integer),
            ..Property::new("property_id")
        };
        let expected_node = Node {
            name: Some("Node name".to_owned()),
            node_type: Some("Node type".to_owned()),
            properties: property_set(vec![expected_property]),
            ..Node::new("node_id")
        };
        let expected_device = Device {
            name: Some("Device name".to_owned()),
            state: State::Ready,
            nodes: node_set(vec![expected_node]),
            ..Device::new("device_id", "4.0")
        };

        assert_eq!(
            controller.devices().get("device_id").unwrap().to_owned(),
            expected_device
        );

        Ok(())
    }
}
