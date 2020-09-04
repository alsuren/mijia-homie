use async_channel::{SendError, Sender};
use futures::future::try_join;
use futures::FutureExt;
use local_ipaddress;
use mac_address::get_mac_address;
use rumqttc::{self, EventLoop, LastWill, MqttOptions, Publish, QoS, Request};
use std::error::Error;
use std::fmt::Display;
use std::future::Future;
use std::time::{Duration, Instant};
use tokio::task::{self, JoinError, JoinHandle};
use tokio::time::delay_for;

const HOMIE_VERSION: &str = "4.0";
const HOMIE_IMPLEMENTATION: &str = "homie-rs";
const DEFAULT_FIRMWARE_NAME: &str = env!("CARGO_PKG_NAME");
const DEFAULT_FIRMWARE_VERSION: &str = env!("CARGO_PKG_VERSION");
const STATS_INTERVAL: Duration = Duration::from_secs(60);
const REQUESTS_CAP: usize = 10;

/// The data type for a Homie property.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Datatype {
    Integer,
    Float,
    Boolean,
    String,
    Enum,
    Color,
}

impl Into<Vec<u8>> for Datatype {
    fn into(self) -> Vec<u8> {
        match self {
            Self::Integer => "integer",
            Self::Float => "float",
            Self::Boolean => "boolean",
            Self::String => "string",
            Self::Enum => "enum",
            Self::Color => "color",
        }
        .into()
    }
}

/// A [property](https://homieiot.github.io/specification/#properties) of a Homie node.
#[derive(Clone, Debug)]
pub struct Property {
    id: String,
    name: String,
    datatype: Datatype,
    unit: Option<String>,
    format: Option<String>,
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
    /// * `format`: The format for the property, if any. This must be specified if the datatype is
    ///   `Enum` or `Color`, and may be specified if the datatype is `Integer` or `Float`.
    pub fn new(
        id: &str,
        name: &str,
        datatype: Datatype,
        unit: Option<&str>,
        format: Option<&str>,
    ) -> Property {
        Property {
            id: id.to_owned(),
            name: name.to_owned(),
            datatype,
            unit: unit.map(|s| s.to_owned()),
            format: format.map(|s| s.to_owned()),
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    Init,
    Ready,
    Disconnected,
    Sleeping,
    Lost,
    Alert,
}

impl State {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::Ready => "ready",
            Self::Disconnected => "disconnected",
            Self::Sleeping => "sleeping",
            Self::Lost => "lost",
            Self::Alert => "alert",
        }
    }
}

impl Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Into<Vec<u8>> for State {
    fn into(self) -> Vec<u8> {
        self.as_str().into()
    }
}

/// Builder for `HomieDevice` and associated objects.
#[derive(Debug)]
pub struct HomieDeviceBuilder {
    device_base: String,
    device_name: String,
    firmware_name: String,
    firmware_version: String,
    mqtt_options: MqttOptions,
}

impl HomieDeviceBuilder {
    /// Set the firmware name and version to be advertised for the Homie device.
    ///
    /// If this is not set, it will default to the cargo package name and version.
    #[allow(dead_code)]
    pub fn set_firmware(&mut self, firmware_name: &str, firmware_version: &str) {
        self.firmware_name = firmware_name.to_string();
        self.firmware_version = firmware_version.to_string();
    }

    /// Create a new Homie device, connect to the MQTT server, and start a task to handle the MQTT
    /// connection.
    ///
    /// # Return value
    /// A pair of the `HomieDevice` itself, and a `Future` for the tasks which handle the MQTT
    /// connection. You should join on this future to handle any errors it returns.
    pub async fn spawn(
        self,
    ) -> Result<
        (
            HomieDevice,
            impl Future<Output = Result<(), Box<dyn Error + Send + Sync>>>,
        ),
        SendError<Request>,
    > {
        let (event_loop, mut homie, stats, firmware) = self.build().await;

        // This needs to be spawned before we wait for anything to be sent, as the start() calls below do.
        let event_task = HomieDevice::spawn(event_loop);

        stats.start().await?;
        firmware.start().await?;
        homie.start().await?;

        let stats_task = stats.spawn();

        let join_handle = try_join_handles(event_task, stats_task).map(|r| r.map(|((), ())| ()));

        Ok((homie, join_handle))
    }

