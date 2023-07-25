//! Support for the [BTHome](https://bthome.io/) v2 format.

use bluez_async::uuid_from_u16;
use std::{
    fmt::{self, Display, Formatter},
    mem::size_of,
};
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
    #[error("Premature end of data")]
    PrematureEnd,
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

macro_rules! generate_element {
    [$({$object_id:literal, $name:ident, $type:ty, $reader:ident, $display_name:literal, $unit:literal},)*] => {
        #[derive(Clone, Debug, Eq, PartialEq)]
        pub enum Element {
            $( $name($type), )*
        }

        impl Element {
            pub fn name(&self) -> &'static str {
                match self {
                    $( Self::$name(_) => $display_name, )*
                }
            }

            pub fn unit(&self) -> &'static str {
                match self {
                    $( Self::$name(_) => $unit, )*
                }
            }

            fn decode(data: &[u8]) -> Result<(usize, Self), DecodeError> {
                let object_id = data[0];
                let remaining = &data[1..];
                match object_id {
                    0x0a => Ok((4, Self::Energy(read_u24(remaining)?))),
                    0x4b => Ok((4, Self::Gas(read_u24(remaining)?))),
                    $( $object_id => Ok((
                        size_of::<$type>() + 1,
                        Self::$name($reader(remaining)?),
                    )), )*
                    object_id => Err(DecodeError::InvalidProperty(object_id)),
                }
            }
        }
    };
}

generate_element![
    { 0x51, Acceleration, u16, read_u16, "acceleration", "m/s²" },
    { 0x01, Battery, u8, read_u8, "battery", "%"},
    { 0x12, Co2, u16, read_u16, "CO2", "ppm"},
    { 0x09, Count8, u8, read_u8, "count", ""},
    { 0x3d, Count16, u16, read_u16, "count", ""},
    { 0x3e, Count32, u32, read_u32, "count", ""},
    { 0x43, Current, u16, read_u16, "current", "A"},
    { 0x08, Dewpoint, i16, read_i16, "dewpoint", "°C"},
    { 0x40, DistanceMm, u16, read_u16, "distance", "mm"},
    { 0x41, DistanceM, u16, read_u16, "distance", "m"},
    { 0x42, Duration, u32, read_u24, "duration", "s"},
    { 0x4d, Energy, u32, read_u32, "energy", "kWh"},
    { 0x4c, Gas, u32, read_u32, "gas", "m3"},
    { 0x52, Gyroscope, u16, read_u16, "gyroscope", "°/s"},
    { 0x03, Humidity, u16, read_u16, "humidity", "%"},
    { 0x2e, HumidityShort, u8, read_u8, "humidity", "%"},
    { 0x05, Illuminance, u32, read_u24, "illuminance", "lux"},
    { 0x06, MassKg, u16, read_u16, "mass", "kg"},
    { 0x07, MassLb, u16, read_u16, "mass", "lb"},
    { 0x14, Moisture, u16, read_u16, "moisture", "%"},
    { 0x2f, MoistureShort, u8, read_u8, "moisture", "%"},
    { 0x0d, Pm2_5, u16, read_u16, "pm2.5", "ug/m3"},
    { 0x0e, Pm10, u16, read_u16, "pm10", "ug/m3"},
    { 0x0b, Power, u32, read_u24, "power", "W"},
    { 0x04, Pressure, u32, read_u24, "pressure", "hPa"},
    { 0x3f, Rotation, i16, read_i16, "rotation", "°"},
    { 0x44, Speed, u16, read_u16, "speed", "m/s"},
    { 0x45, Temperature, i16, read_i16, "temperature", "°C"},
    { 0x02, TemperatureSmall, i16, read_i16, "temperature", "°C"},
    { 0x50, Timestamp, u32, read_u32, "timestamp", ""},
    { 0x13, Tvoc, u16, read_u16, "tvoc", "ug/m3"},
    { 0x0c, VoltageSmall, u16, read_u16, "voltage", "V"},
    { 0x4a, Voltage, u16, read_u16, "voltage", "V"},
    { 0x4e, VolumeLong, u32, read_u32, "volume", "L"},
    { 0x47, Volume, u16, read_u16, "volume", "L"},
    { 0x48, VolumeMl, u16, read_u16, "volume", "mL"},
    { 0x49, FlowRate, u16, read_u16, "volume flow rate", "m3/hr"},
    { 0x46, UvIndex, u8, read_u8, "UV index", ""},
    { 0x4f, Water, u32, read_u32, "water", "L"},
];

