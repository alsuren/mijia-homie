//! Support for the [BTHome](https://bthome.io/) v1 format.

use super::events::{ButtonEventType, DimmerEventType, Event};
use super::DecodeError;
use bluez_async::uuid_from_u16;
use num_enum::IntoPrimitive;
use std::fmt::{self, Display, Formatter};
use uuid::Uuid;

pub const UNENCRYPTED_UUID: Uuid = uuid_from_u16(0x181c);
pub const ENCRYPTED_UUID: Uuid = uuid_from_u16(0x181e);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Element {
    Sensor(Sensor),
    Event(Event),
}

impl Element {
    pub fn new_unsigned(property: Property, value: u32) -> Self {
        Self::Sensor(Sensor {
            property,
            value: Value::UnsignedInt(value),
        })
    }

    pub fn new_signed(property: Property, value: i32) -> Self {
        Self::Sensor(Sensor {
            property,
            value: Value::SignedInt(value),
        })
    }

    pub fn new_event(event: Event) -> Self {
        Self::Event(event)
    }

    fn decode(format: DataType, data: &[u8]) -> Result<Self, DecodeError> {
        let property = Property::try_from(data[0])?;
        let value = &data[1..];
        match property {
            Property::ButtonEvent => {
                let event_type = ButtonEventType::from_bytes(value)?;
                let event = Event::Button(event_type);
                Ok(Self::Event(event))
            }
            Property::DimmerEvent => {
                let event_type = DimmerEventType::from_bytes(value)?;
                let event = Event::Dimmer(event_type);
                Ok(Self::Event(event))
            }
            _ => Ok(Self::Sensor(Sensor::decode(format, property, value)?)),
        }
    }
}

impl Display for Element {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::Sensor(sensor) => sensor.fmt(f),
            Self::Event(event) => event.fmt(f),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sensor {
    pub property: Property,
    value: Value,
}

impl Sensor {
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

    fn decode(format: DataType, property: Property, value: &[u8]) -> Result<Self, DecodeError> {
        let value = Value::from_bytes(value, format)?;
        Ok(Self { property, value })
    }
}

impl Display for Sensor {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        // TODO: Special handling for timestamp.
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

#[derive(Copy, Clone, Debug, Eq, IntoPrimitive, PartialEq)]
#[repr(u8)]
pub enum DataType {
    UnsignedInt = 0b000,
    SignedInt = 0b001,
    Float = 0b010,
    String = 0b011,
    Mac = 0b100,
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
        match *value {
            Value::SignedInt(v) => v.into(),
            Value::UnsignedInt(v) => v.into(),
        }
    }
}

impl From<&Value> for i64 {
    fn from(value: &Value) -> Self {
        match *value {
            Value::SignedInt(v) => v.into(),
            Value::UnsignedInt(v) => v.into(),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, IntoPrimitive, PartialEq)]
#[repr(u8)]
pub enum Property {
    // Misc data.
    PacketId = 0x00,

    // Sensor data.
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

    // Binary sensor data.
    GenericBoolean = 0x0f,
    PowerOn = 0x10,
    Open = 0x11,
    BatteryLow = 0x15,
    BatteryCharging = 0x16,
    CarbonMonoxideDetected = 0x17,
    Cold = 0x18,
    Connected = 0x19,
    DoorOpen = 0x1a,
    GarageDoorOpen = 0x1b,
    GasDetected = 0x1c,
    HeatAbnormal = 0x1d,
    LightDetected = 0x1e,
    Unlocked = 0x1f,
    Wet = 0x20,
    MotionDetected = 0x21,
    Moving = 0x22,
    OccupancyDetected = 0x23,
    PluggedIn = 0x24,
    Home = 0x25,
    Problem = 0x26,
    Running = 0x27,
    Safe = 0x28,
    SmokeDetected = 0x29,
    Sound = 0x2a,
    Tamper = 0x2b,
    VibrationDetected = 0x2c,
    WindowOpen = 0x2d,