    async fn build(self) -> (EventLoop, HomieDevice, HomieStats, HomieFirmware) {
        let mut mqtt_options = self.mqtt_options;
        mqtt_options.set_last_will(LastWill::new(
            format!("{}/$state", self.device_base),
            State::Lost,
            QoS::AtLeastOnce,
            true,
        ));
        let event_loop = EventLoop::new(mqtt_options, REQUESTS_CAP).await;

        let publisher = DevicePublisher::new(event_loop.handle(), self.device_base);
        let homie = HomieDevice::new(publisher.clone(), self.device_name, &EXTENSION_IDS);

        let stats = HomieStats::new(publisher.clone());
        let firmware = HomieFirmware::new(publisher, self.firmware_name, self.firmware_version);

        (event_loop, homie, stats, firmware)
    }
}

const EXTENSION_IDS: [&str; 2] = [HomieStats::EXTENSION_ID, HomieFirmware::EXTENSION_ID];

/// A Homie [device](https://homieiot.github.io/specification/#devices). This corresponds to a
/// single MQTT connection.
#[derive(Debug)]
pub struct HomieDevice {
    publisher: DevicePublisher,
    device_name: String,
    nodes: Vec<Node>,
    state: State,
    extension_ids: String,
}

impl HomieDevice {
    /// Create a builder to construct a new Homie device.
    ///
    /// # Arguments
    /// * `device_base`: The base topic ID for the device, including the Homie base topic. This
    ///   might be something like "homie/my-device-id" if you are using the default Homie
    ///   [base topic](https://homieiot.github.io/specification/#base-topic). This must be
    ///   unique per MQTT server.
    /// * `device_name`: The human-readable name of the device.
    /// * `mqtt_options`: Options for the MQTT connection, including which server to connect to.
    pub fn builder(
        device_base: &str,
        device_name: &str,
        mqtt_options: MqttOptions,
    ) -> HomieDeviceBuilder {
        HomieDeviceBuilder {
            device_base: device_base.to_string(),
            device_name: device_name.to_string(),
            firmware_name: DEFAULT_FIRMWARE_NAME.to_string(),
            firmware_version: DEFAULT_FIRMWARE_VERSION.to_string(),
            mqtt_options,
        }
    }

    fn new(publisher: DevicePublisher, device_name: String, extension_ids: &[&str]) -> HomieDevice {
        HomieDevice {
            publisher,
            device_name,
            nodes: vec![],
            state: State::Disconnected,
            extension_ids: extension_ids.join(","),
        }
    }

    async fn start(&mut self) -> Result<(), SendError<Request>> {
        assert_eq!(self.state, State::Disconnected);
        self.publisher
            .publish_retained("$homie", HOMIE_VERSION)
            .await?;
        self.publisher
            .publish_retained("$extensions", self.extension_ids.as_str())
            .await?;
        self.publisher
            .publish_retained("$implementation", HOMIE_IMPLEMENTATION)
            .await?;
        self.publisher
            .publish_retained("$name", self.device_name.as_str())
            .await?;
        self.set_state(State::Init).await?;
        Ok(())
    }

    /// Spawn a task to handle the EventLoop.
    fn spawn(mut event_loop: EventLoop) -> JoinHandle<Result<(), Box<dyn Error + Send + Sync>>> {
        task::spawn(async move {
            loop {
                let (incoming, outgoing) = event_loop.poll().await?;
                log::trace!("Incoming = {:?}, Outgoing = {:?}", incoming, outgoing);
            }
        })
    }

    /// Add a node to the Homie device. It will immediately be published.
    ///
    /// This will panic if you attempt to add a node with the same ID as a node which was previously
    /// added.
    pub async fn add_node(&mut self, node: Node) -> Result<(), SendError<Request>> {
        // First check that there isn't already a node with the same ID.
        if self.nodes.iter().any(|n| n.id == node.id) {
            panic!("Tried to add node with duplicate ID: {:?}", node);
        }
        self.nodes.push(node);
        // `node` was moved into the `nodes` vector, but we can safely get a reference to it because
        // nothing else can modify `nodes` in the meantime.
        let node = &self.nodes[self.nodes.len() - 1];

        self.publish_node(&node).await?;
        self.publish_nodes().await
    }

