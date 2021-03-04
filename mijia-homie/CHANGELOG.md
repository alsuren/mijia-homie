# Changelog

## Unreleased

### Bug fixes

- Fixed owner of mijia-history-influx.toml config file.

## 0.2.3

### New features

- Added `mijia-names` utility to scan for sensors and create a `sensor-names.toml` config.
- Added `mijia-history-influx` utility to dump historical data from sensors to InfluxDB.

## 0.2.2

### New features

- Added `min_update_period_seconds` option, to allow the rate of sensor updates sent to the MQTT
  broker to be limited if desired.

### Other changes

- Fixed dependencies of Debian packages.
- Fixed permissions of config file not to be world-readable as it may contain a password.
- Added comments to config file explaining what the options do.

## 0.2.1

Skipped because of an issue with the Debian package build linking against the wrong version of some
dynamic libraries meaning that it wouldn't run (#125).

## 0.2.0

### Breaking changes

- Config files are now TOML and their filenames have changed, so you will need to update your config
  after updating to this release.

### New features

- Using multiple Bluetooth adapters is now supported. mijia-homie will scan on all Bluetooth
  adapters on the system, and try to connect to a sensor on all adapters in turn which discover it
  until one succeeds. This is useful if you have more sensors than a single Bluetooth adapter
  supports (usually 7 or 10).

## 0.1.0

First release of `mijia-homie`. This works on Raspberry Pi and other devices, and can reliably
connect to several sensors and publish them to an MQTT broker following the Homie convention. It has
been tested with openHAB and HoDD.
