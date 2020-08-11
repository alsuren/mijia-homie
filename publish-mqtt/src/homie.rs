use async_channel::{SendError, Sender};
use rumqttc::{self, EventLoop, LastWill, MqttOptions, Publish, QoS, Request};
use std::error::Error;
use tokio::task::{self, JoinHandle};

const HOMIE_VERSION: &str = "4.0";

pub struct Sensor {
    id: String,
    name: String,
}

impl Sensor {
    pub fn new(id: String, name: String) -> Sensor {
        Sensor { id, name }
    }
}

pub(crate) struct HomieDevice {
    requests_tx: Sender<Request>,
    device_base: String,
    device_name: String,
    sensors: Vec<Sensor>,
}

impl HomieDevice {
    pub async fn new(
        device_base: &str,
        device_name: &str,
        mut mqtt_options: MqttOptions,
    ) -> (
        HomieDevice,
        JoinHandle<Result<(), Box<dyn Error + Send + Sync>>>,
    ) {
        mqtt_options.set_last_will(LastWill {
            topic: format!("{}/$state", device_base),
            message: "lost".to_string(),
            qos: QoS::AtLeastOnce,
            retain: true,
        });
        let mut event_loop = EventLoop::new(mqtt_options, 10).await;

        let homie = HomieDevice {
            requests_tx: event_loop.handle(),
            device_base: device_base.to_string(),
            device_name: device_name.to_string(),
            sensors: vec![],
        };

        let join_handle: JoinHandle<Result<(), Box<dyn Error + Send + Sync>>> =
            task::spawn(async move {
                loop {
                    let (incoming, outgoing) = event_loop.poll().await?;
                    log::trace!("Incoming = {:?}, Outgoing = {:?}", incoming, outgoing);
                }
            });

        (homie, join_handle)
    }

    pub async fn start(&self) -> Result<(), SendError<Request>> {
        publish_retained(
            &self.requests_tx,
            format!("{}/$homie", self.device_base),
            HOMIE_VERSION,
        )
        .await?;
        publish_retained(
            &self.requests_tx,
            format!("{}/$extensions", self.device_base),
            "",
        )
        .await?;
        publish_retained(
            &self.requests_tx,
            format!("{}/$name", self.device_base),
            &self.device_name,
        )
        .await?;
        publish_retained(
            &self.requests_tx,
            format!("{}/$state", self.device_base),
            "init",
        )
        .await?;
        Ok(())
    }

    pub fn add_node(&mut self, sensor: Sensor) {
        self.sensors.push(sensor);
    }

    pub async fn publish_nodes(&self) -> Result<(), SendError<Request>> {
        let mut nodes: Vec<&str> = vec![];
        for sensor in &self.sensors {
            let node_base = format!("{}/{}", self.device_base, sensor.id);
            nodes.push(&sensor.id);
            publish_retained(
                &self.requests_tx,
                format!("{}/$name", node_base),
                &sensor.name,
            )
            .await?;
            publish_retained(
                &self.requests_tx,
                format!("{}/$type", node_base),
                "Mijia sensor",
            )
            .await?;
            publish_retained(
                &self.requests_tx,
                format!("{}/$properties", node_base),
                "temperature,humidity,battery",
            )
            .await?;
            publish_retained(
                &self.requests_tx,
                format!("{}/temperature/$name", node_base),
                "Temperature",
            )
            .await?;
            publish_retained(
                &self.requests_tx,
                format!("{}/temperature/$datatype", node_base),
                "float",
            )
            .await?;
            publish_retained(
                &self.requests_tx,
                format!("{}/temperature/$unit", node_base),
                "ºC",
            )
            .await?;
            publish_retained(
                &self.requests_tx,
                format!("{}/humidity/$name", node_base),
                "Humidity",
            )
            .await?;
            publish_retained(
                &self.requests_tx,
                format!("{}/humidity/$datatype", node_base),
                "integer",
            )
            .await?;
            publish_retained(
                &self.requests_tx,
                format!("{}/humidity/$unit", node_base),
                "%",
            )
            .await?;
            publish_retained(
                &self.requests_tx,
                format!("{}/battery/$name", node_base),
                "Battery level",
            )
            .await?;
            publish_retained(
                &self.requests_tx,
                format!("{}/battery/$datatype", node_base),
                "integer",
            )
            .await?;
            publish_retained(
                &self.requests_tx,
                format!("{}/battery/$unit", node_base),
                "%",
            )
            .await?;
        }
        publish_retained(
            &self.requests_tx,
            format!("{}/$nodes", self.device_base),
            &nodes.join(","),
        )
        .await?;
        publish_retained(
            &self.requests_tx,
            format!("{}/$state", self.device_base),
            "ready",
        )
        .await?;
        Ok(())
    }

    pub async fn publish_values(
        &self,
        node_id: &str,
        temperature: f32,
        humidity: u8,
        battery_percent: u16,
    ) -> Result<(), SendError<Request>> {
        let node_base = format!("{}/{}", self.device_base, node_id);
        publish_retained(
            &self.requests_tx,
            format!("{}/temperature", node_base),
            &format!("{:.2}", temperature),
        )
        .await?;
        publish_retained(
            &self.requests_tx,
            format!("{}/humidity", node_base),
            &humidity.to_string(),
        )
        .await?;
        publish_retained(
            &self.requests_tx,
            format!("{}/battery", node_base),
            &battery_percent.to_string(),
        )
        .await?;
        Ok(())
    }
}

async fn publish_retained(
    requests_tx: &Sender<Request>,
    name: String,
    value: &str,
) -> Result<(), SendError<Request>> {
    let mut publish = Publish::new(name, QoS::AtLeastOnce, value);
    publish.set_retain(true);
    requests_tx.send(publish.into()).await
}