    /// Remove the node with the given ID.
    pub async fn remove_node(&mut self, node_id: &str) -> Result<(), SendError<Request>> {
        self.nodes.retain(|n| n.id != node_id);
        self.publish_nodes().await
    }

    async fn publish_node(&self, node: &Node) -> Result<(), SendError<Request>> {
        self.publisher
            .publish_retained(&format!("{}/$name", node.id), node.name.as_str())
            .await?;
        self.publisher
            .publish_retained(&format!("{}/$type", node.id), node.node_type.as_str())
            .await?;
        let mut property_ids: Vec<&str> = vec![];
        for property in &node.properties {
            property_ids.push(&property.id);
            self.publisher
                .publish_retained(
                    &format!("{}/{}/$name", node.id, property.id),
                    property.name.as_str(),
                )
                .await?;
            self.publisher
                .publish_retained(
                    &format!("{}/{}/$datatype", node.id, property.id),
                    property.datatype,
                )
                .await?;
            if let Some(unit) = &property.unit {
                self.publisher
                    .publish_retained(&format!("{}/{}/$unit", node.id, property.id), unit.as_str())
                    .await?;
            }
            if let Some(format) = &property.format {
                self.publisher
                    .publish_retained(
                        &format!("{}/{}/$format", node.id, property.id),
                        format.as_str(),
                    )
                    .await?;
            }
        }
        self.publisher
            .publish_retained(&format!("{}/$properties", node.id), property_ids.join(","))
            .await?;
        Ok(())
    }

    async fn publish_nodes(&mut self) -> Result<(), SendError<Request>> {
        let node_ids = self
            .nodes
            .iter()
            .map(|node| node.id.as_str())
            .collect::<Vec<&str>>()
            .join(",");
        self.publisher.publish_retained("$nodes", node_ids).await
    }

    async fn set_state(&mut self, state: State) -> Result<(), SendError<Request>> {
        self.state = state;
        self.publisher.publish_retained("$state", self.state).await
    }

    /// Update the [state](https://homieiot.github.io/specification/#device-lifecycle) of the Homie
    /// device to 'ready'. This should be called once it is ready to begin normal operation, or to
    /// return to normal operation after calling `sleep()` or `alert()`.
    pub async fn ready(&mut self) -> Result<(), SendError<Request>> {
        assert!(&[State::Init, State::Sleeping, State::Alert].contains(&self.state));
        self.set_state(State::Ready).await
    }

    /// Update the [state](https://homieiot.github.io/specification/#device-lifecycle) of the Homie
    /// device to 'sleeping'. This should be only be called after `ready()`, otherwise it will panic.
    pub async fn sleep(&mut self) -> Result<(), SendError<Request>> {
        assert_eq!(self.state, State::Ready);
        self.set_state(State::Sleeping).await
    }

    /// Update the [state](https://homieiot.github.io/specification/#device-lifecycle) of the Homie
    /// device to 'alert', to indicate that something wrong is happening and manual intervention may
    /// be required. This should be only be called after `ready()`, otherwise it will panic.
    pub async fn alert(&mut self) -> Result<(), SendError<Request>> {
        assert_eq!(self.state, State::Ready);
        self.set_state(State::Alert).await
    }

    /// Disconnect cleanly from the MQTT broker, after updating the state of the Homie device to
    // 'disconnected'.
    pub async fn disconnect(mut self) -> Result<(), SendError<Request>> {
        self.set_state(State::Disconnected).await?;
        self.publisher.disconnect().await
    }

    /// Publish a new value for the given property of the given node of this device. The caller is
    /// responsible for ensuring that the value is of the correct type.
    pub async fn publish_value(
        &self,
        node_id: &str,
        property_id: &str,
        value: impl ToString,
    ) -> Result<(), SendError<Request>> {
        self.publisher
            .publish_retained(&format!("{}/{}", node_id, property_id), value.to_string())
            .await
    }
}

#[derive(Clone, Debug)]
struct DevicePublisher {
    requests_tx: Sender<Request>,
    device_base: String,
}

impl DevicePublisher {
    fn new(requests_tx: Sender<Request>, device_base: String) -> Self {
        Self {
            requests_tx,
            device_base,
        }
    }

