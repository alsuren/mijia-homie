//! Integration test between homie-controller and homie-device crates. Starts a device and a
//! controller connected to the same MQTT broker, and ensures that the controller can discover the
//! device as expected.

use homie_controller::{HomieController, State};
use homie_device::{HomieDevice, Node, Property, SpawnError};
use librumqttd::{Broker, Config};
use rumqttc::{ConnectionError, MqttOptions, StateError};
use std::io::ErrorKind;
use std::thread;

// A high port number which is hopefully not in use, to use for the MQTT broker.
const PORT: u16 = 10883;

#[tokio::test]
async fn test_device() {
    pretty_env_logger::init();

    // Start MQTT broker.
    spawn_mqtt_broker(PORT);

    // Start controller.
    let controller_options = MqttOptions::new("homie_controller", "localhost", PORT);
    let (controller, mut event_loop) = HomieController::new(controller_options, "homie");
    controller.start().await.unwrap();

    // Start device
    let device_options = MqttOptions::new("homie_device", "localhost", PORT);
    let (mut homie, homie_handle) =
        HomieDevice::builder("homie/device_id", "Device name", device_options)
            .spawn()
            .await
            .unwrap();
    let node = Node::new(
        "node_id",
        "Node name",
        "node_type",
        vec![Property::boolean(
            "property_id",
            "Property name",
            true,
            None,
        )],
    );
    homie.add_node(node).await.unwrap();
    homie.ready().await.unwrap();

    // Wait until the controller knows about all required attributes of the device.
    loop {
        if let Some(event) = controller.poll(&mut event_loop).await.unwrap() {
            log::trace!("Event: {:?}", event);
            let devices = controller.devices();
            if let Some(device) = devices.get("device_id") {
                if device.state == State::Ready
                    && device.has_required_attributes()
                    && device.nodes.len() == 1
                {
                    break;
                }
            }
        }
    }

    // Check that the device looks how we expect.
    let devices = controller.devices();
    let device = devices.get("device_id").unwrap();
    log::info!("Device: {:?}", device);
    assert_eq!(device.name, Some("Device name".to_string()));
    assert_eq!(device.homie_version, "4.0");
    assert_eq!(device.state, State::Ready);
    assert_eq!(device.nodes.len(), 1);
    let node = device.nodes.get("node_id").unwrap();
    assert_eq!(node.name, Some("Node name".to_string()));
    assert_eq!(node.node_type, Some("node_type".to_string()));
    assert_eq!(node.properties.len(), 1);
    let property = node.properties.get("property_id").unwrap();
    assert_eq!(property.name, Some("Property name".to_string()));

    // Disconnect the device.
    homie.disconnect().await.unwrap();
    let err = homie_handle.await.unwrap_err();
    if let SpawnError::Connection(ConnectionError::MqttState(StateError::Io(e))) = err {
        assert_eq!(e.kind(), ErrorKind::ConnectionAborted);
    } else {
        panic!("Unexpected error {:?}", err);
    }
}

/// Spawn an MQTT broker listening on the given port.
fn spawn_mqtt_broker(port: u16) {
    // TODO: Construct Config directly once fields are made public.
    let broker_config = toml::from_str::<Config>(&format!(
        r#"
    id = 0

    [router]
    id = 0
    dir = "/tmp/integration/rumqttd"
    max_segment_size = 10240
    max_segment_count = 10
    max_connections = 20

    [servers.1]
    port = {}
    next_connection_delay_ms = 1

    [servers.1.connections]
    connection_timeout_ms = 100
    max_client_id_len = 100
    throttle_delay_ms = 0
    max_payload_size = 2048
    max_inflight_count = 500
    max_inflight_size = 1024

    [console]
    port = 13030
    "#,
        port
    ))
    .unwrap();
    let mut broker = Broker::new(broker_config);
    thread::spawn(move || {
        broker.start().unwrap();
    });
}
