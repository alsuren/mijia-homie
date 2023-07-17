//! Support for the
//! [atc1441 format](https://github.com/atc1441/ATC_MiThermometer#advertising-format-of-the-custom-firmware)
//! and [pvvx custom format](https://github.com/pvvx/ATC_MiThermometer#custom-format-all-data-little-endian).

use bluez_async::uuid_from_u16;
use uuid::Uuid;

/// GATT service 0x181a, environmental sensing.
pub const UUID: Uuid = uuid_from_u16(0x181a);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SensorReading {
    mac: [u8; 6],
    temperature: i16,
    humidity: u8,
    battery_percent: u8,
    battery_mv: u16,
    packet_counter: u8,
}

impl SensorReading {
    pub fn decode(data: &[u8]) -> Option<Self> {
        match data.len() {
            13 => {
                // atc1441 format
                Some(Self {
                    mac: data[0..6].try_into().unwrap(),
                    temperature: i16::from_le_bytes(data[6..=7].try_into().unwrap()),
                    humidity: data[8],
                    battery_percent: data[9],
                    battery_mv: u16::from_le_bytes(data[10..=11].try_into().unwrap()),
                    packet_counter: data[12],
                })
            }
            19 => {
                // pvvx custom format
                None
            }
            _ => None,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(13);
        data.extend_from_slice(&self.mac);
        data.extend_from_slice(&self.temperature.to_le_bytes());
        data.push(self.humidity);
        data.push(self.battery_percent);
        data.extend_from_slice(&self.battery_mv.to_le_bytes());
        data.push(self.packet_counter);
        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_atc1441() {
        assert_eq!(
            SensorReading::decode(&[
                0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x37, 0x08, 42, 89, 0xf6, 0x05, 0x00
            ]),
            Some(SensorReading {
                mac: [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                temperature: 2103,   // 21.03°
                humidity: 42,        // 42%
                battery_percent: 89, // 89%
                battery_mv: 1526,    // 1.526 mV
                packet_counter: 0,
            })
        );
    }

    #[test]
    fn encode_atc1441() {
        assert_eq!(
            SensorReading {
                mac: [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                temperature: 2103,   // 21.03°
                humidity: 42,        // 42%
                battery_percent: 89, // 89%
                battery_mv: 1526,    // 1.526 mV
                packet_counter: 0,
            }
            .encode(),
            &[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x37, 0x08, 42, 89, 0xf6, 0x05, 0x00]
        );
    }

    #[test]
    fn decode_pvvx() {}

    fn decode_empty() {
        assert_eq!(SensorReading::decode(&[]), None);
    }
}
