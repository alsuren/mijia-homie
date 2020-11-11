use eyre::bail;
use std::cmp::max;
use std::convert::TryInto;
use std::fmt::{Display, Formatter};
use std::time::{Duration, SystemTime};

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
        let temperature = i16::from_le_bytes(temperature_array) as f32 * 0.01;
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
