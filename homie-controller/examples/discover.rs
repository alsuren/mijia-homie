use homie_controller::{HomieController, SpawnError};
use rumqttc::MqttOptions;

#[tokio::main(core_threads = 2)]
async fn main() -> Result<(), SpawnError> {
    pretty_env_logger::init();

    let mqttoptions = MqttOptions::new("homie_controller", "test.mosquitto.org", 1883);

    let (controller, event_loop) = HomieController::new(mqttoptions, "homie");
    let handle = controller.spawn(event_loop);
    controller.start().await?;

    println!("Ready");

    // This will only resolve (with an error) if we lose connection to the MQTT broker.
    handle.await
}
