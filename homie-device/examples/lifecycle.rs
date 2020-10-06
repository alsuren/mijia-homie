use futures::{FutureExt, TryFutureExt};
use homie_device::HomieDevice;
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
        HomieDevice::builder("homie/example_lifecycle", "Homie lifecycle example", mqttoptions)
            .spawn()
            .await?;

    let handle: JoinHandle<Result<(), Box<dyn Error + Send + Sync>>> = task::spawn(async move {
        println!("init");

        time::delay_for(Duration::from_secs(5)).await;
        homie.ready().await?;
        println!("ready");

        time::delay_for(Duration::from_secs(5)).await;
        homie.sleep().await?;
        println!("sleeping");

        time::delay_for(Duration::from_secs(5)).await;
        homie.ready().await?;
        println!("ready");

        time::delay_for(Duration::from_secs(5)).await;
        homie.alert().await?;
        println!("alert");

        time::delay_for(Duration::from_secs(5)).await;
        homie.ready().await?;
        println!("ready");

        time::delay_for(Duration::from_secs(5)).await;
        homie.disconnect().await?;
        println!("disconnected");
        Ok(())
    });

    // Poll everything to completion, until the first one bombs out.
    let res: Result<_, Box<dyn Error + Send + Sync>> = try_join! {
        homie_handle.err_into(),
        handle.map(|res| Ok(res??)),
    };
    res?;
    Ok(())
}
