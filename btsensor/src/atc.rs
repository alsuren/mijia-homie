//! Support for the
//! [atc1441 format](https://github.com/atc1441/ATC_MiThermometer#advertising-format-of-the-custom-firmware)
//! and [pvvx custom format](https://github.com/pvvx/ATC_MiThermometer#custom-format-all-data-little-endian).

use bluez_async::uuid_from_u16;
use std::fmt::{self, Display, Formatter};
use uuid::Uuid;

/// GATT service 0x181a, environmental sensing.
pub const UUID: Uuid = uuid_from_u16(0x181a);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SensorReading {
    Atc {
        mac: [u8; 6],
        temperature: i16,
        humidity: u8,
        battery_percent: u8,
        battery_mv: u16,
        packet_counter: u8,
    },
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
                temperature,
                humidity,
                battery_percent,
                battery_mv,
                packet_counter,
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
                    *temperature as f64 / 100.0,
                    humidity,
                    battery_percent,
                    battery_mv
                )?;
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
                write!(
                    f,
                    "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    mac[0], mac[1], mac[2], mac[3], mac[4], mac[5],
                )?;
                write!(
                    f,
                    " ({}): {:0.2}°C, {:0.2}% humidity, {}%/{}mV battery, flags {:#04x}",
                    counter,
                    *temperature as f64 / 100.0,
                    *humidity as f64 / 100.0,
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
    pub fn decode(data: &[u8]) -> Option<Self> {
        match data.len() {
            13 => {
                // atc1441 format
                Some(Self::Atc {
                    mac: data[0..6].try_into().unwrap(),
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
                data.extend_from_slice(mac);
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
        assert_eq!(
            SensorReading::decode(&[
                0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x37, 0x08, 42, 89, 0xf6, 0x05, 0x00
            ]),
            Some(SensorReading::Atc {
                mac: [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                temperature: 2103,   // 21.03°
                humidity: 42,        // 42%
                battery_percent: 89, // 89%
                battery_mv: 1526,    // 1526 mV
                packet_counter: 0,
            })
        );
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
            &[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x37, 0x08, 42, 89, 0xf6, 0x05, 0x00]
        );
    }

    #[test]
    fn decode_pvvx() {
        assert_eq!(
            SensorReading::decode(&[
                0x93, 0x41, 0x8c, 0x38, 0xc1, 0xa4, 0xac, 0x08, 0x9e, 0x14, 0xa0, 0x0b, 100, 136,
                0x04
            ]),
            Some(SensorReading::Pvvx {
                mac: [0xa4, 0xc1, 0x38, 0x8c, 0x41, 0x93],
                temperature: 2220,
                humidity: 5278,
                battery_mv: 2976,
                battery_percent: 100,
                counter: 136,
                flags: 0x04,
            })
        );
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
