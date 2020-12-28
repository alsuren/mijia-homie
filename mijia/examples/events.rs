use futures::stream::StreamExt;
use mijia::bluetooth::BluetoothSession;

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
