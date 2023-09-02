# Changelog

## 0.9.0

### Breaking changes

- Updated to `rumqttc` 0.22.

## 0.8.0

### Breaking changes

- Updated to `rumqttc` 0.20.

## 0.7.0

### Breaking changes

- `HomieController::poll` now returns a `Vec` of events rather than an `Option`.
- Updated to `rumqttc` 0.16.

### Bugfixes

- Fixed [bug](https://github.com/alsuren/mijia-homie/issues/227) where property values arriving
  before the corresponding `$properties` topic would be dropped.

## 0.6.0

### Breaking changes

- Updated to `rumqttc` 0.12.

## 0.5.1

### Bugfixes

- Fixed bug introduced in 0.5.0 which could result in an infinite loop of subscribing and receiving
  messages.

## 0.5.0

### Breaking changes

- It is no longer necessary to call `HomieController::start`, it has been removed from the public
  API. If the MQTT connection is dropped and reconnected the necessary subscriptions will
  automatically be set up again, without the need for a persistent session.
- Added new `Event::Connected`.
- Updated to `rumqttc` 0.10.

## 0.4.0

### Breaking changes

- Acronyms no longer upper-case.
- Updated to `rumqttc` 0.8.

## 0.3.0

### Breaking changes

- Updated to Tokio 1.0, and updated some other dependencies to match.

### Other changes

- Added an integration test, testing that this crate works as expected with the `homie-device`
  crate.

## 0.2.0

### Breaking changes

- Updated to `rumqttc` 0.2.
- Added `fresh` flag to `PropertyValueChanged` event.

### New features

- Added method to get Homie base topic.

## 0.1.0

Initial release.
