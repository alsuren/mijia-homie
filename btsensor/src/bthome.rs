//! Support for the BTHome format, both v1 and v2.

pub mod events;
pub mod v1;
pub mod v2;

use self::v1::DataType;
use thiserror::Error;

/// An error encountered while decoding BTHome sensor data.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum DecodeError {
    /// Invalid data type.
    #[error("Invalid data type {0:#03x}")]
    InvalidDataType(u8),
    /// Invalid property.
    #[error("Invalid property {0:#04x}")]
    InvalidProperty(u8),
    /// Premature end of data.
    #[error("Premature end of data")]
    PrematureEnd,
    /// Extra data.
    #[error("Extra data {0:?}")]
    ExtraData(Vec<u8>),
    /// Unsupported format.
    #[error("Unsupported format {0:?}")]
    UnsupportedFormat(DataType),
    /// Invalid event type.
    #[error("Invalid event type {0:#04x}")]
    InvalidEventType(u8),
    /// Unsupported BTHome version.
    #[error("Unsupported BTHome version {0}")]
    UnsupportedVersion(u8),
    /// Invalid boolean value.
    #[error("Invalid boolean value {0:#04x}")]
    InvalidBooleanValue(u8),
}
