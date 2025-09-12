# Changelog

## 0.2.11

### Other changes

- Updated dependencies.

## 0.2.10

### Other changes

- Updated dependencies.

## 0.2.9

### Other changes

- Updated dependencies.

## 0.2.8

### Other changes

- Updated dependencies.

## 0.2.7

### Other changes

- Updated dependencies.

## 0.2.6

### Bugfixes

- Fixed bug introduced in 0.2.5 which could result in an infinite loop of subscribing and receiving
  messages.

## 0.2.5

### Bugfixes

- Handle reconnection to the MQTT broker without relying on a persistent session.

## 0.2.4

### Bugfixes

- Updated to `rumqttc` 0.8, to fix build errors on latest rustc.

## 0.2.3

### New features

- Added `value_int` field to `boolean` measurements, for compatibility with Grafana.

### Other changes

- Use rustls rather than OpenSSL.

## 0.2.2

### Bug fixes

- Linked against right version of libssl.
- Fixed dependencies of Debian packages.

## 0.2.1

### New features

- Automatically try to reconnect to the MQTT broker if the connection is lost, with a configurable
  delay.

### Other changes

- Fixed dependencies of Debian packages.
- Fixed permissions of config file not to be world-readable as it may contain a password.
- Added comments to config file explaining what the options do.

## 0.2.0

### Breaking changes

- Config files are now TOML and their filenames have changed, so you will need to update your config
  after updating to this release.

## 0.1.0

First release of homie-influx service. It's been working for me so far at least!
