//! A library for decoding sensor readings from BLE advertisements.

pub mod atc;
pub mod bthome;

use log::warn;
use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
};
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Reading {
    Atc(atc::SensorReading),
    BtHome(Vec<bthome::Element>),
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
