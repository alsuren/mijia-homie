//! Support for the
//! [atc1441 format](https://github.com/atc1441/ATC_MiThermometer#advertising-format-of-the-custom-firmware)
//! and [pvvx custom format](https://github.com/pvvx/ATC_MiThermometer#custom-format-all-data-little-endian).

use crate::uuid_from_u16;
use std::fmt::{self, Display, Formatter};
use uuid::Uuid;

/// GATT service 0x181a, environmental sensing.
pub const UUID: Uuid = uuid_from_u16(0x181a);

/// A sensor reading in the atc1411 or pvvx custom format.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SensorReading {
    /// [atc1441 format](https://github.com/atc1441/ATC_MiThermometer#advertising-format-of-the-custom-firmware)
    Atc {
        mac: [u8; 6],
        temperature: i16,
        humidity: u8,
        battery_percent: u8,
        battery_mv: u16,
        packet_counter: u8,
    },
    /// [pvvx custom format](https://github.com/pvvx/ATC_MiThermometer#custom-format-all-data-little-endian)
    Pvvx {
        mac: [u8; 6],
        temperature: i16,
        humidity: u16,
        battery_mv: u16,
        battery_percent: u8,
        counter: u8,
        flags: u8,
    },
}

impl Display for SensorReading {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::Atc {
                mac,
                humidity,
                battery_percent,
                battery_mv,
                packet_counter,
                ..
            } => {
                write!(
                    f,
                    "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    mac[0], mac[1], mac[2], mac[3], mac[4], mac[5],
                )?;
                write!(
                    f,
                    " ({}): {:0.2}°C, {}% humidity, {}%/{}mV battery",
                    packet_counter,
                    self.temperature(),
                    humidity,
                    battery_percent,
                    battery_mv
                )?;
            }
            Self::Pvvx {
                mac,
                battery_mv,
                battery_percent,
                counter,
                flags,
                ..
            } => {
                write!(
                    f,
                    "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    mac[0], mac[1], mac[2], mac[3], mac[4], mac[5],
                )?;
                write!(
                    f,
                    " ({}): {:0.2}°C, {:0.2}% humidity, {}%/{}mV battery, flags {:#04x}",
                    counter,
                    self.temperature(),
                    self.humidity(),
                    battery_percent,
                    battery_mv,
                    flags,
                )?;
            }
        }
        Ok(())
    }
}

impl SensorReading {
    /// Tries to decode the given bytestring (from service data with UUID [`UUID`] of a BLE
    /// advertisement) as a sensor reading.
    ///
    /// Returns `None` if it is not a recognised format.
    pub fn decode(data: &[u8]) -> Option<Self> {
        match data.len() {
            13 => {
                // atc1441 format
                let mut mac: [u8; 6] = data[0..6].try_into().unwrap();
                mac.reverse();
                Some(Self::Atc {
                    mac,
                    temperature: i16::from_le_bytes(data[6..=7].try_into().unwrap()),
                    humidity: data[8],
                    battery_percent: data[9],
                    battery_mv: u16::from_le_bytes(data[10..=11].try_into().unwrap()),
                    packet_counter: data[12],
                })
            }
            15 => {
                // pvvx custom format
                let mut mac: [u8; 6] = data[0..6].try_into().unwrap();
                mac.reverse();
                Some(Self::Pvvx {
                    mac,
                    temperature: i16::from_le_bytes(data[6..=7].try_into().unwrap()),
                    humidity: u16::from_le_bytes(data[8..=9].try_into().unwrap()),
                    battery_mv: u16::from_le_bytes(data[10..=11].try_into().unwrap()),
                    battery_percent: data[12],
                    counter: data[13],
                    flags: data[14],
                })
            }
            _ => None,
        }
    }

