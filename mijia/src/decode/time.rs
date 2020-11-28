use eyre::bail;
use std::convert::TryInto;
use std::time::{Duration, SystemTime};

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
    fn decode_valid() {
        assert_eq!(
            decode_time(&[0x01, 0x02, 0x03, 0x04]).unwrap(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(0x04030201)
        );
    }

    #[test]
    fn decode_too_short() {
        assert!(decode_time(&[0x01, 0x02, 0x03]).is_err());
    }

    #[test]
    fn decode_too_long() {
        assert!(decode_time(&[0x01, 0x02, 0x03, 0x04, 0x05]).is_err());
    }

    #[test]
    fn encode_decode() {
        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(12345678);
        assert_eq!(decode_time(&encode_time(time).unwrap()).unwrap(), time);
    }
}
