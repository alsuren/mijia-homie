use futures::FutureExt;
use homie::HomieDevice;
use rumqttc::MqttOptions;
use std::error::Error;
use std::time::Duration;
use tokio::task::{self, JoinHandle};
use tokio::{time, try_join};

#[tokio::main(core_threads = 2)]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut mqttoptions = MqttOptions::new("homie_example", "test.mosquitto.org", 1883);
    mqttoptions.set_keep_alive(5);

    let (mut homie, homie_handle) =
        HomieDevice::builder("homie/example", "Homie example", mqttoptions)
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
        homie_handle,
        handle.map(|res| Ok(res??)),
    };
    res?;
    Ok(())
}
