use bluez_async::BluetoothSession;
use futures::stream::StreamExt;

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    pretty_env_logger::init();

    let (_, session) = BluetoothSession::new().await?;
    let mut events = session.event_stream().await?;

    println!("Events:");
    while let Some(event) = events.next().await {
        println!("{:?}", event);
    }

    Ok(())
}
