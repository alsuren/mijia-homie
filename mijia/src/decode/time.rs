use crate::decode::{DecodeError, EncodeError, check_length};
use std::convert::TryInto;
use std::time::{Duration, SystemTime};

pub(crate) fn decode_time(value: &[u8]) -> Result<SystemTime, DecodeError> {
    check_length(value.len(), 4)?;

    let timestamp = u32::from_le_bytes(value.try_into().unwrap());
    Ok(SystemTime::UNIX_EPOCH + Duration::from_secs(timestamp as u64))
}

pub(crate) fn encode_time(time: SystemTime) -> Result<[u8; 4], EncodeError> {
    let timestamp = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| EncodeError::TimeOutOfRange(time))?
        .as_secs()
        .try_into()
        .map_err(|_| EncodeError::TimeOutOfRange(time))?;
    let encoded = u32::to_le_bytes(timestamp);
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_valid() {
        assert_eq!(
            decode_time(&[0x01, 0x02, 0x03, 0x04]).unwrap(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(0x04030201)
        );
    }

    #[test]
    fn decode_too_short() {
        assert_eq!(
            decode_time(&[0x01, 0x02, 0x03]),
            Err(DecodeError::WrongLength {
                length: 3,
                expected_length: 4
            })
        );
    }

    #[test]
    fn decode_too_long() {
        assert_eq!(
            decode_time(&[0x01, 0x02, 0x03, 0x04, 0x05]),
            Err(DecodeError::WrongLength {
                length: 5,
                expected_length: 4
            })
        );
    }

    #[test]
    fn encode_decode() {
        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(12345678);
        assert_eq!(decode_time(&encode_time(time).unwrap()).unwrap(), time);
    }
}
