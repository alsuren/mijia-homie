//! `homie-device` is a library for creating devices implementing the
//! [Homie convention](https://homieiot.github.io/) for IoT devices connecting to an MQTT broker.
//!
//! See the examples directory for examples of how to use it.

use futures::future::try_join;
use futures::FutureExt;

use mac_address::get_mac_address;
use rumqttc::{
    self, AsyncClient, ClientError, ConnectionError, Event, EventLoop, Incoming, LastWill,
    MqttOptions, Outgoing, QoS, StateError,
};
use std::fmt::{self, Debug, Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::str;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::Mutex;
use tokio::task::{self, JoinError, JoinHandle};
use tokio::time::sleep;

mod types;
pub use crate::types::{Datatype, Node, Property};
mod values;
pub use crate::values::{Color, ColorFormat, ColorHSV, ColorRGB};

const HOMIE_VERSION: &str = "4.0";
const HOMIE_IMPLEMENTATION: &str = "homie-rs";
const STATS_INTERVAL: Duration = Duration::from_secs(60);
const REQUESTS_CAP: usize = 10;

/// The default duration to wait between attempts to reconnect to the MQTT broker if the connection
/// is lost.
pub const DEFAULT_RECONNECT_INTERVAL: Duration = Duration::from_secs(5);

/// Error type for futures representing tasks spawned by this crate.
#[derive(Error, Debug)]
pub enum SpawnError {
    #[error("{0}")]
    Client(#[from] ClientError),
    #[error("{0}")]
    Connection(#[from] ConnectionError),
    #[error("Task failed: {0}")]
    Join(#[from] JoinError),
    #[error("Internal error: {0}")]
    Internal(&'static str),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    /// The device is connected to the MQTT broker but is not yet ready to operate.
    Init,
    /// The device is connected and operational.
    Ready,
    /// The device has cleanly disconnected from the MQTT broker.
    Disconnected,
    /// The device is currently sleeping.
    Sleeping,
    /// The device was uncleanly disconnected from the MQTT broker. This could happen due to a
    /// network issue, power failure or some other unexpected failure.
    Lost,
    /// The device is connected to the MQTT broker but something is wrong and it may require human
    /// intervention.
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
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Into<Vec<u8>> for State {
    fn into(self) -> Vec<u8> {
        self.as_str().into()
    }
}

type UpdateCallback = Box<
    dyn FnMut(String, String, String) -> Pin<Box<dyn Future<Output = Option<String>> + Send>>
        + Send
        + Sync,
>;

/// Builder for `HomieDevice` and associated objects.
pub struct HomieDeviceBuilder {
    device_base: String,
    device_name: String,
    firmware_name: Option<String>,
    firmware_version: Option<String>,
    mqtt_options: MqttOptions,
    update_callback: Option<UpdateCallback>,
    reconnect_interval: Duration,
}

impl Debug for HomieDeviceBuilder {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("HomieDeviceBuilder")
            .field("device_base", &self.device_base)
            .field("device_name", &self.device_name)
            .field("firmware_name", &self.firmware_name)
            .field("firmware_version", &self.firmware_version)
            .field("mqtt_options", &self.mqtt_options)
            .field(
                "update_callback",
                &self.update_callback.as_ref().map(|_| "..."),
            )
            .finish()
    }
}

impl HomieDeviceBuilder {
    /// Set the firmware name and version to be advertised for the Homie device.
    ///
    /// If this is not set, it will default to the cargo package name and version.
    pub fn set_firmware(&mut self, firmware_name: &str, firmware_version: &str) {
        self.firmware_name = Some(firmware_name.to_string());
        self.firmware_version = Some(firmware_version.to_string());
    }

    /// Set the duration to wait between attempts to reconnect to the MQTT broker if the connection
    /// is lost.
    ///
    /// If this is not set it will default to `DEFAULT_RECONNECT_INTERVAL`.
    pub fn set_reconnect_interval(&mut self, reconnect_interval: Duration) {
        self.reconnect_interval = reconnect_interval;
    }

    pub fn set_update_callback<F, Fut>(&mut self, mut update_callback: F)
    where
        F: (FnMut(String, String, String) -> Fut) + Send + Sync + 'static,
        Fut: Future<Output = Option<String>> + Send + 'static,
    {
        self.update_callback = Some(Box::new(
            move |node_id: String, property_id: String, value: String| {
                update_callback(node_id, property_id, value).boxed()
            },
        ));
    }

    /// Create a new Homie device, connect to the MQTT broker, and start a task to handle the MQTT
    /// connection.
    ///
    /// # Return value
    /// A pair of the `HomieDevice` itself, and a `Future` for the tasks which handle the MQTT
    /// connection. You should join on this future to handle any errors it returns.
    pub async fn spawn(
        self,
    ) -> Result<(HomieDevice, impl Future<Output = Result<(), SpawnError>>), ClientError> {
        let (event_loop, mut homie, stats, firmware, update_callback) = self.build();

        // This needs to be spawned before we wait for anything to be sent, as the start() calls below do.
        let event_task = homie.spawn(event_loop, update_callback);

        stats.start().await?;
        if let Some(firmware) = firmware {
            firmware.start().await?;
        }
        homie.start().await?;

        let stats_task = stats.spawn();
        let join_handle = try_join(event_task, stats_task).map(simplify_unit_pair);

        Ok((homie, join_handle))
    }

    fn build(
        self,
    ) -> (
        EventLoop,
        HomieDevice,
        HomieStats,
        Option<HomieFirmware>,
        Option<UpdateCallback>,
    ) {
        let mut mqtt_options = self.mqtt_options;
        let last_will = LastWill::new(
            format!("{}/$state", self.device_base),
            State::Lost,
            QoS::AtLeastOnce,
            true,
        );
        mqtt_options.set_last_will(last_will);
        // Setting this to false means that our subscriptions will be kept when we reconnect.
        mqtt_options.set_clean_session(false);
        let (client, event_loop) = AsyncClient::new(mqtt_options, REQUESTS_CAP);

        let publisher = DevicePublisher::new(client, self.device_base);

        let mut extension_ids = vec![HomieStats::EXTENSION_ID];
        let stats = HomieStats::new(publisher.clone());
        let firmware = if let (Some(firmware_name), Some(firmware_version)) =
            (self.firmware_name, self.firmware_version)
        {
            extension_ids.push(HomieFirmware::EXTENSION_ID);
            Some(HomieFirmware::new(
                publisher.clone(),
                firmware_name,
                firmware_version,
            ))
        } else {
            None
        };

        let homie = HomieDevice::new(
            publisher,
            self.device_name,
            &extension_ids,
            self.reconnect_interval,
        );

        (event_loop, homie, stats, firmware, self.update_callback)
    }
}

/// A Homie [device](https://homieiot.github.io/specification/#devices). This corresponds to a
/// single MQTT connection.
#[derive(Debug)]
pub struct HomieDevice {
    state: Arc<Mutex<DeviceState>>,
    reconnect_interval: Duration,
}

impl HomieDevice {
    /// Create a builder to construct a new Homie device.
    ///
    /// # Arguments
    /// * `device_base`: The base topic ID for the device, including the Homie base topic. This
    ///   might be something like "homie/my-device-id" if you are using the default Homie
    ///   [base topic](https://homieiot.github.io/specification/#base-topic). This must be
    ///   unique per MQTT broker.
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
            firmware_name: None,
            firmware_version: None,
            mqtt_options,
            update_callback: None,
            reconnect_interval: DEFAULT_RECONNECT_INTERVAL,
        }
    }

    fn new(
        publisher: DevicePublisher,
        device_name: String,
        extension_ids: &[&str],
        reconnect_interval: Duration,
    ) -> HomieDevice {
        HomieDevice {
            state: Arc::new(Mutex::new(DeviceState {
                publisher,
                device_name,
                nodes: vec![],
                state: State::Disconnected,
                extension_ids: extension_ids.join(","),
            })),
            reconnect_interval,
        }
    }

    async fn start(&mut self) -> Result<(), ClientError> {
        let mut state = self.state.lock().await;
        assert_eq!(state.state, State::Disconnected);
        state
            .publisher
            .publish_retained("$homie", HOMIE_VERSION)
            .await?;
        state
            .publisher
            .publish_retained("$extensions", state.extension_ids.as_str())
            .await?;
        state
            .publisher
            .publish_retained("$implementation", HOMIE_IMPLEMENTATION)
            .await?;
        state
            .publisher
            .publish_retained("$name", state.device_name.as_str())
            .await?;
        state.set_state(State::Init).await?;
        Ok(())
    }

    /// Spawn a task to handle the EventLoop.
    fn spawn(
        &self,
        mut event_loop: EventLoop,
        mut update_callback: Option<UpdateCallback>,
    ) -> impl Future<Output = Result<(), SpawnError>> {
        let (incoming_tx, incoming_rx) = async_channel::unbounded();

        let reconnect_interval = self.reconnect_interval;
        let mqtt_task = task::spawn(async move {
            let mut disconnect_requested = false;
            loop {
                match event_loop.poll().await {
                    Ok(notification) => {
                        log::trace!("Notification = {:?}", notification);

                        match notification {
                            Event::Incoming(incoming) => {
                                incoming_tx.send(incoming).await.map_err(|_| {
                                    SpawnError::Internal("Incoming event channel receiver closed.")
                                })?;
                            }
                            Event::Outgoing(Outgoing::Disconnect) => {
                                // Flag that we have tried to disconnect intentionally, but keep
                                // polling until we get an error implying that we have actually disconnected.
                                disconnect_requested = true;
                            }
                            Event::Outgoing(_) => {}
                        }
                    }
                    Err(e) => {
                        if disconnect_requested {
                            log::trace!("Disconnected as requested.");
                            return Ok(());
                        }
                        log::error!("Failed to poll EventLoop: {}", e);
                        match e {
                            ConnectionError::Io(_) => {
                                sleep(reconnect_interval).await;
                            }
                            ConnectionError::MqttState(StateError::AwaitPingResp) => {}
                            _ => return Err(e.into()),
                        }
                    }
                }
            }
        });

        let state = self.state.clone();
        let incoming_task: JoinHandle<Result<(), SpawnError>> = task::spawn(async move {
            let publisher = state.lock().await.publisher.clone();
            let device_base = format!("{}/", publisher.device_base);
            loop {
                match incoming_rx.recv().await {
                    Err(_) => {
                        // This happens when the task above exits either because of an error or
                        // because we disconnected intentionally.
                        log::trace!("Incoming event channel sender closed.");
                        return Ok(());
                    }
                    Ok(Incoming::Publish(publish)) => {
                        if let Some(rest) = publish.topic.strip_prefix(&device_base) {
                            if let ([node_id, property_id, "set"], Ok(payload)) = (
                                rest.split('/').collect::<Vec<&str>>().as_slice(),
                                str::from_utf8(&publish.payload),
                            ) {
                                log::trace!(
                                    "set node {:?} property {:?} to {:?}",
                                    node_id,
                                    property_id,
                                    payload
                                );
                                if let Some(callback) = update_callback.as_mut() {
                                    if let Some(value) = callback(
                                        node_id.to_string(),
                                        property_id.to_string(),
                                        payload.to_string(),
                                    )
                                    .await
                                    {
                                        publisher
                                            .publish_retained(
                                                &format!("{}/{}", node_id, property_id),
                                                value,
                                            )
                                            .await?;
                                    }
                                }
                            }
                        } else {
                            log::warn!("Unexpected publish: {:?}", publish);
                        }
                    }
                    Ok(Incoming::ConnAck(_)) => {
                        // TODO: Only if this is not the initial connection.
                        state.lock().await.republish_all().await?;
                    }
                    _ => {}
                }
            }
        });
        try_join_unit_handles(mqtt_task, incoming_task)
    }

    /// Add a node to the Homie device. It will immediately be published.
    ///
    /// This will panic if you attempt to add a node with the same ID as a node which was previously
    /// added.
    pub async fn add_node(&mut self, node: Node) -> Result<(), ClientError> {
        self.state.lock().await.add_node(node).await
    }

    /// Remove the node with the given ID.
    pub async fn remove_node(&mut self, node_id: &str) -> Result<(), ClientError> {
        self.state.lock().await.remove_node(node_id).await
    }

    /// Update the [state](https://homieiot.github.io/specification/#device-lifecycle) of the Homie
    /// device to 'ready'. This should be called once it is ready to begin normal operation, or to
    /// return to normal operation after calling `sleep()` or `alert()`.
    pub async fn ready(&mut self) -> Result<(), ClientError> {
        let mut state = self.state.lock().await;
        assert!(&[State::Init, State::Sleeping, State::Alert].contains(&state.state));
        state.set_state(State::Ready).await
    }

    /// Update the [state](https://homieiot.github.io/specification/#device-lifecycle) of the Homie
    /// device to 'sleeping'. This should be only be called after `ready()`, otherwise it will panic.
    pub async fn sleep(&mut self) -> Result<(), ClientError> {
        let mut state = self.state.lock().await;
        assert_eq!(state.state, State::Ready);
        state.set_state(State::Sleeping).await
    }

    /// Update the [state](https://homieiot.github.io/specification/#device-lifecycle) of the Homie
    /// device to 'alert', to indicate that something wrong is happening and manual intervention may
    /// be required. This should be only be called after `ready()`, otherwise it will panic.
    pub async fn alert(&mut self) -> Result<(), ClientError> {
        let mut state = self.state.lock().await;
        assert_eq!(state.state, State::Ready);
        state.set_state(State::Alert).await
    }

    /// Disconnect cleanly from the MQTT broker, after updating the state of the Homie device to
    // 'disconnected'.
    pub async fn disconnect(self) -> Result<(), ClientError> {
        let mut state = self.state.lock().await;
        state.set_state(State::Disconnected).await?;
        state.publisher.client.disconnect().await
    }

    /// Publish a new value for the given property of the given node of this device. The caller is
    /// responsible for ensuring that the value is of the correct type.
    pub async fn publish_value(
        &self,
        node_id: &str,
        property_id: &str,
        value: impl ToString,
    ) -> Result<(), ClientError> {
        // TODO: If we are disconnected, just keep track of the latest value for each property to
        // publish after reconnecting, rather than queuing these all up.
        self.state
            .lock()
            .await
            .publisher
            .publish_retained(&format!("{}/{}", node_id, property_id), value.to_string())
            .await
    }
}

/// The internal state of a `HomieDevice`, so it can be shared between threads.
#[derive(Debug)]
struct DeviceState {
    publisher: DevicePublisher,
    device_name: String,
    nodes: Vec<Node>,
    state: State,
    extension_ids: String,
}

impl DeviceState {
    /// Publish the current list of node IDs.
    async fn publish_nodes(&self) -> Result<(), ClientError> {
        let node_ids = self
            .nodes
            .iter()
            .map(|node| node.id.as_str())
            .collect::<Vec<&str>>()
            .join(",");
        self.publisher.publish_retained("$nodes", node_ids).await
    }

    /// Set the state to the given value, and publish it.
    async fn set_state(&mut self, state: State) -> Result<(), ClientError> {
        self.state = state;
        self.send_state().await
    }

    /// Publish the current state.
    async fn send_state(&self) -> Result<(), ClientError> {
        self.publisher.publish_retained("$state", self.state).await
    }

    async fn republish_all(&self) -> Result<(), ClientError> {
        for node in &self.nodes {
            self.publisher.publish_node(node).await?;
        }
        self.publish_nodes().await?;
        // TODO: Stats and firmware extensions
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
        self.send_state().await
    }

    /// Add a node to the Homie device and publish it.
    async fn add_node(&mut self, node: Node) -> Result<(), ClientError> {
        // First check that there isn't already a node with the same ID.
        if self.nodes.iter().any(|n| n.id == node.id) {
            panic!("Tried to add node with duplicate ID: {:?}", node);
        }
        self.nodes.push(node);
        // `node` was moved into the `nodes` vector, but we can safely get a reference to it because
        // nothing else can modify `nodes` in the meantime.
        let node = &self.nodes[self.nodes.len() - 1];

        self.publisher.publish_node(&node).await?;
        self.publish_nodes().await
    }

    /// Remove the node with the given ID.
    async fn remove_node(&mut self, node_id: &str) -> Result<(), ClientError> {
        // Panic on attempt to remove a node which was never added.
        let index = self.nodes.iter().position(|n| n.id == node_id).unwrap();
        self.publisher.unpublish_node(&self.nodes[index]).await?;
        self.nodes.remove(index);
        self.publish_nodes().await
    }
}

#[derive(Clone, Debug)]
struct DevicePublisher {
    pub client: AsyncClient,
    device_base: String,
}

impl DevicePublisher {
    fn new(client: AsyncClient, device_base: String) -> Self {
        Self {
            client,
            device_base,
        }
    }

    async fn publish_retained(
        &self,
        subtopic: &str,
        value: impl Into<Vec<u8>>,
    ) -> Result<(), ClientError> {
        let topic = format!("{}/{}", self.device_base, subtopic);
        self.client
            .publish(topic, QoS::AtLeastOnce, true, value)
            .await
    }

    async fn subscribe(&self, subtopic: &str) -> Result<(), ClientError> {
        let topic = format!("{}/{}", self.device_base, subtopic);
        self.client.subscribe(topic, QoS::AtLeastOnce).await
    }

    async fn unsubscribe(&self, subtopic: &str) -> Result<(), ClientError> {
        let topic = format!("{}/{}", self.device_base, subtopic);
        self.client.unsubscribe(topic).await
    }

    /// Publish metadata about the given node and its properties.
    async fn publish_node(&self, node: &Node) -> Result<(), ClientError> {
        self.publish_retained(&format!("{}/$name", node.id), node.name.as_str())
            .await?;
        self.publish_retained(&format!("{}/$type", node.id), node.node_type.as_str())
            .await?;
        let mut property_ids: Vec<&str> = vec![];
        for property in &node.properties {
            property_ids.push(&property.id);
            self.publish_retained(
                &format!("{}/{}/$name", node.id, property.id),
                property.name.as_str(),
            )
            .await?;
            self.publish_retained(
                &format!("{}/{}/$datatype", node.id, property.id),
                property.datatype,
            )
            .await?;
            self.publish_retained(
                &format!("{}/{}/$settable", node.id, property.id),
                if property.settable { "true" } else { "false" },
            )
            .await?;
            if let Some(unit) = &property.unit {
                self.publish_retained(&format!("{}/{}/$unit", node.id, property.id), unit.as_str())
                    .await?;
            }
            if let Some(format) = &property.format {
                self.publish_retained(
                    &format!("{}/{}/$format", node.id, property.id),
                    format.as_str(),
                )
                .await?;
            }
            if property.settable {
                self.subscribe(&format!("{}/{}/set", node.id, property.id))
                    .await?;
            }
        }
        self.publish_retained(&format!("{}/$properties", node.id), property_ids.join(","))
            .await?;
        Ok(())
    }

    async fn unpublish_node(&self, node: &Node) -> Result<(), ClientError> {
        for property in &node.properties {
            if property.settable {
                self.unsubscribe(&format!("{}/{}/set", node.id, property.id))
                    .await?;
            }
        }
        Ok(())
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
    async fn start(&self) -> Result<(), ClientError> {
        self.publisher
            .publish_retained("$stats/interval", STATS_INTERVAL.as_secs().to_string())
            .await
    }

    /// Periodically send stats.
    fn spawn(self) -> impl Future<Output = Result<(), SpawnError>> {
        let task: JoinHandle<Result<(), SpawnError>> = task::spawn(async move {
            loop {
                // TODO: Break out of this loop if disconnection is requested.
                let uptime = Instant::now() - self.start_time;
                self.publisher
                    .publish_retained("$stats/uptime", uptime.as_secs().to_string())
                    .await?;
                sleep(STATS_INTERVAL).await;
            }
        });
        task.map(|res| Ok(res??))
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
    async fn start(&self) -> Result<(), ClientError> {
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

fn try_join_unit_handles<E>(
    a: JoinHandle<Result<(), E>>,
    b: JoinHandle<Result<(), E>>,
) -> impl Future<Output = Result<(), E>>
where
    E: From<JoinError>,
{
    try_join_handles(a, b).map(simplify_unit_pair)
}

fn simplify_unit_pair<E>(m: Result<((), ()), E>) -> Result<(), E> {
    m.map(|((), ())| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_channel::Receiver;
    use rumqttc::Request;

    fn make_test_device() -> (HomieDevice, Receiver<Request>) {
        let (requests_tx, requests_rx) = async_channel::unbounded();
        let (cancel_tx, _cancel_rx) = async_channel::unbounded();
        let client = AsyncClient::from_senders(requests_tx, cancel_tx);
        let publisher = DevicePublisher::new(client, "homie/test-device".to_string());
        let device = HomieDevice::new(
            publisher,
            "Test device".to_string(),
            &[],
            DEFAULT_RECONNECT_INTERVAL,
        );
        (device, requests_rx)
    }

    #[tokio::test]
    #[should_panic(expected = "Tried to add node with duplicate ID")]
    async fn add_node_fails_given_duplicate_id() {
        let (mut device, rx) = make_test_device();

        device
            .add_node(Node::new("id", "Name", "type", vec![]))
            .await
            .unwrap();
        device
            .add_node(Node::new("id", "Name 2", "type2", vec![]))
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
    async fn start_succeeds_with_no_nodes() -> Result<(), ClientError> {
        let (mut device, rx) = make_test_device();

        device.start().await?;
        device.ready().await?;

        // Need to keep rx alive until here so that the channel isn't closed.
        drop(rx);
        Ok(())
    }

    #[tokio::test]
    async fn sleep_then_ready_again_succeeds() -> Result<(), ClientError> {
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
    async fn alert_then_ready_again_succeeds() -> Result<(), ClientError> {
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
    async fn disconnect_succeeds_before_ready() -> Result<(), ClientError> {
        let (mut device, rx) = make_test_device();

        device.start().await?;
        device.disconnect().await?;

        // Need to keep rx alive until here so that the channel isn't closed.
        drop(rx);
        Ok(())
    }

    #[tokio::test]
    async fn disconnect_succeeds_after_ready() -> Result<(), ClientError> {
        let (mut device, rx) = make_test_device();

        device.start().await?;
        device.ready().await?;
        device.disconnect().await?;

        // Need to keep rx alive until here so that the channel isn't closed.
        drop(rx);
        Ok(())
    }

    #[tokio::test]
    async fn minimal_build_succeeds() -> Result<(), ClientError> {
        let builder = HomieDevice::builder(
            "homie/test-device",
            "Test device",
            MqttOptions::new("client_id", "hostname", 1234),
        );

        let (_event_loop, homie, _stats, firmware, _callback) = builder.build();

        let state = homie.state.lock().await;
        assert_eq!(state.device_name, "Test device");
        assert_eq!(state.publisher.device_base, "homie/test-device");
        assert!(firmware.is_none());

        Ok(())
    }

    #[tokio::test]
    async fn set_firmware_build_succeeds() -> Result<(), ClientError> {
        let mut builder = HomieDevice::builder(
            "homie/test-device",
            "Test device",
            MqttOptions::new("client_id", "hostname", 1234),
        );

        builder.set_firmware("firmware_name", "firmware_version");

        let (_event_loop, homie, _stats, firmware, _callback) = builder.build();

        let state = homie.state.lock().await;
        assert_eq!(state.device_name, "Test device");
        assert_eq!(state.publisher.device_base, "homie/test-device");
        let firmware = firmware.unwrap();
        assert_eq!(firmware.firmware_name, "firmware_name");
        assert_eq!(firmware.firmware_version, "firmware_version");

        Ok(())
    }

    #[tokio::test]
    async fn add_node_succeeds_before_and_after_start() -> Result<(), ClientError> {
        let (mut device, rx) = make_test_device();

        device
            .add_node(Node::new("id", "Name", "type", vec![]))
            .await?;

        device.start().await?;
        device.ready().await?;

        // Add another node after starting.
        device
            .add_node(Node::new("id2", "Name 2", "type2", vec![]))
            .await?;

        // Need to keep rx alive until here so that the channel isn't closed.
        drop(rx);
        Ok(())
    }

    /// Add a node, remove it, and add it back again.
    #[tokio::test]
    async fn add_node_succeeds_after_remove() -> Result<(), ClientError> {
        let (mut device, rx) = make_test_device();

        device
            .add_node(Node::new("id", "Name", "type", vec![]))
            .await?;

        device.remove_node("id").await?;

        // Adding it back shouldn't give an error.
        device
            .add_node(Node::new("id", "Name", "type", vec![]))
            .await?;

        // Need to keep rx alive until here so that the channel isn't closed.
        drop(rx);
        Ok(())
    }
}
