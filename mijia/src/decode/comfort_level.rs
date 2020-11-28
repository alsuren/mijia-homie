use crate::decode::{decode_temperature, encode_temperature};
use eyre::bail;
use std::convert::TryInto;
use std::fmt::{self, Display, Formatter};

/// Configuration which determines when the sensor displays a happy face.
#[derive(Clone, Debug, PartialEq)]
pub struct ComfortLevel {
    /// Minimum comfortable temperature in ºC, with 2 decimal places of precision
    pub temperature_min: f32,
    /// Maximum comfortable temperature in ºC, with 2 decimal places of precision
    pub temperature_max: f32,
    /// Minimum comfortable percent humidity.
    pub humidity_min: u8,
    /// Maximum comfortable percent humidity.
    pub humidity_max: u8,
}

impl ComfortLevel {
    pub(crate) fn decode(value: &[u8]) -> Result<ComfortLevel, eyre::Report> {
        if value.len() != 6 {
            bail!("Wrong length {} for comfort level", value.len());
        }

        let temperature_max = decode_temperature(value[0..2].try_into().unwrap());
        let temperature_min = decode_temperature(value[2..4].try_into().unwrap());
        let humidity_max = value[4];
        let humidity_min = value[5];

        Ok(ComfortLevel {
            temperature_min,
            temperature_max,
            humidity_min,
            humidity_max,
        })
    }

    pub(crate) fn encode(&self) -> Result<[u8; 6], eyre::Report> {
        let mut bytes = [0; 6];
        bytes[0..2].copy_from_slice(&encode_temperature(self.temperature_max)?);
        bytes[2..4].copy_from_slice(&encode_temperature(self.temperature_min)?);
        bytes[4] = self.humidity_max;
        bytes[5] = self.humidity_min;
        Ok(bytes)
    }
}

impl Display for ComfortLevel {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "Temperature: {:.2}–{:.2}ºC Humidity: {:?}–{:?}%",
            self.temperature_min, self.humidity_max, self.humidity_min, self.humidity_max
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_too_short() {
        assert!(ComfortLevel::decode(&[0x04, 0x02, 0x03, 0x01, 0x06]).is_err());
    }

    #[test]
    fn decode_too_long() {
        assert!(ComfortLevel::decode(&[0x04, 0x02, 0x03, 0x01, 0x06, 0x05, 0x07]).is_err());
    }

    #[test]
    fn decode_valid() {
        assert_eq!(
            ComfortLevel::decode(&[0x04, 0x02, 0x03, 0x01, 0x06, 0x05]).unwrap(),
            ComfortLevel {
                temperature_min: 2.59,
                temperature_max: 5.16,
                humidity_min: 5,
                humidity_max: 6,
            }
        );
    }

    #[test]
    fn encode_decode() {
        let comfort_level = ComfortLevel {
            temperature_min: -5.1,
            temperature_max: 99.5,
            humidity_min: 3,
            humidity_max: 42,
        };
        assert_eq!(
            ComfortLevel::decode(&comfort_level.encode().unwrap()).unwrap(),
            comfort_level
        );
    }
}
