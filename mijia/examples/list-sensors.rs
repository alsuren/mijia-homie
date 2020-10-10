use mijia::MijiaSession;

#[tokio::main]
async fn main() -> Result<(), eyre::Error> {
    pretty_env_logger::init();

    let (_, session) = MijiaSession::new().await?;

    session.bt_session.start_discovery().await?;

    let sensors = session.get_sensors().await?;
    println!("Sensors:");
    for sensor in sensors {
        println!("{}: {:?}", sensor.mac_address, sensor.id);
    }

    Ok(())
}