    async fn publish_retained(
        &self,
        subtopic: &str,
        value: impl Into<Vec<u8>>,
    ) -> Result<(), SendError<Request>> {
        let name = format!("{}/{}", self.device_base, subtopic);
        let mut publish = Publish::new(name, QoS::AtLeastOnce, value);
        publish.set_retain(true);
        self.requests_tx.send(publish.into()).await
    }

    async fn disconnect(&self) -> Result<(), SendError<Request>> {
        self.requests_tx.send(Request::Disconnect).await
    }
}

/// Legacy stats extension.
#[derive(Debug)]
struct HomieStats {
    publisher: DevicePublisher,
    start_time: Instant,
}

impl HomieStats {
    const EXTENSION_ID: &'static str = "org.homie.legacy-stats:0.1.1:[4.x]";

    fn new(publisher: DevicePublisher) -> Self {
        let now = Instant::now();
        Self {
            publisher,
            start_time: now,
        }
    }

    /// Send initial topics.
    async fn start(&self) -> Result<(), SendError<Request>> {
        self.publisher
            .publish_retained("$stats/interval", STATS_INTERVAL.as_secs().to_string())
            .await
    }

    /// Periodically send stats.
    fn spawn(self) -> JoinHandle<Result<(), Box<dyn Error + Send + Sync>>> {
        task::spawn(async move {
            loop {
                let uptime = Instant::now() - self.start_time;
                self.publisher
                    .publish_retained("$stats/uptime", uptime.as_secs().to_string())
                    .await?;
                delay_for(STATS_INTERVAL).await;
            }
        })
    }
}

/// Legacy firmware extension.
#[derive(Debug)]
struct HomieFirmware {
    publisher: DevicePublisher,
    firmware_name: String,
    firmware_version: String,
}

impl HomieFirmware {
    const EXTENSION_ID: &'static str = "org.homie.legacy-firmware:0.1.1:[4.x]";

    fn new(publisher: DevicePublisher, firmware_name: String, firmware_version: String) -> Self {
        Self {
            publisher,
            firmware_name,
            firmware_version,
        }
    }

    /// Send initial topics.
    async fn start(&self) -> Result<(), SendError<Request>> {
        self.publisher
            .publish_retained("$localip", local_ipaddress::get().unwrap())
            .await?;
        self.publisher
            .publish_retained("$mac", get_mac_address().unwrap().unwrap().to_string())
            .await?;
        self.publisher
            .publish_retained("$fw/name", self.firmware_name.as_str())
            .await?;
        self.publisher
            .publish_retained("$fw/version", self.firmware_version.as_str())
            .await?;
        Ok(())
    }
}

