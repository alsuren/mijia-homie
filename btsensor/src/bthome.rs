pub mod events;
pub mod v1;
pub mod v2;

use self::v1::DataType;
use thiserror::Error;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum DecodeError {
    #[error("Invalid data type {0:#03x}")]
    InvalidDataType(u8),
    #[error("Invalid property {0:#04x}")]
    InvalidProperty(u8),
    #[error("Premature end of data")]
    PrematureEnd,
    #[error("Extra data {0:?}")]
    ExtraData(Vec<u8>),
    #[error("Unsupported format {0:?}")]
    UnsupportedFormat(DataType),
    #[error("Invalid event type {0:#04x}")]
    InvalidEventType(u8),
    #[error("Unsupported BTHome version {0}")]
    UnsupportedVersion(u8),
    #[error("Invalid boolean value {0:#04x}")]
    InvalidBooleanValue(u8),
}
