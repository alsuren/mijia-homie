//! A library for decoding sensor readings from BLE advertisements.

pub mod atc;
pub mod bthome;

use crate::{atc::SensorReading, bthome::Element};
use log::warn;
use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
};
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Reading {
    Atc(SensorReading),
    BtHome(Vec<Element>),
}

impl Reading {
    pub fn decode(service_data: &HashMap<Uuid, Vec<u8>>) -> Option<Self> {
        if let Some(data) = service_data.get(&atc::UUID) {
            if let Some(reading) = atc::SensorReading::decode(data) {
                return Some(Self::Atc(reading));
            }
        }
        if let Some(data) = service_data.get(&bthome::UNENCRYPTED_UUID) {
            match bthome::decode(data) {
                Ok(elements) => {
                    return Some(Self::BtHome(elements));
                }
                Err(e) => {
                    warn!("Error decoding BTHome data: {}", e);
                }
            }
        }
        None
    }
}

impl Display for Reading {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::Atc(reading) => reading.fmt(f),
            Self::BtHome(elements) => {
                for (i, element) in elements.into_iter().enumerate() {
                    if i != 0 {
                        f.write_str(", ")?;
                    }
                    element.fmt(f)?;
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bthome::Property;

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
        let service_data = [(bthome::UNENCRYPTED_UUID, vec![])].into_iter().collect();
        assert_eq!(
            Reading::decode(&service_data),
            Some(Reading::BtHome(vec![]))
        );
    }

    #[test]
    fn decode_bthome_v1_valid() {
        let service_data = [(
            bthome::UNENCRYPTED_UUID,
            vec![0x23, 0x02, 0xC4, 0x09, 0x03, 0x03, 0xBF, 0x13],
        )]
        .into_iter()
        .collect();
        assert_eq!(
            Reading::decode(&service_data),
            Some(Reading::BtHome(vec![
                Element::new_signed(Property::Temperature, 2500),
                Element::new_unsigned(Property::Humidity, 5055),
            ]))
        );
    }
}
