# Changelog

## 0.8.0

### Breaking changes

- Updated to `bluez-async` 0.8.1.

## 0.7.1

### New features

- Exposed `MijiaEvent::from`, for converting a `bluez-async` `BluetoothEvent` into a `MijiaEvent`.
  This can be useful if you want to handle other Bluetooth events directly as well as Mijia events.

## 0.7.0

### Breaking changes

- Updated to `bluez-async` 0.7.0.

## 0.6.0

### Breaking changes

- Updated to `bluez-async` 0.6.0.

## 0.5.0

### Breaking changes

- Updated to `bluez-async` 0.5.0.

## 0.4.0

### Breaking changes

- Updated to `bluez-async` 0.3.0.

### New features

- Added event for new sensor being discovered.
- Added `SignedDuration` type for conveniently comparing `SystemTime`s.
- Added example to fix clocks of sensors.

## 0.3.1

### Other changes

- Added more documentation.
- Print IDs more nicely in examples.

## 0.3.0

### Breaking changes

- Updated to Tokio 1.0, and updated some other dependencies to match.
- Error types have changed slightly due to changes in the `dbus` crate.
- `MijiaSession::event_stream()` no longer returns a `MsgMatch`; the match will automatically be
  removed when you drop the stream.

### Other changes

- Split out BlueZ wrapper code to new crate `bluez-async`.
- Use UUIDs to look up services and characteristics, rather than hardcoding the paths BlueZ gives them.
- Get D-Bus to filter events when reading history records, which should make it slightly faster.

## 0.2.0

### Breaking changes

- Switched to `thiserror` for error types.

### New features

- Added support for getting historical data.
- Added support for setting comfort level, temperature unit and time.

### Other changes

- Added documentation.
- Added examples.

## 0.1.0

Initial release.