    // Events.
    ButtonEvent = 0x3a,
    DimmerEvent = 0x3c,
}

impl TryFrom<u8> for Property {
    type Error = DecodeError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::PacketId),
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
            0x0f => Ok(Self::GenericBoolean),
            0x10 => Ok(Self::PowerOn),
            0x11 => Ok(Self::Open),
            0x15 => Ok(Self::BatteryLow),
            0x16 => Ok(Self::BatteryCharging),
            0x17 => Ok(Self::CarbonMonoxideDetected),
            0x18 => Ok(Self::Cold),
            0x19 => Ok(Self::Connected),
            0x1a => Ok(Self::DoorOpen),
            0x1b => Ok(Self::GarageDoorOpen),
            0x1c => Ok(Self::GasDetected),
            0x1d => Ok(Self::HeatAbnormal),
            0x1e => Ok(Self::LightDetected),
            0x1f => Ok(Self::Unlocked),
            0x20 => Ok(Self::Wet),
            0x21 => Ok(Self::MotionDetected),
            0x22 => Ok(Self::Moving),
            0x23 => Ok(Self::OccupancyDetected),
            0x24 => Ok(Self::PluggedIn),
            0x25 => Ok(Self::Home),
            0x26 => Ok(Self::Problem),
            0x27 => Ok(Self::Running),
            0x28 => Ok(Self::Safe),
            0x29 => Ok(Self::SmokeDetected),
            0x2a => Ok(Self::Sound),
            0x2b => Ok(Self::Tamper),
            0x2c => Ok(Self::VibrationDetected),
            0x2d => Ok(Self::WindowOpen),
            0x3a => Ok(Self::ButtonEvent),
            0x3c => Ok(Self::DimmerEvent),
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
            Self::PacketId => "packet ID",
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
            Self::GenericBoolean => "generic boolean",
            Self::PowerOn => "power on",
            Self::Open => "open",
            Self::BatteryLow => "battery low",
            Self::BatteryCharging => "battery charging",
            Self::CarbonMonoxideDetected => "carbon monoxide detected",
            Self::Cold => "cold",
            Self::Connected => "connected",
            Self::DoorOpen => "door open",
            Self::GarageDoorOpen => "garage door open",
            Self::GasDetected => "gas detected",
            Self::HeatAbnormal => "abnormal heat",
            Self::LightDetected => "light detected",
            Self::Unlocked => "unlocked",
            Self::Wet => "wet",
            Self::MotionDetected => "motion detected",
            Self::Moving => "moving",
            Self::OccupancyDetected => "occupancy detected",
            Self::PluggedIn => "plugged in",
            Self::Home => "home",
            Self::Problem => "problem",
            Self::Running => "running",
            Self::Safe => "safe",
            Self::SmokeDetected => "smoke detected",
            Self::Sound => "sound detected",
            Self::Tamper => "tampered",
            Self::VibrationDetected => "vibration detected",
            Self::WindowOpen => "window open",
            Self::ButtonEvent => "button event",
            Self::DimmerEvent => "dimmer event",
        }
    }

    pub fn unit(self) -> &'static str {
        match self {
            Self::PacketId
            | Self::Count
            | Self::Timestamp
            | Self::GenericBoolean
            | Self::PowerOn
            | Self::Open
            | Self::BatteryLow
            | Self::BatteryCharging
            | Self::CarbonMonoxideDetected
            | Self::Cold
            | Self::Connected
            | Self::DoorOpen
            | Self::GarageDoorOpen
            | Self::GasDetected
            | Self::HeatAbnormal
            | Self::LightDetected
            | Self::Unlocked
            | Self::Wet
            | Self::MotionDetected
            | Self::Moving
            | Self::OccupancyDetected
            | Self::PluggedIn
            | Self::Home
            | Self::Problem
            | Self::Running
            | Self::Safe
            | Self::SmokeDetected
            | Self::Sound
            | Self::Tamper
            | Self::VibrationDetected
            | Self::WindowOpen
            | Self::ButtonEvent
            | Self::DimmerEvent => "",
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
            Self::PacketId
            | Self::Battery
            | Self::HumidityShort
            | Self::Count
            | Self::Pm2_5
            | Self::Pm10
            | Self::Co2
            | Self::Tvoc
            | Self::MoistureShort
            | Self::Timestamp
            | Self::GenericBoolean
            | Self::PowerOn
            | Self::Open
            | Self::BatteryLow
            | Self::BatteryCharging
            | Self::CarbonMonoxideDetected
            | Self::Cold
            | Self::Connected
            | Self::DoorOpen
            | Self::GarageDoorOpen
            | Self::GasDetected
            | Self::HeatAbnormal
            | Self::LightDetected
            | Self::Unlocked
            | Self::Wet
            | Self::MotionDetected
            | Self::Moving
            | Self::OccupancyDetected
            | Self::PluggedIn
            | Self::Home
            | Self::Problem
            | Self::Running
            | Self::Safe
            | Self::SmokeDetected
            | Self::Sound
            | Self::Tamper
            | Self::VibrationDetected
            | Self::WindowOpen
            | Self::ButtonEvent
            | Self::DimmerEvent => 0,
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
        let element_end = usize::from(length) + 1;
        // length includes the measurement type byte but not the length/format byte.
        if data.len() <= length.into() {
            return Err(DecodeError::PrematureEnd);
        }
        let format = ((length_format & 0xe0) >> 5).try_into()?;
        elements.push(Element::decode(format, &data[1..element_end])?);

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
                Element::Sensor(Sensor {
                    property: Property::Temperature,
                    value: Value::SignedInt(2500),
                }),
                Element::Sensor(Sensor {
                    property: Property::Humidity,
                    value: Value::UnsignedInt(5055),
                }),
            ]
        );
        assert_eq!(
            decode(&[2, 0, 140, 35, 2, 203, 8, 3, 3, 171, 20, 2, 1, 100]).unwrap(),
            vec![
                Element::Sensor(Sensor {
                    property: Property::PacketId,
                    value: Value::UnsignedInt(140),
                }),
                Element::Sensor(Sensor {
                    property: Property::Temperature,
                    value: Value::SignedInt(2251),
                }),
                Element::Sensor(Sensor {
                    property: Property::Humidity,
                    value: Value::UnsignedInt(5291),
                }),
                Element::Sensor(Sensor {
                    property: Property::Battery,
                    value: Value::UnsignedInt(100),
                }),
            ]
        );
        assert_eq!(
            decode(&[2, 0, 137, 2, 16, 0, 3, 12, 182, 11]).unwrap(),
            vec![
                Element::Sensor(Sensor {
                    property: Property::PacketId,
                    value: Value::UnsignedInt(137),
                }),
                Element::Sensor(Sensor {
                    property: Property::PowerOn,
                    value: Value::UnsignedInt(0),
                }),
                Element::Sensor(Sensor {
                    property: Property::Voltage,
                    value: Value::UnsignedInt(2998),
                }),
            ]
        );
    }

    #[test]
    fn decode_button_events() {
        assert_eq!(
            decode(&[0x02, 0x3a, 0x00, 0x02, 0x3a, 0x05]).unwrap(),
            vec![
                Element::Event(Event::Button(None)),
                Element::Event(Event::Button(Some(ButtonEventType::LongDoublePress))),
            ]
        );
    }

    #[test]
    fn decode_dimmer_events() {
        assert_eq!(
            decode(&[0x02, 0x3c, 0x00, 0x03, 0x3c, 0x01, 0x03]).unwrap(),
            vec![
                Element::Event(Event::Dimmer(None)),
                Element::Event(Event::Dimmer(Some(DimmerEventType::RotateLeft(3)))),
            ]
        );
    }

    #[test]
    fn format_sensor_element() {
        assert_eq!(
            Element::Sensor(Sensor {
                property: Property::Humidity,
                value: Value::UnsignedInt(5055),
            })
            .to_string(),
            "humidity: 50.55%"
        );
        assert_eq!(
            Element::Sensor(Sensor {
                property: Property::Temperature,
                value: Value::SignedInt(2500),
            })
            .to_string(),
            "temperature: 25°C"
        );
        assert_eq!(
            Element::Sensor(Sensor {
                property: Property::HumidityShort,
                value: Value::UnsignedInt(42),
            })
            .to_string(),
            "humidity: 42%"
        );
    }

    #[test]
    fn format_event_element() {
        assert_eq!(
            Element::Event(Event::Button(None)).to_string(),
            "button: none"
        );
        assert_eq!(
            Element::Event(Event::Button(Some(ButtonEventType::LongDoublePress))).to_string(),
            "button: long double press"
        );
        assert_eq!(
            Element::Event(Event::Dimmer(Some(DimmerEventType::RotateRight(42)))).to_string(),
            "dimmer: rotate right 42 steps"
        );
    }
}
