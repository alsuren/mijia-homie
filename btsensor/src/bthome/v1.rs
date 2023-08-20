//! Support for the [BTHome](https://bthome.io/) v1 format.

use super::events::{ButtonEventType, DimmerEventType, Event};
use super::DecodeError;
use crate::uuid_from_u16;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use std::fmt::{self, Display, Formatter};
use uuid::Uuid;

/// The service data UUID used for unencrypted BTHome v1 advertisements.
pub const UNENCRYPTED_UUID: Uuid = uuid_from_u16(0x181c);

/// The service data UUID used for encrypted BTHome v1 advertisements.
pub const ENCRYPTED_UUID: Uuid = uuid_from_u16(0x181e);

/// A single element of a BTHome v1 advertisement: either a sensor reading or an event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Element {
    Sensor(Sensor),
    Event(Event),
}

impl Element {
    /// Constructs a new sensor reading element for the given property, with an unsigned integer
    /// value.
    pub fn new_unsigned(property: Property, value: u32) -> Self {
        Self::Sensor(Sensor {
            property,
            value: Value::UnsignedInt(value),
        })
    }

    /// Constructs a new sensor reading element for the given property, with a signed integer value.
    pub fn new_signed(property: Property, value: i32) -> Self {
        Self::Sensor(Sensor {
            property,
            value: Value::SignedInt(value),
        })
    }

    /// Constructs a new element for the given event.
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

    /// Attempts to decode the given service data as a BTHome v1 advertisement.
    pub fn decode_all(mut data: &[u8]) -> Result<Vec<Self>, DecodeError> {
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
            elements.push(Self::decode(format, &data[1..element_end])?);

            data = &data[element_end..];
        }

        if data.is_empty() {
            Ok(elements)
        } else {
            Err(DecodeError::ExtraData(data.to_owned()))
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

/// A sensor reading from a device.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Sensor {
    /// The property for which the reading is taken. This implies the unit and scale.
    pub property: Property,
    /// The raw value of the reading as sent by the device, before the scaling factor for the
    /// property is applied.
    value: Value,
}

impl Sensor {
    /// Returns the value of the reading as a floating-point number, properly scaled according to
    /// the property it is for.
    pub fn value_float(&self) -> f64 {
        f64::from(&self.value) / self.property.denominator()
    }

    /// Returns the integer value of the reading, if it is for a property with no scaling factor.
    ///
    /// Returns `None` if the property has a scaling factor meaning that the value may not be an
    /// integer.
    pub fn value_int(&self) -> Option<i64> {
        if self.property.denominator() == 1.0 {
            Some((&self.value).into())
        } else {
            None
        }
    }

    /// Returns the boolean value of the reading, if it is a boolean property and has a valid value.
    pub fn value_bool(&self) -> Option<bool> {
        if self.property.is_boolean() {
            match &self.value {
                Value::UnsignedInt(0) => Some(false),
                Value::UnsignedInt(1) => Some(true),
                _ => None,
            }
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
        if let Some(value) = self.value_bool() {
            write!(f, "{}: {}", self.property, value)
        } else if let Some(value) = self.value_int() {
            write!(f, "{}: {}{}", self.property, value, self.property.unit())
        } else {
            write!(
                f,
                "{}: {}{}",
                self.property,
                self.value_float(),
                self.property.unit(),
            )
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, IntoPrimitive, PartialEq, TryFromPrimitive)]
#[num_enum(error_type(name = DecodeError, constructor = DecodeError::InvalidDataType))]
#[repr(u8)]
pub enum DataType {
    UnsignedInt = 0b000,
    SignedInt = 0b001,
    Float = 0b010,
    String = 0b011,
    Mac = 0b100,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Value {
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

/// A BTHome v1 property type.
///
/// Each property type has an associated unit and scale defined by the
/// standard.
#[derive(Copy, Clone, Debug, Eq, IntoPrimitive, PartialEq, TryFromPrimitive)]
#[num_enum(error_type(name = DecodeError, constructor = DecodeError::InvalidProperty))]
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

impl Display for Property {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl Property {
    /// Returns the name of the property.
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

    /// Returns the standard unit for the property.
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

    /// The denominator for fixed-point values.
    ///
    /// In other words, the value stored should be divided by this number to get the actual value.
    fn denominator(self) -> f64 {
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
            | Self::DimmerEvent => 1.0,
            Self::Temperature
            | Self::Humidity
            | Self::Pressure
            | Self::Illuminance
            | Self::MassKg
            | Self::MassLb
            | Self::Dewpoint
            | Self::Power
            | Self::Moisture => 100.0,
            Self::Energy | Self::Voltage | Self::Acceleration | Self::Gyroscope => 1000.0,
        }
    }

    fn is_boolean(self) -> bool {
        matches!(
            self,
            Self::GenericBoolean
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
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_valid() {
        assert_eq!(
            Element::decode_all(&[0x23, 0x02, 0xC4, 0x09, 0x03, 0x03, 0xBF, 0x13]).unwrap(),
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
            Element::decode_all(&[2, 0, 140, 35, 2, 203, 8, 3, 3, 171, 20, 2, 1, 100]).unwrap(),
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
            Element::decode_all(&[2, 0, 137, 2, 16, 0, 3, 12, 182, 11]).unwrap(),
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
            Element::decode_all(&[0x02, 0x3a, 0x00, 0x02, 0x3a, 0x05]).unwrap(),
            vec![
                Element::Event(Event::Button(None)),
                Element::Event(Event::Button(Some(ButtonEventType::LongDoublePress))),
            ]
        );
    }

    #[test]
    fn decode_dimmer_events() {
        assert_eq!(
            Element::decode_all(&[0x02, 0x3c, 0x00, 0x03, 0x3c, 0x01, 0x03]).unwrap(),
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
    fn format_boolean_element() {
        assert_eq!(
            Element::Sensor(Sensor {
                property: Property::BatteryLow,
                value: Value::UnsignedInt(0),
            })
            .to_string(),
            "battery low: false"
        );
        assert_eq!(
            Element::Sensor(Sensor {
                property: Property::LightDetected,
                value: Value::UnsignedInt(1),
            })
            .to_string(),
            "light detected: true"
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
