use eyre::bail;
use std::cmp::max;
use std::convert::TryInto;
use std::fmt::{self, Display, Formatter};
use std::time::{Duration, SystemTime};

const TEMPERATURE_MAX: f32 = i16::MAX as f32 * 0.01;
const TEMPERATURE_MIN: f32 = i16::MIN as f32 * 0.01;

/// A set of readings from a Mijia sensor.
#[derive(Clone, Debug, PartialEq)]
pub struct Readings {
    /// Temperature in ºC, with 2 decimal places of precision
    pub temperature: f32,
    /// Percent humidity
    pub humidity: u8,
    /// Voltage in millivolts
    pub battery_voltage: u16,
    /// Inferred from `battery_voltage` with a bit of hand-waving.
    pub battery_percent: u16,
}

impl Display for Readings {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(
            f,
            "Temperature: {:.2}ºC Humidity: {:?}% Battery: {:?} mV ({:?}%)",
            self.temperature, self.humidity, self.battery_voltage, self.battery_percent
        )
    }
}

impl Readings {
    /// Decode the readings from the raw bytes of the Bluetooth characteristic value, if they are
    /// valid.
    /// Returns `None` if the value is not valid.
    pub(crate) fn decode(value: &[u8]) -> Option<Readings> {
        if value.len() != 5 {
            return None;
        }

        let mut temperature_array = [0; 2];
        temperature_array.clone_from_slice(&value[..2]);
        let temperature = decode_temperature(temperature_array);
        let humidity = value[2];
        let battery_voltage = u16::from_le_bytes(value[3..5].try_into().unwrap());
        let battery_percent = (max(battery_voltage, 2100) - 2100) / 10;
        Some(Readings {
            temperature,
            humidity,
            battery_voltage,
            battery_percent,
        })
    }
}

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
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(
            f,
            "Temperature: {:.2}–{:.2}ºC Humidity: {:?}–{:?}%",
            self.temperature_min, self.humidity_max, self.humidity_min, self.humidity_max
        )
    }
}

fn decode_temperature(bytes: [u8; 2]) -> f32 {
    i16::from_le_bytes(bytes) as f32 * 0.01
}

fn encode_temperature(temperature: f32) -> Result<[u8; 2], eyre::Report> {
    if temperature < TEMPERATURE_MIN || temperature > TEMPERATURE_MAX {
        bail!("Temperature {} out of range.", temperature);
    }
    let temperature_fixed = (temperature * 100.0) as i16;
    Ok(temperature_fixed.to_le_bytes())
}

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

pub(crate) fn decode_time(value: &[u8]) -> Result<SystemTime, eyre::Report> {
    if value.len() != 4 {
        bail!("Wrong length {} for time", value.len());
    }

    let timestamp = u32::from_le_bytes(value.try_into().unwrap());
    Ok(SystemTime::UNIX_EPOCH + Duration::from_secs(timestamp as u64))
}

pub(crate) fn encode_time(time: SystemTime) -> Result<[u8; 4], eyre::Report> {
    let timestamp = time
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_secs()
        .try_into()?;
    let encoded = u32::to_le_bytes(timestamp);
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_empty() {
        assert_eq!(Readings::decode(&[]), None);
    }

    #[test]
    fn decode_too_short() {
        assert_eq!(Readings::decode(&[1, 2, 3, 4]), None);
    }

    #[test]
    fn decode_too_long() {
        assert_eq!(Readings::decode(&[1, 2, 3, 4, 5, 6]), None);
    }

    #[test]
    fn decode_valid() {
        assert_eq!(
            Readings::decode(&[1, 2, 3, 4, 10]),
            Some(Readings {
                temperature: 5.13,
                humidity: 3,
                battery_voltage: 2564,
                battery_percent: 46
            })
        );
    }

    #[test]
    fn decode_comfort_level_too_short() {
        assert!(ComfortLevel::decode(&[0x04, 0x02, 0x03, 0x01, 0x06]).is_err());
    }

    #[test]
    fn decode_comfort_level_too_long() {
        assert!(ComfortLevel::decode(&[0x04, 0x02, 0x03, 0x01, 0x06, 0x05, 0x07]).is_err());
    }

    #[test]
    fn decode_comfort_level_valid() {
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
    fn encode_decode_comfort_level() {
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

    #[test]
    fn decode_time_valid() {
        assert_eq!(
            decode_time(&[0x01, 0x02, 0x03, 0x04]).unwrap(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(0x04030201)
        );
    }

    #[test]
    fn decode_time_too_short() {
        assert!(decode_time(&[0x01, 0x02, 0x03]).is_err());
    }

    #[test]
    fn decode_time_too_long() {
        assert!(decode_time(&[0x01, 0x02, 0x03, 0x04, 0x05]).is_err());
    }

    #[test]
    fn encode_decode_time() {
        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(12345678);
        assert_eq!(decode_time(&encode_time(time).unwrap()).unwrap(), time);
    }
}
