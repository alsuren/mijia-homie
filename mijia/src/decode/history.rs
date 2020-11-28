use crate::decode::time::decode_time;
use crate::decode::{check_length, DecodeError};
use std::convert::TryInto;
use std::fmt::{self, Display, Formatter};
use std::ops::Range;
use std::time::SystemTime;

/// Decode a range of indices encoded as a last index and count into a Rust half-open `Range`.
pub(crate) fn decode_range(value: &[u8]) -> Result<Range<u32>, DecodeError> {
    check_length(value.len(), 8)?;

    let last_index = u32::from_le_bytes(value[0..4].try_into().unwrap());
    let count = u32::from_le_bytes(value[4..8].try_into().unwrap());

    let end = last_index + 1;
    let start = end - count;

    Ok(start..end)
}

/// A historical temperature/humidity record stored by a sensor.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    /// The index of the record.
    pub index: u32,
    /// The time at which the record was created.
    pub time: SystemTime,
    /// Minimum temperature in ºC, with 1 decimal place of precision
    pub temperature_min: f32,
    /// Maximum temperature in ºC, with 1 decimal place of precision
    pub temperature_max: f32,
    /// Minimum percent humidity.
    pub humidity_min: u8,
    /// Maximum percent humidity.
    pub humidity_max: u8,
}

impl Record {
    pub(crate) fn decode(value: &[u8]) -> Result<Record, DecodeError> {
        check_length(value.len(), 14)?;

        let index = u32::from_le_bytes(value[0..4].try_into().unwrap());
        let time = decode_time(&value[4..8])?;
        let temperature_max = decode_history_temperature(value[8..10].try_into().unwrap());
        let temperature_min = decode_history_temperature(value[11..13].try_into().unwrap());
        let humidity_max = value[10];
        let humidity_min = value[13];

        Ok(Record {
            index,
            time,
            temperature_min,
            temperature_max,
            humidity_min,
            humidity_max,
        })
    }
}

impl Display for Record {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "{}/{:?} Temperature: {:.2}–{:.2}ºC Humidity: {:?}–{:?}%",
            self.index,
            self.time,
            self.temperature_min,
            self.temperature_max,
            self.humidity_min,
            self.humidity_max
        )
    }
}

/// Decode a temperature from a history record.
///
/// For some reason this is stored with 1 decimal place rather than 2 like other temperature values,
/// so we can't use the common `decode_temperature` function.
fn decode_history_temperature(bytes: [u8; 2]) -> f32 {
    i16::from_le_bytes(bytes) as f32 / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn decode_too_short() {
        assert_eq!(
            Record::decode(&[
                0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            ]),
            Err(DecodeError::WrongLength {
                length: 13,
                expected_length: 14
            })
        );
    }

    #[test]
    fn decode_too_long() {
        assert_eq!(
            Record::decode(&[
                0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
                0x0f,
            ]),
            Err(DecodeError::WrongLength {
                length: 15,
                expected_length: 14
            })
        );
    }

    #[test]
    fn decode_valid() {
        assert_eq!(
            Record::decode(&[
                0x49, 0x01, 0x00, 0x00, 0x40, 0x0c, 0x55, 0x5e, 0xdd, 0x00, 0x43, 0xd5, 0x00, 0x3c
            ])
            .unwrap(),
            Record {
                index: 329,
                time: SystemTime::UNIX_EPOCH + Duration::from_secs(1582632000),
                temperature_min: 21.3,
                temperature_max: 22.1,
                humidity_min: 60,
                humidity_max: 67,
            }
        );
    }
}
