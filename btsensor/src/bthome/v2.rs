//! Support for the [BTHome](https://bthome.io/) v2 format.

use bluez_async::uuid_from_u16;
use std::fmt::{self, Display, Formatter};
use thiserror::Error;
use uuid::Uuid;

pub const UUID: Uuid = uuid_from_u16(0xfcd2);

const DEVICE_INFO_ENCRYPTED: u8 = 0x01;
const DEVICE_INFO_TRIGGER_BASED: u8 = 0x04;
const DEVICE_INFO_VERSION_MASK: u8 = 0xe0;
const DEVICE_INFO_VERSION_OFFSET: usize = 5;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum DecodeError {
    #[error("Unsupported BTHome version {0}")]
    UnsupportedVersion(u8),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BtHomeV2 {
    pub encrypted: bool,
    pub trigger_based: bool,
}

impl BtHomeV2 {
    pub fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        let device_info = data[0];
        let encrypted = device_info & DEVICE_INFO_ENCRYPTED != 0;
        let trigger_based = device_info & DEVICE_INFO_TRIGGER_BASED != 0;
        let version = (device_info & DEVICE_INFO_VERSION_MASK) >> DEVICE_INFO_VERSION_OFFSET;
        if version != 2 {
            return Err(DecodeError::UnsupportedVersion(version));
        }

        Ok(Self {
            encrypted,
            trigger_based,
        })
    }
}

impl Display for BtHomeV2 {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("(")?;
        if self.encrypted {
            f.write_str("encrypted")?;
        } else {
            f.write_str("unencrypted")?;
        }
        if self.trigger_based {
            f.write_str(", trigger based")?;
        }
        f.write_str(")")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_valid() {
        assert_eq!(
            BtHomeV2::decode(&[0x40, 0x02, 0xc4, 0x09]).unwrap(),
            BtHomeV2 {
                encrypted: false,
                trigger_based: false,
            }
        );
    }
}