fn try_join_handles<A, B, E>(
    a: JoinHandle<Result<A, E>>,
    b: JoinHandle<Result<B, E>>,
) -> impl Future<Output = Result<(A, B), E>>
where
    E: From<JoinError>,
{
    // Unwrap the JoinHandle results to get to the real results.
    try_join(a.map(|res| Ok(res??)), b.map(|res| Ok(res??)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_channel::Receiver;

    fn make_test_device() -> (HomieDevice, Receiver<Request>) {
        let (requests_tx, requests_rx) = async_channel::unbounded();
        let publisher = DevicePublisher::new(requests_tx, "homie/test-device".to_string());
        let device = HomieDevice::new(publisher, "Test device".to_string(), &[]);
        (device, requests_rx)
    }

    #[tokio::test]
    #[should_panic(expected = "Tried to add node with duplicate ID")]
    async fn add_node_fails_given_duplicate_id() {
        let (mut device, rx) = make_test_device();

        device
            .add_node(Node::new(
                "id".to_string(),
                "Name".to_string(),
                "type".to_string(),
                vec![],
            ))
            .await
            .unwrap();
        device
            .add_node(Node::new(
                "id".to_string(),
                "Name 2".to_string(),
                "type2".to_string(),
                vec![],
            ))
            .await
            .unwrap();

        // Need to keep rx alive until here so that the channel isn't closed.
        drop(rx);
    }

    #[tokio::test]
    #[should_panic(expected = "Init")]
    async fn ready_fails_if_called_before_start() {
        let (mut device, rx) = make_test_device();

        device.ready().await.unwrap();

        // Need to keep rx alive until here so that the channel isn't closed.
        drop(rx);
    }

    #[tokio::test]
    async fn start_succeeds_with_no_nodes() -> Result<(), SendError<Request>> {
        let (mut device, rx) = make_test_device();

        device.start().await?;
        device.ready().await?;

        // Need to keep rx alive until here so that the channel isn't closed.
        drop(rx);
        Ok(())
    }

    #[tokio::test]
    async fn sleep_then_ready_again_succeeds() -> Result<(), SendError<Request>> {
        let (mut device, rx) = make_test_device();

        device.start().await?;
        device.ready().await?;
        device.sleep().await?;
        device.ready().await?;

        // Need to keep rx alive until here so that the channel isn't closed.
        drop(rx);
        Ok(())
    }

    #[tokio::test]
    async fn alert_then_ready_again_succeeds() -> Result<(), SendError<Request>> {
        let (mut device, rx) = make_test_device();

        device.start().await?;
        device.ready().await?;
        device.alert().await?;
        device.ready().await?;

        // Need to keep rx alive until here so that the channel isn't closed.
        drop(rx);
        Ok(())
    }

    #[tokio::test]
    async fn disconnect_succeeds_before_ready() -> Result<(), SendError<Request>> {
        let (mut device, rx) = make_test_device();

        device.start().await?;
        device.disconnect().await?;

        // Need to keep rx alive until here so that the channel isn't closed.
        drop(rx);
        Ok(())
    }

    #[tokio::test]
    async fn disconnect_succeeds_after_ready() -> Result<(), SendError<Request>> {
        let (mut device, rx) = make_test_device();

        device.start().await?;
        device.ready().await?;
        device.disconnect().await?;

        // Need to keep rx alive until here so that the channel isn't closed.
        drop(rx);
        Ok(())
    }

    #[tokio::test]
    async fn minimal_build_succeeds() -> Result<(), SendError<Request>> {
        let builder = HomieDevice::builder(
            "homie/test-device",
            "Test device",
            MqttOptions::new("client_id", "hostname", 1234),
        );

        let (_event_loop, homie, _stats, firmware) = builder.build().await;

        assert_eq!(homie.device_name, "Test device");
        assert_eq!(homie.publisher.device_base, "homie/test-device");
        assert_eq!(firmware.firmware_name, DEFAULT_FIRMWARE_NAME);
        assert_eq!(firmware.firmware_version, DEFAULT_FIRMWARE_VERSION);

        Ok(())
    }

    #[tokio::test]
    async fn set_firmware_build_succeeds() -> Result<(), SendError<Request>> {
        let mut builder = HomieDevice::builder(
            "homie/test-device",
            "Test device",
            MqttOptions::new("client_id", "hostname", 1234),
        );

        builder.set_firmware("firmware_name", "firmware_version");

        let (_event_loop, homie, _stats, firmware) = builder.build().await;

        assert_eq!(homie.device_name, "Test device");
        assert_eq!(homie.publisher.device_base, "homie/test-device");
        assert_eq!(firmware.firmware_name, "firmware_name");
        assert_eq!(firmware.firmware_version, "firmware_version");

        Ok(())
    }

    #[tokio::test]
    async fn add_node_succeeds_before_and_after_start() -> Result<(), SendError<Request>> {
        let (mut device, rx) = make_test_device();

        device
            .add_node(Node::new(
                "id".to_string(),
                "Name".to_string(),
                "type".to_string(),
                vec![],
            ))
            .await?;

        device.start().await?;
        device.ready().await?;

        // Add another node after starting.
        device
            .add_node(Node::new(
                "id2".to_string(),
                "Name 2".to_string(),
                "type2".to_string(),
                vec![],
            ))
            .await?;

        // Need to keep rx alive until here so that the channel isn't closed.
        drop(rx);
        Ok(())
    }

    /// Add a node, remove it, and add it back again.
    #[tokio::test]
    async fn add_node_succeeds_after_remove() -> Result<(), SendError<Request>> {
        let (mut device, rx) = make_test_device();

        device
            .add_node(Node::new(
                "id".to_string(),
                "Name".to_string(),
                "type".to_string(),
                vec![],
            ))
            .await?;

        device.remove_node("id").await?;

        // Adding it back shouldn't give an error.
        device
            .add_node(Node::new(
                "id".to_string(),
                "Name".to_string(),
                "type".to_string(),
                vec![],
            ))
            .await?;

        // Need to keep rx alive until here so that the channel isn't closed.
        drop(rx);
        Ok(())
    }
}
