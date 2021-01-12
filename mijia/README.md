# Mijia sensor library

[![crates.io page](https://img.shields.io/crates/v/mijia.svg)](https://crates.io/crates/mijia)
[![docs.rs page](https://docs.rs/mijia/badge.svg)](https://docs.rs/mijia)

A library for connecting to Xiaomi Mijia 2 Bluetooth temperature/humidity sensors.

Currently only supports running on Linux, as it depends on BlueZ for Bluetooth.

## Usage

```rust
// Create a new session. This establishes the D-Bus connection. In this case we ignore the join
// handle, as we don't intend to run indefinitely.
let (_, session) = MijiaSession::new().await?;

// Start scanning for Bluetooth devices, and wait a few seconds for some to be discovered.
session.bt_session.start_discovery().await?;
time::sleep(Duration::from_secs(5)).await;

// Get the list of sensors which are currently known.
let sensors = session.get_sensors().await?;

for sensor in sensors {
    // Connect to the sensor
    session.bt_session.connect(&sensor.id).await?;

    // Print some properties of the sensor.
    let sensor_time: DateTime<Utc> = session.get_time(&sensor.id).await?.into();
    let temperature_unit = session.get_temperature_unit(&sensor.id).await?;
    let comfort_level = session.get_comfort_level(&sensor.id).await?;
    println!(
        "Time: {}, Unit: {}, Comfort level: {}",
        sensor_time, temperature_unit, comfort_level
    );

    // Subscribe to readings from the sensor.
    session.start_notify_sensor(&sensor.id).await?;
}

// Print readings from all the sensors we subscribed to.
let mut events = session.event_stream().await?;
while let Some(event) = events.next().await {
    if let MijiaEvent::Readings { id, readings } = event {
        println!("{}: {}", id, readings);
    }
}
```

For some more complete examples, see the [examples](examples/) directory.

## License

Licensed under either of

- [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
- [MIT license](http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the
work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
