# Changelog

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
