use homie_controller::{HomieController, PollError};
use rumqttc::MqttOptions;

#[tokio::main(core_threads = 2)]
async fn main() -> Result<(), PollError> {
    pretty_env_logger::init();

    let mqttoptions = MqttOptions::new("homie_controller", "test.mosquitto.org", 1883);

    let (mut controller, mut event_loop) = HomieController::new(mqttoptions, "homie");
    controller.start().await?;
    loop {
        controller.poll(&mut event_loop).await?;
        println!("Devices:");
        for device in controller.devices.values() {
            if device.has_required_attributes() {
                println!(" * {:?}", device);
            } else {
                println!(" * {} not ready.", device.id);
            }
        }
    }
}
