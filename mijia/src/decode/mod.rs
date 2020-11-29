pub mod comfort_level;
pub mod readings;
pub mod temperature_unit;
pub mod time;

use std::time::SystemTime;
use thiserror::Error;

const TEMPERATURE_MAX: f32 = i16::MAX as f32 * 0.01;
const TEMPERATURE_MIN: f32 = i16::MIN as f32 * 0.01;

/// An error decoding a property from a sensor.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum DecodeError {
    /// The value being decoded wasn't the expected length.
    #[error("Wrong length {length}, expected {expected_length}")]
    WrongLength {
        length: usize,
        expected_length: usize,
    },
    /// The value being decoded was invalid in some other way.
    #[error("{0}")]
    InvalidValue(String),
}

/// An error encoding a property to be sent to a sensor.
#[derive(Clone, Debug, Error)]
pub enum EncodeError {
    /// The temperature value given is out of the range which can be encoded.
    #[error("Temperature {0} out of range.")]
    TemperatureOutOfRange(f32),
    /// The time value given is out of the range which can be encoded.
    #[error("Time {0:?} out of range.")]
    TimeOutOfRange(SystemTime),
}

fn decode_temperature(bytes: [u8; 2]) -> f32 {
    i16::from_le_bytes(bytes) as f32 * 0.01
}

fn encode_temperature(temperature: f32) -> Result<[u8; 2], EncodeError> {
    if temperature < TEMPERATURE_MIN || temperature > TEMPERATURE_MAX {
        return Err(EncodeError::TemperatureOutOfRange(temperature));
    }
    let temperature_fixed = (temperature * 100.0) as i16;
    Ok(temperature_fixed.to_le_bytes())
}

fn check_length(length: usize, expected_length: usize) -> Result<(), DecodeError> {
    if length != expected_length {
        Err(DecodeError::WrongLength {
            length,
            expected_length,
        })
    } else {
        Ok(())
    }
}
