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
    ColorFormat, ColorHSV, ColorRGB, EnumValue, ParseColorError, ParseEnumError, Value,
};

const REQUESTS_CAP: usize = 1000;

/// Error type for futures representing tasks spawned by this crate.
#[derive(Error, Debug)]
pub enum PollError {
    #[error("{0}")]
    Client(#[from] ClientError),
    #[error("{0}")]
    Connection(#[from] ConnectionError),
    #[error("Internal error: {0}")]
    Internal(&'static str),
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
    // TODO: Consider using Mutex<im::HashMap<...>> instead.
    devices: Mutex<Arc<HashMap<String, Device>>>,
}

pub struct HomieEventLoop {
    event_loop: EventLoop,
}

impl HomieEventLoop {
    fn new(event_loop: EventLoop) -> HomieEventLoop {
        HomieEventLoop { event_loop }
    }
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
        };
        (controller, HomieEventLoop::new(event_loop))
    }

    /// Get a snapshot of the set of Homie devices which have been discovered so far, keyed by their
    /// IDs.
    pub fn devices(&self) -> Arc<HashMap<String, Device>> {
        self.devices.lock().unwrap().clone()
    }

    /// Poll the `EventLoop`, and maybe return a Homie event.
    pub async fn poll(&self, event_loop: &mut HomieEventLoop) -> Result<Option<Event>, PollError> {
        let notification = event_loop.event_loop.poll().await?;
        log::trace!("Notification = {:?}", notification);

        if let rumqttc::Event::Incoming(incoming) = notification {
            self.handle_event(incoming).await
        } else {
            Ok(None)
        }
    }

    async fn handle_event(&self, incoming: Incoming) -> Result<Option<Event>, PollError> {
        log::trace!("Incoming: {:?}", incoming);
        if let Incoming::Publish(publish) = incoming {
            match self.handle_publish(publish).await {
                Err(HandleError::Warning(err)) => {
                    // These error strings indicate some issue with parsing the publish
                    // event from the network, perhaps due to a malfunctioning device,
                    // so should just be logged and ignored.
                    log::warn!("{}", err)
                }
                Err(HandleError::Fatal(e)) => return Err(e.into()),
                Ok(event) => return Ok(event),
            }
        }
        Ok(None)
    }

    /// Handle a publish event received from the MQTT broker, updating the devices and our
    /// subscriptions as appropriate and possibly returning an event to send back to the controller
    /// application.
    async fn handle_publish(&self, publish: Publish) -> Result<Option<Event>, HandleError> {
        let (event, topics_to_subscribe, topics_to_unsubscribe) =
            self.handle_publish_sync(publish)?;

        for topic in topics_to_subscribe {
            log::trace!("Subscribe to {}", topic);
            self.mqtt_client.subscribe(topic, QoS::AtLeastOnce).await?;
        }
        for topic in topics_to_unsubscribe {
            log::trace!("Unsubscribe from {}", topic);
            self.mqtt_client.unsubscribe(topic).await?;
        }

        Ok(event)
    }

    /// Handle a publish event, update the devices, and return any event and any new topics which
    /// should be subscribed to or unsubscribed from.
    ///
    /// This is separate from `handle_publish` because it takes the `devices` lock, to ensure that
    /// no async operations are awaited while the lock is held.
    fn handle_publish_sync(
        &self,
        publish: Publish,
    ) -> Result<(Option<Event>, Vec<String>, Vec<String>), HandleError> {
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

        // Collect MQTT topics to which we need to subscribe or unsubscribe here, so that the
        // subscription can happen after the devices lock has been released.
        let mut topics_to_subscribe: Vec<String> = vec![];
        let mut topics_to_unsubscribe: Vec<String> = vec![];

        let parts = subtopic.split('/').collect::<Vec<&str>>();
        let event = match parts.as_slice() {
            [device_id, "$homie"] => {
                if !devices.contains_key(*device_id) {
                    log::trace!("Homie device '{}' version '{}'", device_id, payload);
                    devices.insert((*device_id).to_owned(), Device::new(device_id, payload));
                    topics_to_subscribe.push(format!("{}/{}/+", self.base_topic, device_id));
                    topics_to_subscribe.push(format!("{}/{}/$fw/+", self.base_topic, device_id));
                    topics_to_subscribe.push(format!("{}/{}/$stats/+", self.base_topic, device_id));
                    Some(Event::DeviceUpdated {
                        device_id: (*device_id).to_owned(),
                        has_required_attributes: false,
                    })
                } else {
                    None
                }
            }
            [device_id, "$name"] => {
                let device = get_mut_device_for(devices, "Got name for", device_id)?;
                device.name = Some(payload.to_owned());
                Some(Event::device_updated(device))
            }
            [device_id, "$state"] => {
                let state = payload.parse()?;
                let device = get_mut_device_for(devices, "Got state for", device_id)?;
                device.state = state;
                Some(Event::device_updated(device))
            }
            [device_id, "$implementation"] => {
                let device = get_mut_device_for(devices, "Got implementation for", device_id)?;
                device.implementation = Some(payload.to_owned());
                Some(Event::device_updated(device))
            }
            [device_id, "$extensions"] => {
                let device = get_mut_device_for(devices, "Got extensions for", device_id)?;
                device.extensions = payload
                    .split(',')
                    .map(|part| part.parse())
                    .collect::<Result<Vec<_>, _>>()?;
                Some(Event::device_updated(device))
            }
            [device_id, "$localip"] => {
                let device = get_mut_device_for(devices, "Got localip for", device_id)?;
                device.local_ip = Some(payload.to_owned());
                Some(Event::device_updated(device))
            }
            [device_id, "$mac"] => {
                let device = get_mut_device_for(devices, "Got mac for", device_id)?;
                device.mac = Some(payload.to_owned());
                Some(Event::device_updated(device))
            }
            [device_id, "$fw", "name"] => {
                let device = get_mut_device_for(devices, "Got fw/name for", device_id)?;
                device.firmware_name = Some(payload.to_owned());
                Some(Event::device_updated(device))
            }
            [device_id, "$fw", "version"] => {
                let device = get_mut_device_for(devices, "Got fw/version for", device_id)?;
                device.firmware_version = Some(payload.to_owned());
                Some(Event::device_updated(device))
            }
            [device_id, "$stats", "interval"] => {
                let interval = payload.parse()?;
                let device = get_mut_device_for(devices, "Got stats/interval for", device_id)?;
                device.stats_interval = Some(Duration::from_secs(interval));
                Some(Event::device_updated(device))
            }
            [device_id, "$stats", "uptime"] => {
                let uptime = payload.parse()?;
                let device = get_mut_device_for(devices, "Got stats/uptime for", device_id)?;
                device.stats_uptime = Some(Duration::from_secs(uptime));
                Some(Event::device_updated(device))
            }
            [device_id, "$stats", "signal"] => {
                let signal = payload.parse()?;
                let device = get_mut_device_for(devices, "Got stats/signal for", device_id)?;
                device.stats_signal = Some(signal);
                Some(Event::device_updated(device))
            }
            [device_id, "$stats", "cputemp"] => {
                let cputemp = payload.parse()?;
                let device = get_mut_device_for(devices, "Got stats/cputemp for", device_id)?;
                device.stats_cputemp = Some(cputemp);
                Some(Event::device_updated(device))
            }
            [device_id, "$stats", "cpuload"] => {
                let cpuload = payload.parse()?;
                let device = get_mut_device_for(devices, "Got stats/cpuload for", device_id)?;
                device.stats_cpuload = Some(cpuload);
                Some(Event::device_updated(device))
            }
            [device_id, "$stats", "battery"] => {
                let battery = payload.parse()?;
                let device = get_mut_device_for(devices, "Got stats/battery for", device_id)?;
                device.stats_battery = Some(battery);
                Some(Event::device_updated(device))
            }
            [device_id, "$stats", "freeheap"] => {
                let freeheap = payload.parse()?;
                let device = get_mut_device_for(devices, "Got stats/freeheap for", device_id)?;
                device.stats_freeheap = Some(freeheap);
                Some(Event::device_updated(device))
            }
            [device_id, "$stats", "supply"] => {
                let supply = payload.parse()?;
                let device = get_mut_device_for(devices, "Got stats/supply for", device_id)?;
                device.stats_supply = Some(supply);
                Some(Event::device_updated(device))
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

                Some(Event::device_updated(device))
            }
            [device_id, node_id, "$name"] => {
                let node = get_mut_node_for(devices, "Got node name for", device_id, node_id)?;
                node.name = Some(payload.to_owned());
                Some(Event::node_updated(device_id, node))
            }
            [device_id, node_id, "$type"] => {
                let node = get_mut_node_for(devices, "Got node type for", device_id, node_id)?;
                node.node_type = Some(payload.to_owned());
                Some(Event::node_updated(device_id, node))
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

                // Add new properties.
                for property_id in properties {
                    if !node.properties.contains_key(property_id) {
                        node.add_property(Property::new(property_id));
                        let topic = format!(
                            "{}/{}/{}/{}/+",
                            self.base_topic, device_id, node_id, property_id
                        );
                        topics_to_subscribe.push(topic);
                    }
                }

                Some(Event::node_updated(device_id, node))
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
                Some(Event::property_updated(device_id, node_id, property))
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
                Some(Event::property_updated(device_id, node_id, property))
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
                Some(Event::property_updated(device_id, node_id, property))
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
                Some(Event::property_updated(device_id, node_id, property))
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
                Some(Event::property_updated(device_id, node_id, property))
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
                Some(Event::property_updated(device_id, node_id, property))
            }
            [device_id, node_id, property_id] if !property_id.starts_with('$') => {
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
                Some(Event::property_value(device_id, node_id, property))
            }
            [_device_id, _node_id, _property_id, "set"] => {
                // Value set message may have been sent by us or another controller. Either way,
                // ignore it, it is only for the device.
                None
            }
            _ => {
                log::warn!("Unexpected subtopic {} = {}", subtopic, payload);
                None
            }
        };

        Ok((event, topics_to_subscribe, topics_to_unsubscribe))
    }

    /// Start discovering Homie devices.
    pub async fn start(&self) -> Result<(), ClientError> {
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
    use async_channel::Receiver;
    use rumqttc::{Packet, Request, Subscribe};

    fn make_test_controller() -> (HomieController, Receiver<Request>) {
        let (requests_tx, requests_rx) = async_channel::unbounded();
        let (cancel_tx, _cancel_rx) = async_channel::unbounded();
        let mqtt_client = AsyncClient::from_senders(requests_tx, cancel_tx);
        let controller = HomieController {
            base_topic: "base_topic".to_owned(),
            mqtt_client,
            devices: Mutex::new(Arc::new(HashMap::new())),
        };
        (controller, requests_rx)
    }

    fn expect_subscriptions(requests_rx: &Receiver<Request>, subscription_topics: &[&str]) {
        let requests: Vec<_> = (0..subscription_topics.len())
            .map(|_| {
                let request = requests_rx.try_recv().unwrap();
                if let Request::Subscribe(subscribe) = request {
                    subscribe
                } else {
                    panic!("Expected subscribe but got {:?}", request);
                }
            })
            .collect();

        for topic in subscription_topics {
            let expected = Subscribe::new(*topic, QoS::AtLeastOnce);
            assert!(requests.contains(&expected));
        }
    }

    async fn publish(
        controller: &HomieController,
        topic: &str,
        payload: &str,
    ) -> Result<Option<Event>, PollError> {
        controller
            .handle_event(Packet::Publish(Publish::new(
                topic,
                QoS::AtLeastOnce,
                payload,
            )))
            .await
    }

    #[tokio::test]
    async fn controller_start() -> Result<(), Box<dyn std::error::Error>> {
        let (controller, requests_rx) = make_test_controller();

        // Start discovering.
        controller.start().await?;
        expect_subscriptions(&requests_rx, &["base_topic/+/$homie"]);

        // Discover a new device.
        assert_eq!(
            publish(&controller, "base_topic/device_id/$homie", "4.0").await?,
            Some(Event::DeviceUpdated {
                device_id: "device_id".to_owned(),
                has_required_attributes: false
            })
        );
        expect_subscriptions(
            &requests_rx,
            &[
                "base_topic/device_id/+",
                "base_topic/device_id/$fw/+",
                "base_topic/device_id/$stats/+",
            ],
        );
        assert_eq!(
            publish(&controller, "base_topic/device_id/$name", "Device name").await?,
            Some(Event::DeviceUpdated {
                device_id: "device_id".to_owned(),
                has_required_attributes: false
            })
        );
        assert_eq!(
            publish(&controller, "base_topic/device_id/$state", "ready").await?,
            Some(Event::DeviceUpdated {
                device_id: "device_id".to_owned(),
                has_required_attributes: true
            })
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
            Some(Event::DeviceUpdated {
                device_id: "device_id".to_owned(),
                has_required_attributes: false
            })
        );
        expect_subscriptions(&requests_rx, &["base_topic/device_id/node_id/+"]);
        assert_eq!(
            publish(
                &controller,
                "base_topic/device_id/node_id/$name",
                "Node name"
            )
            .await?,
            Some(Event::NodeUpdated {
                device_id: "device_id".to_owned(),
                node_id: "node_id".to_owned(),
                has_required_attributes: false
            })
        );
        assert_eq!(
            publish(
                &controller,
                "base_topic/device_id/node_id/$type",
                "Node type"
            )
            .await?,
            Some(Event::NodeUpdated {
                device_id: "device_id".to_owned(),
                node_id: "node_id".to_owned(),
                has_required_attributes: false
            })
        );
        let mut expected_node = Node::new("node_id");
        expected_node.name = Some("Node name".to_owned());
        expected_node.node_type = Some("Node type".to_owned());
        expected_device.add_node(expected_node.clone());
        assert_eq!(
            controller.devices().get("device_id").unwrap().to_owned(),
            expected_device
        );

        // A property on the node.
        assert_eq!(
            publish(
                &controller,
                "base_topic/device_id/node_id/$properties",
                "property_id"
            )
            .await?,
            Some(Event::NodeUpdated {
                device_id: "device_id".to_owned(),
                node_id: "node_id".to_owned(),
                has_required_attributes: false
            })
        );
        expect_subscriptions(
            &requests_rx,
            &["base_topic/device_id/node_id/property_id/+"],
        );
        assert_eq!(
            publish(
                &controller,
                "base_topic/device_id/node_id/property_id/$name",
                "Property name"
            )
            .await?,
            Some(Event::PropertyUpdated {
                device_id: "device_id".to_owned(),
                node_id: "node_id".to_owned(),
                property_id: "property_id".to_owned(),
                has_required_attributes: false
            })
        );
        assert_eq!(
            publish(
                &controller,
                "base_topic/device_id/node_id/property_id/$datatype",
                "integer"
            )
            .await?,
            Some(Event::PropertyUpdated {
                device_id: "device_id".to_owned(),
                node_id: "node_id".to_owned(),
                property_id: "property_id".to_owned(),
                has_required_attributes: true
            })
        );
        let mut expected_property = Property::new("property_id");
        expected_property.name = Some("Property name".to_owned());
        expected_property.datatype = Some(Datatype::Integer);
        expected_node.add_property(expected_property);
        expected_device.add_node(expected_node);
        assert_eq!(
            controller.devices().get("device_id").unwrap().to_owned(),
            expected_device
        );

        Ok(())
    }

    #[tokio::test]
    async fn constructs_device_tree() -> Result<(), Box<dyn std::error::Error>> {
        let (controller, _requests_rx) = make_test_controller();

        // Discover a new device with property with nodes.

        controller.start().await?;
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

        // Construct the fixture

        let expected_property = Property {
            name: Some("Property name".to_owned()),
            datatype: Some(Datatype::Integer),
            ..Property::new("property_id")
        };

        // We could do something like this here?
        //     ..Node::with_property("node_id", expected_property)
        let mut expected_node = Node {
            name: Some("Node name".to_owned()),
            node_type: Some("Node type".to_owned()),
            ..Node::new("node_id")
        };
        expected_node.add_property(expected_property);

        // Similarly, ..Device::with_node("device_id", "4.0", expected_node)
        let mut expected_device = Device {
            name: Some("Device name".to_owned()),
            state: State::Ready,
            ..Device::new("device_id", "4.0")
        };
        // Maybe we don't need two nodes?
        expected_device.add_node(expected_node.clone());
        expected_device.add_node(expected_node);

        assert_eq!(
            controller.devices().get("device_id").unwrap().to_owned(),
            expected_device
        );

        Ok(())
    }
}