impl Element {
    pub fn value_int(&self) -> Option<i64> {
        match self {
            &Self::Battery(value) => Some(value.into()),
            &Self::Co2(value) => Some(value.into()),
            &Self::Count8(value) => Some(value.into()),
            &Self::Count16(value) => Some(value.into()),
            &Self::Count32(value) => Some(value.into()),
            &Self::DistanceMm(value) => Some(value.into()),
            &Self::HumidityShort(value) => Some(value.into()),
            &Self::MoistureShort(value) => Some(value.into()),
            &Self::Pm2_5(value) => Some(value.into()),
            &Self::Pm10(value) => Some(value.into()),
            &Self::Timestamp(value) => Some(value.into()),
            &Self::Tvoc(value) => Some(value.into()),
            &Self::VolumeMl(value) => Some(value.into()),
            _ => None,
        }
    }

    pub fn value_float(&self) -> Option<f64> {
        match self {
            &Self::Acceleration(value) => Some(f64::from(value) / 1000.0),
            &Self::Current(value) => Some(f64::from(value) / 1000.0),
            &Self::Dewpoint(value) => Some(f64::from(value) / 100.0),
            &Self::DistanceMm(value) => Some(f64::from(value)),
            &Self::DistanceM(value) => Some(f64::from(value) / 10.0),
            &Self::Duration(value) => Some(f64::from(value) / 1000.0),
            &Self::Energy(value) => Some(f64::from(value) / 1000.0),
            &Self::Gas(value) => Some(f64::from(value) / 1000.0),
            &Self::Gyroscope(value) => Some(f64::from(value) / 1000.0),
            &Self::Humidity(value) => Some(f64::from(value) / 100.0),
            &Self::HumidityShort(value) => Some(f64::from(value)),
            &Self::Illuminance(value) => Some(f64::from(value) / 100.0),
            &Self::MassKg(value) => Some(f64::from(value) / 100.0),
            &Self::MassLb(value) => Some(f64::from(value) / 100.0),
            &Self::Moisture(value) => Some(f64::from(value) / 100.0),
            &Self::MoistureShort(value) => Some(f64::from(value)),
            &Self::Power(value) => Some(f64::from(value) / 100.0),
            &Self::Pressure(value) => Some(f64::from(value) / 100.0),
            &Self::Rotation(value) => Some(f64::from(value) / 10.0),
            &Self::Speed(value) => Some(f64::from(value) / 100.0),
            &Self::Temperature(value) => Some(f64::from(value) / 10.0),
            &Self::TemperatureSmall(value) => Some(f64::from(value) / 100.0),
            &Self::VoltageSmall(value) => Some(f64::from(value) / 1000.0),
            &Self::Voltage(value) => Some(f64::from(value) / 10.0),
            &Self::VolumeLong(value) => Some(f64::from(value) / 1000.0),
            &Self::Volume(value) => Some(f64::from(value) / 10.0),
            &Self::VolumeMl(value) => Some(f64::from(value)),
            &Self::FlowRate(value) => Some(f64::from(value) / 1000.0),
            &Self::UvIndex(value) => Some(f64::from(value) / 10.0),
            &Self::Water(value) => Some(f64::from(value) / 1000.0),
            _ => None,
        }
    }
}

fn read_u8(data: &[u8]) -> Result<u8, DecodeError> {
    Ok(*data.get(0).ok_or(DecodeError::PrematureEnd)?)
}

fn read_u16(data: &[u8]) -> Result<u16, DecodeError> {
    Ok(u16::from_le_bytes(
        data.get(0..2)
            .ok_or(DecodeError::PrematureEnd)?
            .try_into()
            .unwrap(),
    ))
}

fn read_u24(data: &[u8]) -> Result<u32, DecodeError> {
    if let &[a, b, c, ..] = data {
        Ok(u32::from(a) | u32::from(b) << 8 | u32::from(c) << 16)
    } else {
        Err(DecodeError::PrematureEnd)
    }
}

fn read_u32(data: &[u8]) -> Result<u32, DecodeError> {
    Ok(u32::from_le_bytes(
        data.get(0..4)
            .ok_or(DecodeError::PrematureEnd)?
            .try_into()
            .unwrap(),
    ))
}

fn read_i16(data: &[u8]) -> Result<i16, DecodeError> {
    Ok(i16::from_le_bytes(
        data.get(0..2)
            .ok_or(DecodeError::PrematureEnd)?
            .try_into()
            .unwrap(),
    ))
}

impl Display for Element {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        if let Some(value) = self.value_int() {
            write!(f, "{}: {}{}", self.name(), value, self.unit())
        } else {
            write!(
                f,
                "{}: {}{}",
                self.name(),
                self.value_float().unwrap(),
                self.unit()
            )
        }
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
                elements: vec![Element::TemperatureSmall(2500)],
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
                    Element::Acceleration(22151),
                    Element::TemperatureSmall(2506),
                ]
            }
            .to_string(),
            "(unencrypted) acceleration: 22.151m/s², temperature: 25.06°C"
        );
    }
}
