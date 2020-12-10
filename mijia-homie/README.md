# Mijia sensor to Homie bridge

[![crates.io page](https://img.shields.io/crates/v/mijia-homie.svg)](https://crates.io/crates/mijia-homie)
[![Download](https://api.bintray.com/packages/homie-rs/homie-rs/mijia-homie/images/download.svg) ](https://bintray.com/homie-rs/homie-rs/mijia-homie/_latestVersion)

`mijia-homie` is a service for connecting to Xiaomi Mijia 2 Bluetooth temperature/humidity sensors and publishing their readings to an MQTT broker following the [Homie convention](https://homieiot.github.io/).

See [the main project readme](https://github.com/alsuren/mijia-homie#readme) for more details and background.

## Installation

It is recommended to install the latest release from our Debian repository:

```sh
$ curl -L https://bintray.com/user/downloadSubjectPublicKey?username=homie-rs | sudo apt-key add -
$ echo "deb https://dl.bintray.com/homie-rs/homie-rs stable main" | sudo tee /etc/apt/sources.list.d/homie-rs.list
$ sudo apt update && sudo apt install mijia-homie
```

Alternatively, you may install with cargo install, but that will require some more setup:

```sh
$ cargo install mijia-homie
```

## Usage

If you have installed the Debian package, the service will be set up with systemd for you already. Otherwise, copy the `mijia-homie` binary to `/usr/bin`, copy `debian-scripts/mijia-homie.service` to `/lib/systemd/system`, create a `mijia-homie` user to run as, and create `/etc/mijia-homie` for configuration files.

There should be two config files under `/etc/mijia-homie`:

- `mijia_homie.toml` contains the main configuration for the service, such as which MQTT broker to connect to and the name and ID of the Homie device. See [mijia_homie.example.toml](mijia_homie.example.toml) for an example of the settings that are supported.
- `sensor_names.conf` contains a map of sensor MAC addresses to human-readable names. Only the sensors listed in this file will be connected to, so you will need to fill it in before `mijia-homie` does anything useful.

After editing these config files you will need to restart the service:

```sh
$ sudo systemctl restart mijia-homie.service
```

You may find it helpful to watch the logs to see whether it is managing to connect to your sensors:

```sh
$ sudo journalctl -u mijia-homie.service --output=cat --follow
```

Once it is running, try connecting to your MQTT broker with a [Homie controller](https://homieiot.github.io/implementations/#controller) such as [HoDD](https://rroemhild.github.io/hodd/) or [openHAB](https://www.openhab.org/) to see your sensors.

## License

Licensed under either of

- [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
- [MIT license](http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
