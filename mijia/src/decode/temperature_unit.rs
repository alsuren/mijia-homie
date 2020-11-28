use eyre::bail;
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
    pub(crate) fn decode(value: &[u8]) -> Result<TemperatureUnit, eyre::Report> {
        if value.len() != 1 {
            bail!("Wrong length {} for temperature unit", value.len());
        }

        match value[0] {
            0x00 => Ok(TemperatureUnit::Celcius),
            0x01 => Ok(TemperatureUnit::Fahrenheit),
            byte => bail!("Invalid temperature unit value 0x{:x}", byte),
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
