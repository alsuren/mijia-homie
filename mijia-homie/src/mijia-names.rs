//! Example of how to subscribe to readings from one or more sensors.

use eyre::Report;
use eyre::WrapErr;
use mijia::bluetooth::MacAddress;
use mijia::{MijiaSession, SensorProps};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::stdin;
use std::io::Write;
use std::str::FromStr;
use std::time::Duration;
use tokio::time;

const SCAN_DURATION: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> Result<(), Report> {
    pretty_env_logger::init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eyre::bail!("USAGE: {} /path/to/sensor-names.toml", args[0]);
    }
    let sensor_names_filename = args.get(1).map(|s| s.to_owned()).unwrap();

    let excludes = get_known_sensors(&sensor_names_filename)?;
    println!("ignoring sensors: {:?}", excludes);

    let (_, session) = MijiaSession::new().await?;

    // Start scanning for Bluetooth devices, and wait a while for some to be discovered.
    session.bt_session.start_discovery().await?;
    time::sleep(SCAN_DURATION).await;

    // Get the list of sensors which are currently visible, connect those which
    // are not already named.
    let sensors = session.get_sensors().await?;
    for sensor in sensors
        .iter()
        .filter(|sensor| should_include_sensor(sensor, &excludes))
    {
        println!("Connecting to {}", sensor.mac_address);
        if let Err(e) = session.bt_session.connect(&sensor.id).await {
            println!("Failed to connect to {}", sensor.mac_address);
            log::debug!("error was: {:?}", e);

            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&sensor_names_filename)?;
            writeln!(file, r#""{mac}" = "failed""#, mac = sensor.mac_address,)?;
            continue;
        }

        {
            let sensor = sensor.clone();
            let sensor_names_filename = sensor_names_filename.clone();

            tokio::task::spawn_blocking(move || {
                println!(
                    "Successfully connected to {} (it should now have a bluetooth icon on it)",
                    sensor.mac_address
                );
                println!("What name do you want to use for {}?", sensor.mac_address);
                let mut name = String::new();
                stdin().read_line(&mut name)?;
                let name = name.trim_end();

                let mut file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&sensor_names_filename)?;
                writeln!(
                    file,
                    r#""{mac}" = "{name}""#,
                    mac = sensor.mac_address,
                    name = name
                )?;
                println!(
                    r#"written: "{mac}" = "{name}""#,
                    mac = sensor.mac_address,
                    name = name
                );
                Ok::<_, Report>(())
            })
            .await??;
        }

        if let Err(e) = session.bt_session.disconnect(&sensor.id).await {
            log::error!("disconnecting failed: {:?}", e);
        }
    }

    println!(
        "Finished inspecting all known sensors.\n\n\
        Some sensors may have been discovered while we were trying to inspect \
        this batch. Re-run the program to also inspect these new sensors."
    );

    Ok(())
}

fn get_known_sensors(sensor_names_filename: &str) -> Result<Vec<MacAddress>, Report> {
    let sensor_names_contents = std::fs::read_to_string(sensor_names_filename)
        .wrap_err_with(|| format!("Reading {}", sensor_names_filename))?;
    let names = toml::from_str::<HashMap<String, String>>(&sensor_names_contents)?;

    let res: Result<Vec<_>, _> = names.keys().map(|s| MacAddress::from_str(s)).collect();
    Ok(res?)
}

fn should_include_sensor(sensor: &SensorProps, excludes: &Vec<MacAddress>) -> bool {
    !excludes.contains(&sensor.mac_address)
}
