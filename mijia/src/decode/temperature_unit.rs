use crate::decode::{check_length, DecodeError};
use std::fmt::{self, Display, Formatter};

/// The temperature unit which a Mijia sensor uses for its display.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TemperatureUnit {
    /// ºC
    Celcius,
    /// ºF
    Fahrenheit,
}

impl TemperatureUnit {
    pub(crate) fn decode(value: &[u8]) -> Result<TemperatureUnit, DecodeError> {
        check_length(value.len(), 1)?;

        match value[0] {
            0x00 => Ok(TemperatureUnit::Celcius),
            0x01 => Ok(TemperatureUnit::Fahrenheit),
            byte => Err(DecodeError::InvalidValue(format!(
                "Invalid temperature unit value 0x{byte:x}"
            ))),
        }
    }

    pub(crate) fn encode(&self) -> [u8; 1] {
        match self {
            TemperatureUnit::Celcius => [0x00],
            TemperatureUnit::Fahrenheit => [0x01],
        }
    }

    /// Returns the string representing this unit, either `"ºC"` or `"ºF"`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Celcius => "ºC",
            Self::Fahrenheit => "ºF",
        }
    }
}

impl Display for TemperatureUnit {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