    /// Encodes the given sensor reading to be sent in a BLE advertisement, as service data for UUID
    /// [`UUID`].
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::Atc {
                mac,
                temperature,
                humidity,
                battery_percent,
                battery_mv,
                packet_counter,
            } => {
                let mut data = Vec::with_capacity(13);
                data.extend(mac.iter().rev());
                data.extend_from_slice(&temperature.to_le_bytes());
                data.push(*humidity);
                data.push(*battery_percent);
                data.extend_from_slice(&battery_mv.to_le_bytes());
                data.push(*packet_counter);
                data
            }
            Self::Pvvx {
                mac,
                temperature,
                humidity,
                battery_mv,
                battery_percent,
                counter,
                flags,
            } => {
                let mut data = Vec::with_capacity(15);
                data.extend(mac.iter().rev());
                data.extend_from_slice(&temperature.to_le_bytes());
                data.extend_from_slice(&humidity.to_le_bytes());
                data.extend_from_slice(&battery_mv.to_le_bytes());
                data.push(*battery_percent);
                data.push(*counter);
                data.push(*flags);
                data
            }
        }
    }

    /// Returns the MAC address of the sensor.
    pub fn mac(&self) -> &[u8; 6] {
        match self {
            Self::Atc { mac, .. } => mac,
            Self::Pvvx { mac, .. } => mac,
        }
    }

    /// Returns the temperature reading in °C.
    pub fn temperature(&self) -> f32 {
        let temperature = match self {
            Self::Atc { temperature, .. } => *temperature,
            Self::Pvvx { temperature, .. } => *temperature,
        };
        f32::from(temperature) / 100.0
    }

    /// Returns the humidity reading, as a percentage.
    pub fn humidity(&self) -> f32 {
        match self {
            Self::Atc { humidity, .. } => (*humidity).into(),
            Self::Pvvx { humidity, .. } => f32::from(*humidity) / 100.0,
        }
    }

    /// Returns the battery level, as a percentage.
    pub fn battery_percent(&self) -> u8 {
        match self {
            Self::Atc {
                battery_percent, ..
            } => *battery_percent,
            Self::Pvvx {
                battery_percent, ..
            } => *battery_percent,
        }
    }

    /// Returns the battery voltage, in mV.
    pub fn battery_mv(&self) -> u16 {
        match self {
            Self::Atc { battery_mv, .. } => *battery_mv,
            Self::Pvvx { battery_mv, .. } => *battery_mv,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_atc1441() {
        assert_eq!(
            SensorReading::Atc {
                mac: [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                temperature: 2103,   // 21.03°
                humidity: 42,        // 42%
                battery_percent: 89, // 89%
                battery_mv: 1526,    // 1526 mV
                packet_counter: 0,
            }
            .to_string(),
            "aa:bb:cc:dd:ee:ff (0): 21.03°C, 42% humidity, 89%/1526mV battery"
        );
    }

    #[test]
    fn format_pvvx() {
        assert_eq!(
            SensorReading::Pvvx {
                mac: [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                temperature: 2103,   // 21.03°
                humidity: 4213,      // 42.13%
                battery_percent: 89, // 89%
                battery_mv: 1526,    // 1526 mV
                counter: 0,
                flags: 0x04,
            }
            .to_string(),
            "aa:bb:cc:dd:ee:ff (0): 21.03°C, 42.13% humidity, 89%/1526mV battery, flags 0x04"
        );
    }

    #[test]
    fn decode_atc1441() {
        let decoded = SensorReading::decode(&[
            0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x37, 0x08, 42, 89, 0xf6, 0x05, 0x00,
        ])
        .unwrap();
        assert_eq!(
            decoded,
            SensorReading::Atc {
                mac: [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                temperature: 2103,   // 21.03°
                humidity: 42,        // 42%
                battery_percent: 89, // 89%
                battery_mv: 1526,    // 1526 mV
                packet_counter: 0,
            }
        );
        assert_eq!(decoded.temperature(), 21.03);
        assert_eq!(decoded.humidity(), 42.00);
        assert_eq!(decoded.battery_mv(), 1526);
        assert_eq!(decoded.battery_percent(), 89);
    }

    #[test]
    fn encode_atc1441() {
        assert_eq!(
            SensorReading::Atc {
                mac: [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                temperature: 2103,   // 21.03°
                humidity: 42,        // 42%
                battery_percent: 89, // 89%
                battery_mv: 1526,    // 1526 mV
                packet_counter: 0,
            }
            .encode(),
            &[
                0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x37, 0x08, 42, 89, 0xf6, 0x05, 0x00
            ]
        );
    }

    #[test]
    fn decode_pvvx() {
        let decoded = SensorReading::decode(&[
            0x93, 0x41, 0x8c, 0x38, 0xc1, 0xa4, 0xac, 0x08, 0x9e, 0x14, 0xa0, 0x0b, 100, 136, 0x04,
        ])
        .unwrap();
        assert_eq!(
            decoded,
            SensorReading::Pvvx {
                mac: [0xa4, 0xc1, 0x38, 0x8c, 0x41, 0x93],
                temperature: 2220,
                humidity: 5278,
                battery_mv: 2976,
                battery_percent: 100,
                counter: 136,
                flags: 0x04,
            }
        );
        assert_eq!(decoded.temperature(), 22.20);
        assert_eq!(decoded.humidity(), 52.78);
        assert_eq!(decoded.battery_mv(), 2976);
        assert_eq!(decoded.battery_percent(), 100);
    }

    #[test]
    fn encode_pvvx() {
        assert_eq!(
            SensorReading::Pvvx {
                mac: [0xa4, 0xc1, 0x38, 0x8c, 0x41, 0x93],
                temperature: 2220,
                humidity: 5278,
                battery_mv: 2976,
                battery_percent: 100,
                counter: 136,
                flags: 0x04,
            }
            .encode(),
            &[
                0x93, 0x41, 0x8c, 0x38, 0xc1, 0xa4, 0xac, 0x08, 0x9e, 0x14, 0xa0, 0x0b, 100, 136,
                0x04
            ]
        );
    }

    #[test]
    fn decode_empty() {
        assert_eq!(SensorReading::decode(&[]), None);
    }
}
