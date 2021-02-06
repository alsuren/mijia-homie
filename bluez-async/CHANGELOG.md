# Changelog

## 0.2.1

### New features

- Added TX power and address type to `DeviceInfo`.
- Added methods for using a specific adapter rather than all adapters on the system.
- Added support for reading and writing characteristics and descriptors from a given offset, and
  explicitly specifying what type of write operation to use.

### Other changes

- Wait for service discovery to complete before returning from `connect`. This should avoid errors
  when trying to look up services on a device immediately after connecting to it.

## 0.2.0

### Breaking changes

- Added new `DeviceEvent` variant for manufacturer-specific data updates.

### New features

- Added method to stop discovery.
- Added method to set discovery filters when starting discovery.
- Added manufacturer-specific data to `DeviceInfo`.

### Other changes

- Added example to log readings from RuuviTag sensors.

## 0.1.1

### New features

- Added more properties to `DeviceInfo`, including service UUIDs.

### Other changes

- Added more documentation.
- Added example to list all devices.

## 0.1.0

Initial release.
