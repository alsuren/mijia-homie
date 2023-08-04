//! Support for the [BTHome](https://bthome.io/) v2 format.

use super::events::{ButtonEventType, DimmerEventType, Event};
use super::DecodeError;
use bluez_async::uuid_from_u16;
use std::fmt::{self, Display, Formatter};
use uuid::Uuid;

pub const UUID: Uuid = uuid_from_u16(0xfcd2);

const DEVICE_INFO_ENCRYPTED: u8 = 0x01;
const DEVICE_INFO_TRIGGER_BASED: u8 = 0x04;
const DEVICE_INFO_VERSION_MASK: u8 = 0xe0;
const DEVICE_INFO_VERSION_OFFSET: usize = 5;

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
            let (element, element_length) = Element::decode(remaining_data)?;
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

            fn decode(data: &[u8]) -> Result<(Self, usize), DecodeError> {
                let object_id = data[0];
                let remaining = &data[1..];
                match object_id {
                    0x0a => {
                        let (value, length) = read_u24(remaining)?;
                        Ok((Self::Energy(value), length + 1))
                    }
                    0x4b => {
                        let (value, length) = read_u24(remaining)?;
                        Ok((Self::Gas(value), length + 1))
                    }
                    $( $object_id => {
                        let (value, length) = $reader(remaining)?;
                        Ok((Self::$name(value), length + 1))
                    } )*
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
    { 0x15, BatteryLow, bool, read_bool, "battery low", ""},
    { 0x16, BatteryCharging, bool, read_bool, "battery charging", ""},
    { 0x17, CarbonMonoxideDetected, bool, read_bool, "carbon monoxide detected", "" },
    { 0x18, Cold, bool, read_bool, "cold", "" },
    { 0x19, Connected, bool, read_bool, "connected", "" },
    { 0x1a, DoorOpen, bool, read_bool, "door open", "" },
    { 0x1b, GarageDoorOpen, bool, read_bool, "garade door open", "" },
    { 0x1c, GasDetected, bool, read_bool, "gas detected", "" },
    { 0x0f, GenericBoolean, bool, read_bool, "generic boolean", "" },
    { 0x1d, Hot, bool, read_bool, "hot", "" },
    { 0x1e, LightDetected, bool, read_bool, "light detected", "" },
    { 0x1f, Unlocked, bool, read_bool, "unlocked", "" },
    { 0x20, Wet, bool, read_bool, "wet", "" },
    { 0x21, MotionDetected, bool, read_bool, "motion detected", "" },
    { 0x22, Moving, bool, read_bool, "moving", "" },
    { 0x23, OccupancyDetected, bool, read_bool, "occupancy detected", "" },
    { 0x11, Open, bool, read_bool, "open", "" },
    { 0x24, Plugged, bool, read_bool, "plugged in", "" },
    { 0x10, PowerOn, bool, read_bool, "power on", "" },
    { 0x25, Present, bool, read_bool, "home", "" },
    { 0x26, Problem, bool, read_bool, "problem", "" },
    { 0x27, Running, bool, read_bool, "running", "" },
    { 0x28, Safe, bool, read_bool, "safe", "" },
    { 0x29, SmokeDetected, bool, read_bool, "smoke detected", "" },
    { 0x2a, Sound, bool, read_bool, "sound detected", "" },
    { 0x2b, Tamper, bool, read_bool, "tampered", "" },
    { 0x2c, VibrationDetected, bool, read_bool, "vibration detected", "" },
    { 0x2d, WindowOpen, bool, read_bool, "window open", "" },
    { 0x3a, ButtonEvent, Option<ButtonEventType>, read_button_event, "button event", "" },
    { 0x3c, DimmerEvent, Option<DimmerEventType>, read_dimmer_event, "dimmer event", "" },
];

impl Element {
    pub fn value_bool(&self) -> Option<bool> {
        match *self {
            Self::BatteryCharging(value)
            | Self::BatteryLow(value)
            | Self::CarbonMonoxideDetected(value)
            | Self::Cold(value)
            | Self::Connected(value)
            | Self::DoorOpen(value)
            | Self::GarageDoorOpen(value)
            | Self::GasDetected(value)
            | Self::GenericBoolean(value)
            | Self::Hot(value)
            | Self::LightDetected(value)
            | Self::Unlocked(value)
            | Self::Wet(value)
            | Self::MotionDetected(value)
            | Self::Moving(value)
            | Self::OccupancyDetected(value)
            | Self::Open(value)
            | Self::Plugged(value)
            | Self::PowerOn(value)
            | Self::Present(value)
            | Self::Problem(value)
            | Self::Running(value)
            | Self::Safe(value)
            | Self::SmokeDetected(value)
            | Self::Sound(value)
            | Self::Tamper(value)
            | Self::VibrationDetected(value)
            | Self::WindowOpen(value) => Some(value),
            _ => None,
        }
    }

    pub fn value_int(&self) -> Option<i64> {
        match *self {
            Self::Battery(value) => Some(value.into()),
            Self::Co2(value) => Some(value.into()),
            Self::Count8(value) => Some(value.into()),
            Self::Count16(value) => Some(value.into()),
            Self::Count32(value) => Some(value.into()),
            Self::DistanceMm(value) => Some(value.into()),
            Self::HumidityShort(value) => Some(value.into()),
            Self::MoistureShort(value) => Some(value.into()),
            Self::Pm2_5(value) => Some(value.into()),
            Self::Pm10(value) => Some(value.into()),
            Self::Timestamp(value) => Some(value.into()),
            Self::Tvoc(value) => Some(value.into()),
            Self::VolumeMl(value) => Some(value.into()),
            _ => None,
        }
    }

    pub fn value_float(&self) -> Option<f64> {
        match *self {
            Self::Acceleration(value) => Some(f64::from(value) / 1000.0),
            Self::Current(value) => Some(f64::from(value) / 1000.0),
            Self::Dewpoint(value) => Some(f64::from(value) / 100.0),
            Self::DistanceMm(value) => Some(f64::from(value)),
            Self::DistanceM(value) => Some(f64::from(value) / 10.0),
            Self::Duration(value) => Some(f64::from(value) / 1000.0),
            Self::Energy(value) => Some(f64::from(value) / 1000.0),
            Self::Gas(value) => Some(f64::from(value) / 1000.0),
            Self::Gyroscope(value) => Some(f64::from(value) / 1000.0),
            Self::Humidity(value) => Some(f64::from(value) / 100.0),
            Self::HumidityShort(value) => Some(f64::from(value)),
            Self::Illuminance(value) => Some(f64::from(value) / 100.0),
            Self::MassKg(value) => Some(f64::from(value) / 100.0),
            Self::MassLb(value) => Some(f64::from(value) / 100.0),
            Self::Moisture(value) => Some(f64::from(value) / 100.0),
            Self::MoistureShort(value) => Some(f64::from(value)),
            Self::Power(value) => Some(f64::from(value) / 100.0),
            Self::Pressure(value) => Some(f64::from(value) / 100.0),
            Self::Rotation(value) => Some(f64::from(value) / 10.0),
            Self::Speed(value) => Some(f64::from(value) / 100.0),
            Self::Temperature(value) => Some(f64::from(value) / 10.0),
            Self::TemperatureSmall(value) => Some(f64::from(value) / 100.0),
            Self::VoltageSmall(value) => Some(f64::from(value) / 1000.0),
            Self::Voltage(value) => Some(f64::from(value) / 10.0),
            Self::VolumeLong(value) => Some(f64::from(value) / 1000.0),
            Self::Volume(value) => Some(f64::from(value) / 10.0),
            Self::VolumeMl(value) => Some(f64::from(value)),
            Self::FlowRate(value) => Some(f64::from(value) / 1000.0),
            Self::UvIndex(value) => Some(f64::from(value) / 10.0),
            Self::Water(value) => Some(f64::from(value) / 1000.0),
            _ => None,
        }
    }

    pub fn event(&self) -> Option<Event> {
        match *self {
            Self::ButtonEvent(event_type) => Some(Event::Button(event_type)),
            Self::DimmerEvent(event_type) => Some(Event::Dimmer(event_type)),
            _ => None,
        }
    }
}

fn read_u8(data: &[u8]) -> Result<(u8, usize), DecodeError> {
    Ok((*data.first().ok_or(DecodeError::PrematureEnd)?, 1))
}

fn read_u16(data: &[u8]) -> Result<(u16, usize), DecodeError> {
    Ok((
        u16::from_le_bytes(
            data.get(0..2)
                .ok_or(DecodeError::PrematureEnd)?
                .try_into()
                .unwrap(),
        ),
        2,
    ))
}

fn read_u24(data: &[u8]) -> Result<(u32, usize), DecodeError> {
    if let &[a, b, c, ..] = data {
        Ok((u32::from(a) | u32::from(b) << 8 | u32::from(c) << 16, 3))
    } else {
        Err(DecodeError::PrematureEnd)
    }
}

fn read_u32(data: &[u8]) -> Result<(u32, usize), DecodeError> {
    Ok((
        u32::from_le_bytes(
            data.get(0..4)
                .ok_or(DecodeError::PrematureEnd)?
                .try_into()
                .unwrap(),
        ),
        4,
    ))
}

fn read_i16(data: &[u8]) -> Result<(i16, usize), DecodeError> {
    Ok((
        i16::from_le_bytes(
            data.get(0..2)
                .ok_or(DecodeError::PrematureEnd)?
                .try_into()
                .unwrap(),
        ),
        2,
    ))
}

fn read_bool(data: &[u8]) -> Result<(bool, usize), DecodeError> {
    let (value, length) = read_u8(data)?;
    let value = match value {
        0x00 => false,
        0x01 => true,
        _ => return Err(DecodeError::InvalidBooleanValue(value)),
    };
    Ok((value, length))
}

fn read_button_event(data: &[u8]) -> Result<(Option<ButtonEventType>, usize), DecodeError> {
    let event_type = ButtonEventType::from_bytes(data.get(0..1).ok_or(DecodeError::PrematureEnd)?)?;
    Ok((event_type, 1))
}

fn read_dimmer_event(data: &[u8]) -> Result<(Option<DimmerEventType>, usize), DecodeError> {
    if data[0] == 0x00 {
        Ok((None, 1))
    } else {
        let event_type =
            DimmerEventType::from_bytes(data.get(0..2).ok_or(DecodeError::PrematureEnd)?)?;
        Ok((event_type, 2))
    }
}

impl Display for Element {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        if let Some(event) = self.event() {
            event.fmt(f)
        } else if let Some(value) = self.value_bool() {
            write!(f, "{}: {}{}", self.name(), value, self.unit())
        } else if let Some(value) = self.value_int() {
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
    fn decode_u24() {
        assert_eq!(
            BtHomeV2::decode(&[0x40, 0x42, 0x4e, 0x34, 0x00, 0x0a, 0x13, 0x8a, 0x14]).unwrap(),
            BtHomeV2 {
                encrypted: false,
                trigger_based: false,
                elements: vec![Element::Duration(13390), Element::Energy(1346067)],
            }
        );
    }

    #[test]
    fn decode_bool() {
        assert_eq!(
            BtHomeV2::decode(&[0x40, 0x15, 0x01, 0x16, 0x00]).unwrap(),
            BtHomeV2 {
                encrypted: false,
                trigger_based: false,
                elements: vec![Element::BatteryLow(true), Element::BatteryCharging(false)],
            }
        );
    }

    #[test]
    fn decode_button_events() {
        assert_eq!(
            BtHomeV2::decode(&[0x44, 0x3a, 0x00, 0x3a, 0x05]).unwrap(),
            BtHomeV2 {
                encrypted: false,
                trigger_based: true,
                elements: vec![
                    Element::ButtonEvent(None),
                    Element::ButtonEvent(Some(ButtonEventType::LongDoublePress)),
                ],
            }
        );
    }

    #[test]
    fn decode_dimmer_events() {
        assert_eq!(
            BtHomeV2::decode(&[0x44, 0x3c, 0x00, 0x3c, 0x01, 0x03]).unwrap(),
            BtHomeV2 {
                encrypted: false,
                trigger_based: true,
                elements: vec![
                    Element::DimmerEvent(None),
                    Element::DimmerEvent(Some(DimmerEventType::RotateLeft(3))),
                ],
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
                    Element::BatteryCharging(true),
                    Element::ButtonEvent(None),
                    Element::ButtonEvent(Some(ButtonEventType::LongDoublePress)),
                    Element::DimmerEvent(Some(DimmerEventType::RotateLeft(3))),
                ]
            }
            .to_string(),
            "(unencrypted) acceleration: 22.151m/s², temperature: 25.06°C, battery charging: true, button: none, button: long double press, dimmer: rotate left 3 steps"
        );
    }
}
