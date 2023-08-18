//! A library for decoding sensor readings from BLE advertisements.
//!
//! This supports [BTHome](https://bthome.io/) (v1 and v2), the
//! [atc1441 format](https://github.com/atc1441/ATC_MiThermometer#advertising-format-of-the-custom-firmware),
//! and the [pvvx custom format](https://github.com/pvvx/ATC_MiThermometer#custom-format-all-data-little-endian).

pub mod atc;
pub mod bthome;

use crate::{atc::SensorReading, bthome::v1::Element};
use bthome::v2::BtHomeV2;
use log::warn;
use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
};
use uuid::Uuid;

const BLUETOOTH_BASE_UUID: u128 = 0x00000000_0000_1000_8000_00805f9b34fb;

/// Converts a 16-bit BLE short UUID to a full 128-bit UUID by filling in the standard Bluetooth
/// Base UUID.
const fn uuid_from_u16(short: u16) -> Uuid {
    Uuid::from_u128(BLUETOOTH_BASE_UUID | ((short as u128) << 96))
}

/// A reading from some BLE sensor advertisement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Reading {
    Atc(SensorReading),
    BtHomeV1(Vec<Element>),
    BtHomeV2(BtHomeV2),
}

impl Reading {
    /// Attempts to decode any relevant entries in the given service data map as either atc1441
    /// format, pvvx custom format, or BTHome (v1 or v2).
    ///
    /// If UUIDs are present for more than one of the above formats then only the first valid one is
    /// returned.
    ///
    /// Returns `None` if none of the UUIDs for the above formats are present, or there is an error
    /// decoding them.
    pub fn decode(service_data: &HashMap<Uuid, Vec<u8>>) -> Option<Self> {
        if let Some(data) = service_data.get(&atc::UUID) {
            if let Some(reading) = atc::SensorReading::decode(data) {
                return Some(Self::Atc(reading));
            }
        }
        if let Some(data) = service_data.get(&bthome::v1::UNENCRYPTED_UUID) {
            match bthome::v1::decode(data) {
                Ok(elements) => return Some(Self::BtHomeV1(elements)),
                Err(e) => {
                    warn!("Error decoding BTHome v1 data: {}", e)
                }
            }
        }
        if let Some(data) = service_data.get(&bthome::v2::UUID) {
            match bthome::v2::BtHomeV2::decode(data) {
                Ok(bthome) => return Some(Self::BtHomeV2(bthome)),
                Err(e) => warn!("Error decoding BTHome v2 data: {}", e),
            }
        }
        None
    }
}

impl Display for Reading {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::Atc(reading) => reading.fmt(f),
            Self::BtHomeV1(elements) => {
                for (i, element) in elements.iter().enumerate() {
                    if i != 0 {
                        f.write_str(", ")?;
                    }
                    element.fmt(f)?;
                }
                Ok(())
            }
            Self::BtHomeV2(bthome) => bthome.fmt(f),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bthome::v1::Property;

    #[test]
    fn decode_none() {
        let empty_service_data = HashMap::new();
        assert_eq!(Reading::decode(&empty_service_data), None);
    }

    #[test]
    fn decode_atc_empty() {
        let service_data = [(atc::UUID, vec![])].into_iter().collect();
        assert_eq!(Reading::decode(&service_data), None);
    }

    #[test]
    fn decode_atc_invalid() {
        let service_data = [(atc::UUID, vec![0xaa, 0xbb])].into_iter().collect();
        assert_eq!(Reading::decode(&service_data), None);
    }

    #[test]
    fn decode_atc_valid() {
        let service_data = [(
            atc::UUID,
            vec![
                0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x37, 0x08, 42, 89, 0xf6, 0x05, 0x00,
            ],
        )]
        .into_iter()
        .collect();
        assert_eq!(
            Reading::decode(&service_data),
            Some(Reading::Atc(SensorReading::Atc {
                mac: [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                temperature: 2103,
                humidity: 42,
                battery_percent: 89,
                battery_mv: 1526,
                packet_counter: 0,
            }))
        );
    }

    #[test]
    fn decode_bthome_v1_empty() {
        let service_data = [(bthome::v1::UNENCRYPTED_UUID, vec![])]
            .into_iter()
            .collect();
        assert_eq!(
            Reading::decode(&service_data),
            Some(Reading::BtHomeV1(vec![]))
        );
    }

    #[test]
    fn decode_bthome_v1_valid() {
        let service_data = [(
            bthome::v1::UNENCRYPTED_UUID,
            vec![0x23, 0x02, 0xC4, 0x09, 0x03, 0x03, 0xBF, 0x13],
        )]
        .into_iter()
        .collect();
        assert_eq!(
            Reading::decode(&service_data),
            Some(Reading::BtHomeV1(vec![
                Element::new_signed(Property::Temperature, 2500),
                Element::new_unsigned(Property::Humidity, 5055),
            ]))
        );
    }
}
