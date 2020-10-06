use futures::{FutureExt, TryFutureExt};
use homie_device::{Datatype, HomieDevice, Node, Property};
use rand::random;
use rumqttc::MqttOptions;
use std::error::Error;
use std::time::Duration;
use tokio::task::{self, JoinHandle};
use tokio::{time, try_join};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    pretty_env_logger::init();

    let mqttoptions = MqttOptions::new("homie_example", "test.mosquitto.org", 1883);

    let (mut homie, homie_handle) =
        HomieDevice::builder("homie/example_sensor", "Homie sensor example", mqttoptions)
            .spawn()
            .await?;

    homie
        .add_node(Node::new(
            "sensor",
            "Sensor",
            "Environment sensor",
            vec![
                Property::new(
                    "temperature",
                    "Temperature",
                    Datatype::Float,
                    false,
                    Some("ºC"),
                    None,
                ),
                Property::new(
                    "humidity",
                    "Humidity",
                    Datatype::Integer,
                    false,
                    Some("%"),
                    None,
                ),
            ],
        ))
        .await?;

    let handle: JoinHandle<Result<(), Box<dyn Error + Send + Sync>>> = task::spawn(async move {
        homie.ready().await?;
        println!("Ready");

        loop {
            let temperature: f32 = random::<f32>() * 40.0;
            let humidity: u8 = (random::<f32>() * 100.0) as u8;
            println!("Update: {}ºC {}%", temperature, humidity);
            homie
                .publish_value("sensor", "temperature", temperature)
                .await?;
            homie.publish_value("sensor", "humidity", humidity).await?;

            time::delay_for(Duration::from_secs(10)).await;
        }
    });

    // Poll everything to completion, until the first one bombs out.
    let res: Result<_, Box<dyn Error + Send + Sync>> = try_join! {
        homie_handle.err_into(),
        handle.map(|res| Ok(res??)),
    };
    res?;
    Ok(())
}
