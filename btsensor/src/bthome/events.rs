//! Types related to BTHome events, shared between both v1 and v2.

use super::{v1::Property, DecodeError};
use num_enum::IntoPrimitive;
use std::fmt::{self, Display, Formatter};

/// The particular type of a button event, indicating how the button was pressed.
#[derive(Copy, Clone, Debug, Eq, IntoPrimitive, PartialEq)]
#[repr(u8)]
pub enum ButtonEventType {
    Press = 0x01,
    DoublePress = 0x02,
    TriplePress = 0x03,
    LongPress = 0x04,
    LongDoublePress = 0x05,
    LongTriplePress = 0x06,
}

impl ButtonEventType {
    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Option<Self>, DecodeError> {
        match bytes {
            [0x00] => Ok(None),
            [0x01] => Ok(Some(Self::Press)),
            [0x02] => Ok(Some(Self::DoublePress)),
            [0x03] => Ok(Some(Self::TriplePress)),
            [0x04] => Ok(Some(Self::LongPress)),
            [0x05] => Ok(Some(Self::LongDoublePress)),
            [0x06] => Ok(Some(Self::LongTriplePress)),
            [value] => Err(DecodeError::InvalidEventType(*value)),
            [] => Err(DecodeError::PrematureEnd),
            _ => Err(DecodeError::ExtraData(bytes.to_owned())),
        }
    }

    /// Returns a string describing how the button was pressed.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Press => "press",
            Self::DoublePress => "double press",
            Self::TriplePress => "triple press",
            Self::LongPress => "long press",
            Self::LongDoublePress => "long double press",
            Self::LongTriplePress => "long triple press",
        }
    }
}

impl Display for ButtonEventType {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Details of a dimmer event, including which direction it was rotated and by how many steps.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DimmerEventType {
    RotateLeft(u8),
    RotateRight(u8),
}

impl DimmerEventType {
    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Option<Self>, DecodeError> {
        match bytes {
            [0x00] => Ok(None),
            [0x01, steps] => Ok(Some(Self::RotateLeft(*steps))),
            [0x02, steps] => Ok(Some(Self::RotateRight(*steps))),
            [value] => Err(DecodeError::InvalidEventType(*value)),
            [] => Err(DecodeError::PrematureEnd),
            _ => Err(DecodeError::ExtraData(bytes.to_owned())),
        }
    }
}

impl Display for DimmerEventType {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::RotateLeft(steps) => write!(f, "rotate left {steps} steps"),
            Self::RotateRight(steps) => write!(f, "rotate right {steps} steps"),
        }
    }
}

/// A BTHome event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    Button(Option<ButtonEventType>),
    Dimmer(Option<DimmerEventType>),
}

impl Event {
    /// The BTHome v1 property for the event.
    pub fn property(&self) -> Property {
        match self {
            Self::Button(_) => Property::ButtonEvent,
            Self::Dimmer(_) => Property::DimmerEvent,
        }
    }
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::Button(None) => f.write_str("button: none"),
            Self::Button(Some(event_type)) => write!(f, "button: {event_type}"),
            Self::Dimmer(None) => f.write_str("dimmer: none"),
            Self::Dimmer(Some(event_type)) => write!(f, "dimmer: {event_type}"),
        }
    }
}
