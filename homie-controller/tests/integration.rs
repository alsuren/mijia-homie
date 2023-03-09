//! Integration test between homie-controller and homie-device crates. Starts a device and a
//! controller connected to the same MQTT broker, and ensures that the controller can discover the
//! device as expected.

use futures::future::ready;
use homie_controller::{Event, HomieController, State};
use homie_device::{HomieDevice, Node, Property, SpawnError};
use rumqttc::{ConnectionError, MqttOptions, StateError};
use rumqttd::{Broker, Config, ConnectionSettings, RouterConfig, ServerSettings};
use std::collections::HashMap;
use std::env;
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::mpsc;
use std::thread;

// A high port number which is hopefully not in use, to use for the MQTT broker.
const PORT: u16 = 10883;

#[tokio::test]
async fn test_device() {
    let mut log_builder = pretty_env_logger::formatted_builder();
    if let Ok(s) = env::var("RUST_LOG") {
        log_builder.parse_filters(&s);
    }
    let _ = log_builder.is_test(true).try_init();

    // Start MQTT broker.
    spawn_mqtt_broker(PORT);

    // Start controller.
    let controller_options = MqttOptions::new("homie_controller", "localhost", PORT);
    let (controller, mut event_loop) = HomieController::new(controller_options, "homie");

    // Start device
    let (updates_tx, updates_rx) = mpsc::sync_channel(10);
    let device_options = MqttOptions::new("homie_device", "localhost", PORT);
    let mut device_builder = HomieDevice::builder("homie/device_id", "Device name", device_options);
    device_builder.set_update_callback(move |node_id, property_id, value| {
        assert_eq!(property_id, "property_id");
        assert_eq!(node_id, "node_id");
        updates_tx.send(value.clone()).unwrap();
        ready(Some(value))
    });
    let (mut homie, homie_handle) = device_builder.spawn().await.unwrap();
    let node = Node::new(
        "node_id",
        "Node name",
        "node_type",
        vec![Property::integer(
            "property_id",
            "Property name",
            true,
            true,
            Some("unit"),
            None,
        )],
    );
    homie.add_node(node).await.unwrap();
    homie.ready().await.unwrap();

    // Wait until the controller knows about all required attributes of the device.
    'outer0: loop {
        for event in controller.poll(&mut event_loop).await.unwrap() {
            log::trace!("Event: {:?}", event);
            let devices = controller.devices();
            if let Some(device) = devices.get("device_id") {
                // For some reason we get the ready state before all the attributes of the property
                // have been filled in, so we need to explicitly check for the unit being set.
                if device.state == State::Ready
                    && device.has_required_attributes()
                    && device.nodes.len() == 1
                    && device
                        .nodes
                        .get("node_id")
                        .unwrap()
                        .properties
                        .get("property_id")
                        .unwrap()
                        .unit
                        .is_some()
                {
                    break 'outer0;
                }
            }
        }
    }

    // Check that the device looks how we expect.
    {
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
        assert_eq!(property.settable, true);
        assert_eq!(property.unit, Some("unit".to_string()));
        assert_eq!(property.value, None);
    }

    // Send a value from the device to the controller.
    homie
        .publish_value("node_id", "property_id", 42)
        .await
        .unwrap();

    // Wait until the controller receives the value.
    'outer1: loop {
        for event in controller.poll(&mut event_loop).await.unwrap() {
            log::trace!("Event: {:?}", event);
            if let Event::PropertyValueChanged {
                device_id,
                node_id,
                property_id,
                value,
                fresh,
            } = event
            {
                assert_eq!(device_id, "device_id");
                assert_eq!(node_id, "node_id");
                assert_eq!(property_id, "property_id");
                assert_eq!(value, "42");
                assert_eq!(fresh, true);
                break 'outer1;
            }
        }
    }

    // Check that the device looks how we expect.
    {
        let devices = controller.devices();
        let device = devices.get("device_id").unwrap();
        let node = device.nodes.get("node_id").unwrap();
        let property = node.properties.get("property_id").unwrap();
        log::info!("Property: {:?}", property);
        assert_eq!(property.value(), Ok(42));
    }

    // Send a value from the controller to the device.
    controller
        .set("device_id", "node_id", "property_id", 13)
        .await
        .unwrap();

    // Wait for the device to receive the value and send it back to the controller.
    'outer2: loop {
        for event in controller.poll(&mut event_loop).await.unwrap() {
            log::trace!("Event: {:?}", event);
            if let Event::PropertyValueChanged {
                device_id,
                node_id,
                property_id,
                value,
                fresh,
            } = event
            {
                assert_eq!(device_id, "device_id");
                assert_eq!(node_id, "node_id");
                assert_eq!(property_id, "property_id");
                assert_eq!(value, "13");
                assert_eq!(fresh, true);
                break 'outer2;
            }
        }
    }
    assert_eq!(updates_rx.try_iter().collect::<Vec<_>>(), vec!["13"]);

    // Check that the value sent back is reflected on the controller's view of the device.
    {
        let devices = controller.devices();
        let device = devices.get("device_id").unwrap();
        let node = device.nodes.get("node_id").unwrap();
        let property = node.properties.get("property_id").unwrap();
        log::info!("Property: {:?}", property);
        assert_eq!(property.value(), Ok(13));
    }

    // Disconnect the device.
    {
        homie.disconnect().await.unwrap();
        let err = homie_handle.await.unwrap_err();
        if let SpawnError::Connection(ConnectionError::MqttState(StateError::Io(e))) = err {
            assert_eq!(e.kind(), ErrorKind::ConnectionAborted);
        } else {
            panic!("Unexpected error {:?}", err);
        }
    }

    // Disconnect the controller.
    controller.disconnect().await.unwrap();
    while let Ok(events) = controller.poll(&mut event_loop).await {
        for event in events {
            log::trace!("Event: {:?}", event);
        }
    }
}

/// Spawn an MQTT broker listening on the given port on localhost.
fn spawn_mqtt_broker(port: u16) {
    let mut v4 = HashMap::new();
    v4.insert(
        "1".to_string(),
        ServerSettings {
            name: "Test broker".to_string(),
            listen: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
            tls: None,
            next_connection_delay_ms: 1,
            connections: ConnectionSettings {
                connection_timeout_ms: 100,
                throttle_delay_ms: 0,
                max_payload_size: 2048,
                max_inflight_count: 500,
                max_inflight_size: 1024,
                auth: None,
                dynamic_filters: false,
            },
        },
    );
    let broker_config = Config {
        id: 0,
        router: RouterConfig {
            instant_ack: false,
            max_segment_size: 2000,
            max_segment_count: 10,
            max_read_len: 2000,
            max_connections: 20,
            initialized_filters: None,
        },
        v4,
        ..Config::default()
    };
    let mut broker = Broker::new(broker_config);
    thread::spawn(move || {
        broker.start().expect(&format!(
            "Failed to start MQTT broker. This may be because port {} is already in use",
            port,
        ));
    });
}
