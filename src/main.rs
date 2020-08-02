use rumqttc::{EventLoop, MqttOptions, Request};
use std::error::Error;
use std::time::Duration;

#[tokio::main(core_threads = 1)]
async fn main() {
    let mut mqttoptions = MqttOptions::new("rumqtt-async", "test.mosquitto.org", 1883);
    let requests_rx = tokio::stream::iter(Vec::new());
    let mut eventloop = EventLoop::new(mqttoptions, requests_rx).await;

    loop {
        let notification = eventloop.poll().await.unwrap();
        println!("Received = {:?}", notification);
        tokio::time::delay_for(Duration::from_secs(1)).await;
    }
}
