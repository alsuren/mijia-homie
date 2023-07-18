//! Support for the [BTHome](https://bthome.io/) v1 format.

use bluez_async::uuid_from_u16;
use std::fmt::{self, Display, Formatter};
use thiserror::Error;
use uuid::Uuid;

pub const UNENCRYPTED_UUID: Uuid = uuid_from_u16(0x181c);
pub const ENCRYPTED_UUID: Uuid = uuid_from_u16(0x181e);

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum DecodeError {
    #[error("Invalid data type {0:03x}")]
    InvalidDataType(u8),
    #[error("Invalid property {0:03x}")]
    InvalidProperty(u8),
    #[error("Premature end of data")]
    PrematureEnd,
    #[error("Extra data {0:?}")]
    ExtraData(Vec<u8>),
    #[error("Unsupported format {0:?}")]
    UnsupportedFormat(DataType),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Element {
    pub property: Property,
    value: Value,
}

impl Element {
    pub fn float_value(&self) -> f64 {
        f64::from(&self.value) / 10.0f64.powi(self.property.decimal_point())
    }

    pub fn int_value(&self) -> Option<i64> {
        if self.property.decimal_point() == 0 {
            Some((&self.value).into())
        } else {
            None
        }
    }
}

impl Display for Element {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(value) = self.int_value() {
            write!(f, "{}: {}{}", self.property, value, self.property.unit())
        } else {
            write!(
                f,
                "{}: {}{}",
                self.property,
                self.float_value(),
                self.property.unit(),
            )
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DataType {
    UnsignedInt = 0b000,
    SignedInt = 0b001,
    Float = 0b010,
    String = 0b011,
    Mac = 0b100,
}

impl From<DataType> for u8 {
    fn from(value: DataType) -> Self {
        value as Self
    }
}

impl TryFrom<u8> for DataType {
    type Error = DecodeError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0b000 => Ok(Self::UnsignedInt),
            0b001 => Ok(Self::SignedInt),
            0b010 => Ok(Self::Float),
            0b011 => Ok(Self::String),
            0b100 => Ok(Self::Mac),
            _ => Err(DecodeError::InvalidDataType(value)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Value {
    UnsignedInt(u32),
    SignedInt(i32),
}

impl Value {
    fn from_bytes(bytes: &[u8], format: DataType) -> Result<Self, DecodeError> {
        match (format, bytes.len()) {
            (DataType::UnsignedInt, 4) => Ok(Self::UnsignedInt(u32::from_le_bytes(
                bytes.try_into().unwrap(),
            ))),
            (DataType::UnsignedInt, 3) => Ok(Self::UnsignedInt(
                bytes[0] as u32 | (bytes[1] as u32) << 8 | (bytes[2] as u32) << 16,
            )),
            (DataType::UnsignedInt, 2) => Ok(Self::UnsignedInt(
                u16::from_le_bytes(bytes.try_into().unwrap()).into(),
            )),
            (DataType::UnsignedInt, 1) => Ok(Self::UnsignedInt(bytes[0].into())),
            (DataType::SignedInt, 4) => Ok(Self::SignedInt(i32::from_le_bytes(
                bytes.try_into().unwrap(),
            ))),
            (DataType::SignedInt, 3) => Ok(Self::SignedInt(
                bytes[0] as i32 | (bytes[1] as i32) << 8 | (bytes[2] as i32) << 16,
            )),
            (DataType::SignedInt, 2) => Ok(Self::SignedInt(
                i16::from_le_bytes(bytes.try_into().unwrap()).into(),
            )),
            (DataType::SignedInt, 1) => Ok(Self::SignedInt((bytes[0] as i8).into())),
            _ => Err(DecodeError::UnsupportedFormat(format)),
        }
    }
}

impl From<&Value> for f64 {
    fn from(value: &Value) -> Self {
        match value {
            &Value::SignedInt(v) => v.into(),
            &Value::UnsignedInt(v) => v.into(),
        }
    }
}

impl From<&Value> for i64 {
    fn from(value: &Value) -> Self {
        match value {
            &Value::SignedInt(v) => v.into(),
            &Value::UnsignedInt(v) => v.into(),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Property {
    Battery = 0x01,
    Temperature = 0x02,
    Humidity = 0x03,
    HumidityShort = 0x2e,
    Pressure = 0x04,
    Illuminance = 0x05,
    MassKg = 0x06,
    MassLb = 0x07,
    Dewpoint = 0x08,
    Count = 0x09,
    Energy = 0x0a,
    Power = 0x0b,
    Voltage = 0x0c,
    Pm2_5 = 0x0d,
    Pm10 = 0x0e,
    Co2 = 0x12,
    Tvoc = 0x13,
    Moisture = 0x14,
    MoistureShort = 0x2f,
    Timestamp = 0x50,
    Acceleration = 0x51,
    Gyroscope = 0x52,
}

impl From<Property> for u8 {
    fn from(value: Property) -> Self {
        value as Self
    }
}

impl TryFrom<u8> for Property {
    type Error = DecodeError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(Self::Battery),
            0x02 => Ok(Self::Temperature),
            0x03 => Ok(Self::Humidity),
            0x2e => Ok(Self::HumidityShort),
            0x04 => Ok(Self::Pressure),
            0x05 => Ok(Self::Illuminance),
            0x06 => Ok(Self::MassKg),
            0x07 => Ok(Self::MassLb),
            0x08 => Ok(Self::Dewpoint),
            0x09 => Ok(Self::Count),
            0x0a => Ok(Self::Energy),
            0x0b => Ok(Self::Power),
            0x0c => Ok(Self::Voltage),
            0x0d => Ok(Self::Pm2_5),
            0x0e => Ok(Self::Pm10),
            0x12 => Ok(Self::Co2),
            0x13 => Ok(Self::Tvoc),
            0x14 => Ok(Self::Moisture),
            0x2f => Ok(Self::MoistureShort),
            0x50 => Ok(Self::Timestamp),
            0x51 => Ok(Self::Acceleration),
            0x52 => Ok(Self::Gyroscope),
            _ => Err(DecodeError::InvalidProperty(value)),
        }
    }
}

impl Display for Property {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl Property {
    pub fn name(self) -> &'static str {
        match self {
            Self::Battery => "battery",
            Self::Temperature => "temperature",
            Self::Humidity | Self::HumidityShort => "humidity",
            Self::Pressure => "pressure",
            Self::Illuminance => "illuminance",
            Self::MassKg | Self::MassLb => "mass",
            Self::Dewpoint => "dew point",
            Self::Count => "count",
            Self::Energy => "energy",
            Self::Power => "power",
            Self::Voltage => "voltage",
            Self::Pm2_5 => "pm2.5",
            Self::Pm10 => "pm10",
            Self::Co2 => "CO2",
            Self::Tvoc => "tvoc",
            Self::Moisture | Self::MoistureShort => "moisture",
            Self::Timestamp => "timestamp",
            Self::Acceleration => "acceleration",
            Self::Gyroscope => "gyroscope",
        }
    }

    pub fn unit(self) -> &'static str {
        match self {
            Self::Battery
            | Self::Humidity
            | Self::HumidityShort
            | Self::Moisture
            | Self::MoistureShort => "%",
            Self::Temperature => "°C",
            Self::Pressure => "hPa",
            Self::Illuminance => "lux",
            Self::MassKg => "kg",
            Self::MassLb => "lb",
            Self::Dewpoint => "°C",
            Self::Count | Self::Timestamp => "",
            Self::Energy => "kWh",
            Self::Power => "W",
            Self::Voltage => "V",
            Self::Pm2_5 | Self::Pm10 | Self::Tvoc => "ug/m3",
            Self::Co2 => "ppm",
            Self::Acceleration => "m/s²",
            Self::Gyroscope => "°/s",
        }
    }

    /// The number of spaces to the left to move the decimal point.
    ///
    /// In other words, the value stored should be divided by 10 to the power of this number to get
    /// the actual value.
    fn decimal_point(self) -> i32 {
        match self {
            Self::Battery
            | Self::HumidityShort
            | Self::Count
            | Self::Pm2_5
            | Self::Pm10
            | Self::Co2
            | Self::Tvoc
            | Self::MoistureShort
            | Self::Timestamp => 0,
            Self::Temperature
            | Self::Humidity
            | Self::Pressure
            | Self::Illuminance
            | Self::MassKg
            | Self::MassLb
            | Self::Dewpoint
            | Self::Power
            | Self::Moisture => 2,
            Self::Energy | Self::Voltage | Self::Acceleration | Self::Gyroscope => 3,
        }
    }
}

pub fn decode(mut data: &[u8]) -> Result<Vec<Element>, DecodeError> {
    let mut elements = Vec::new();

    while data.len() > 2 {
        let length_format = data[0];
        let length = length_format & 0x1f;
        // length includes the measurement type byte but not the length/format byte.
        if data.len() <= length.into() {
            return Err(DecodeError::PrematureEnd);
        }
        let format = ((length_format & 0xe0) >> 5).try_into()?;
        let property = data[1].try_into()?;
        let element_end = usize::from(length) + 1;
        let value = Value::from_bytes(&data[2..element_end], format)?;
        elements.push(Element { property, value });

        data = &data[element_end..];
    }

    if data.is_empty() {
        Ok(elements)
    } else {
        Err(DecodeError::ExtraData(data.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_valid() {
        assert_eq!(
            decode(&[0x23, 0x02, 0xC4, 0x09, 0x03, 0x03, 0xBF, 0x13]).unwrap(),
            vec![
                Element {
                    property: Property::Temperature,
                    value: Value::SignedInt(2500),
                },
                Element {
                    property: Property::Humidity,
                    value: Value::UnsignedInt(5055),
                },
            ]
        );
    }

    #[test]
    fn format_element() {
        assert_eq!(
            Element {
                property: Property::Humidity,
                value: Value::UnsignedInt(5055),
            }
            .to_string(),
            "humidity: 50.55%"
        );
        assert_eq!(
            Element {
                property: Property::Temperature,
                value: Value::SignedInt(2500),
            }
            .to_string(),
            "temperature: 25°C"
        );
        assert_eq!(
            Element {
                property: Property::HumidityShort,
                value: Value::UnsignedInt(42),
            }
            .to_string(),
            "humidity: 42%"
        );
    }
}
