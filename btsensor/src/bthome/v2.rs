//! Support for the [BTHome](https://bthome.io/) v2 format.

use bluez_async::uuid_from_u16;
use std::fmt::{self, Display, Formatter};
use thiserror::Error;
use uuid::Uuid;

pub const UUID: Uuid = uuid_from_u16(0xfcd2);

const DEVICE_INFO_ENCRYPTED: u8 = 0x01;
const DEVICE_INFO_TRIGGER_BASED: u8 = 0x04;
const DEVICE_INFO_VERSION_MASK: u8 = 0xe0;
const DEVICE_INFO_VERSION_OFFSET: usize = 5;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum DecodeError {
    #[error("Unsupported BTHome version {0}")]
    UnsupportedVersion(u8),
    #[error("Invalid property {0:#04x}")]
    InvalidProperty(u8),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BtHomeV2 {
    pub encrypted: bool,
    pub trigger_based: bool,
    pub elements: Vec<Element>,
}

impl BtHomeV2 {
    pub fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        let device_info = data[0];
        let encrypted = device_info & DEVICE_INFO_ENCRYPTED != 0;
        let trigger_based = device_info & DEVICE_INFO_TRIGGER_BASED != 0;
        let version = (device_info & DEVICE_INFO_VERSION_MASK) >> DEVICE_INFO_VERSION_OFFSET;
        if version != 2 {
            return Err(DecodeError::UnsupportedVersion(version));
        }

        let mut remaining_data = &data[1..];
        let mut elements = Vec::new();
        while remaining_data.len() >= 2 {
            let (element_length, element) = Element::decode(remaining_data)?;
            remaining_data = &remaining_data[element_length..];
            elements.push(element);
        }

        Ok(Self {
            encrypted,
            trigger_based,
            elements,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Element {
    property: Property,
    value: Value,
}

impl Element {
    fn decode(data: &[u8]) -> Result<(usize, Self), DecodeError> {
        let object_id = data[0];
        let remaining = &data[1..];
        let (value_length, value) = match object_id {
            0x51 => Ok(Self::read_u16(remaining, Property::Acceleration)),
            0x02 => Ok(Self::read_i16(remaining, Property::Temperature)),
            object_id => Err(DecodeError::InvalidProperty(object_id)),
        }?;
        Ok((value_length + 1, value))
    }

    fn read_u16(data: &[u8], property: Property) -> (usize, Self) {
        (
            2,
            Element {
                property,
                value: Value::U16(u16::from_le_bytes(data[0..2].try_into().unwrap())),
            },
        )
    }

    fn read_i16(data: &[u8], property: Property) -> (usize, Self) {
        (
            2,
            Element {
                property,
                value: Value::I16(i16::from_le_bytes(data[0..2].try_into().unwrap())),
            },
        )
    }
}

impl Display for Element {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "{}: {}{}",
            self.property.name(),
            self.value,
            self.property.unit()
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Value {
    U16(u16),
    I16(i16),
}

impl Display for Value {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::U16(value) => value.fmt(f),
            Self::I16(value) => value.fmt(f),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Property {
    Acceleration,
    Temperature,
}

impl Property {
    pub fn name(self) -> &'static str {
        match self {
            Self::Acceleration => "acceleration",
            Self::Temperature => "temperature",
        }
    }

    pub fn unit(self) -> &'static str {
        match self {
            Self::Acceleration => "m/s²",
            Self::Temperature => "°C",
        }
    }
}

impl Display for BtHomeV2 {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("(")?;
        if self.encrypted {
            f.write_str("encrypted")?;
        } else {
            f.write_str("unencrypted")?;
        }
        if self.trigger_based {
            f.write_str(", trigger based")?;
        }
        f.write_str(") ")?;

        for (i, element) in self.elements.iter().enumerate() {
            if i != 0 {
                f.write_str(", ")?;
            }
            element.fmt(f)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_valid() {
        assert_eq!(
            BtHomeV2::decode(&[0x40, 0x02, 0xc4, 0x09]).unwrap(),
            BtHomeV2 {
                encrypted: false,
                trigger_based: false,
                elements: vec![Element {
                    property: Property::Temperature,
                    value: Value::I16(2500),
                }],
            }
        );
    }

    #[test]
    fn format() {
        assert_eq!(
            BtHomeV2 {
                encrypted: false,
                trigger_based: false,
                elements: vec![
                    Element {
                        property: Property::Acceleration,
                        value: Value::U16(22151),
                    },
                    Element {
                        property: Property::Temperature,
                        value: Value::I16(2506),
                    }
                ]
            }
            .to_string(),
            "(unencrypted) acceleration: 22.151m/s², temperature: 25.06°"
        );
    }
}
