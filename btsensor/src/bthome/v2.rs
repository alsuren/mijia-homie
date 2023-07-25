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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Element {
    Acceleration(u16),
    Battery(u8),
    Co2(u16),
    Count8(u8),
    Count16(u16),
    Count32(u32),
    Current(u16),
    Dewpoint(i16),
    DistanceMm(u16),
    DistanceM(u16),
    Duration(u32),
    Energy(u32),
    Gas(u32),
    Gyroscope(u16),
    Humidity(u16),
    HumidityShort(u8),
    Illuminance(u32),
    MassKg(u16),
    MassLb(u16),
    Moisture(u16),
    MoistureShort(u8),
    Pm2_5(u16),
    Pm10(u16),
    Power(u32),
    Pressure(u32),
    Rotation(i16),
    Speed(u16),
    Temperature(i16),
    TemperatureSmall(i16),
    Timestamp(u32),
    Tvoc(u16),
    VoltageSmall(u16),
    Voltage(u16),
    VolumeLong(u32),
    Volume(u16),
    VolumeMl(u16),
    FlowRate(u16),
    UvIndex(u8),
    Water(u32),
}

impl Element {
    fn decode(data: &[u8]) -> Result<(usize, Self), DecodeError> {
        let object_id = data[0];
        let remaining = &data[1..];
        match object_id {
            0x51 => Ok((3, Self::Acceleration(read_u16(remaining)?))),
            0x01 => Ok((2, Self::Battery(read_u8(remaining)?))),
            0x12 => Ok((3, Self::Co2(read_u16(remaining)?))),
            0x09 => Ok((2, Self::Count8(read_u8(remaining)?))),
            0x3d => Ok((3, Self::Count16(read_u16(remaining)?))),
            0x3e => Ok((5, Self::Count32(read_u32(remaining)?))),
            0x43 => Ok((3, Self::Current(read_u16(remaining)?))),
            0x08 => Ok((3, Self::Dewpoint(read_i16(remaining)?))),
            0x40 => Ok((3, Self::DistanceMm(read_u16(remaining)?))),
            0x41 => Ok((3, Self::DistanceM(read_u16(remaining)?))),
            0x02 => Ok((3, Self::TemperatureSmall(read_i16(remaining)?))),
            object_id => Err(DecodeError::InvalidProperty(object_id)),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Acceleration(_) => "acceleration",
            Self::Battery(_) => "battery",
            Self::Co2(_) => "CO2",
            Self::Count8(_) | Self::Count16(_) | Self::Count32(_) => "count",
            Self::Current(_) => "current",
            Self::Dewpoint(_) => "dewpoint",
            Self::DistanceMm(_) | Self::DistanceM(_) => "distance",
            Self::Duration(_) => "duration",
            Self::Energy(_) => "energy",
            Self::Gas(_) => "gas",
            Self::Gyroscope(_) => "gyroscope",
            Self::Humidity(_) | Self::HumidityShort(_) => "humidity",
            Self::Illuminance(_) => "illuminance",
            Self::MassKg(_) | Self::MassLb(_) => "mass",
            Self::Moisture(_) | Self::MoistureShort(_) => "moisture",
            Self::Pm2_5(_) => "pm2.5",
            Self::Pm10(_) => "pm10",
            Self::Power(_) => "power",
            Self::Pressure(_) => "pressure",
            Self::Rotation(_) => "rotation",
            Self::Speed(_) => "speed",
            Self::Temperature(_) | Self::TemperatureSmall(_) => "temperature",
            Self::Timestamp(_) => "timestamp",
            Self::Tvoc(_) => "tvoc",
            Self::VoltageSmall(_) | Self::Voltage(_) => "voltage",
            Self::VolumeLong(_) | Self::Volume(_) | Self::VolumeMl(_) => "volume",
            Self::FlowRate(_) => "volume flow rate",
            Self::UvIndex(_) => "UV index",
            Self::Water(_) => "water",
        }
    }

    pub fn unit(&self) -> &'static str {
        match self {
            Self::Acceleration(_) => "m/s²",
            Self::Battery(_)
            | Self::Humidity(_)
            | Self::HumidityShort(_)
            | Self::Moisture(_)
            | Self::MoistureShort(_) => "%",
            Self::Co2(_) => "ppm",
            Self::Count8(_)
            | Self::Count16(_)
            | Self::Count32(_)
            | Self::Timestamp(_)
            | Self::UvIndex(_) => "",
            Self::Current(_) => "A",
            Self::Dewpoint(_) | Self::Temperature(_) | Self::TemperatureSmall(_) => "°C",
            Self::DistanceMm(_) => "mm",
            Self::DistanceM(_) => "m",
            Self::Duration(_) => "s",
            Self::Energy(_) => "kWh",
            Self::Gas(_) => "m3",
            Self::Gyroscope(_) => "°/s",
            Self::Illuminance(_) => "lux",
            Self::MassKg(_) => "kg",
            Self::MassLb(_) => "lb",
            Self::Pm2_5(_) | Self::Pm10(_) | Self::Tvoc(_) => "ug/m3",
            Self::Power(_) => "W",
            Self::Pressure(_) => "hPa",
            Self::Rotation(_) => "°",
            Self::Speed(_) => "m/s",
            Self::VoltageSmall(_) | Self::Voltage(_) => "V",
            Self::VolumeLong(_) | Self::Volume(_) | Self::Water(_) => "L",
            Self::VolumeMl(_) => "mL",
            Self::FlowRate(_) => "m3/hr",
        }
    }

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
