use std::convert::TryInto;
use std::fmt::{self, Debug, Display, Formatter, LowerHex, UpperHex};
use std::str::FromStr;
use thiserror::Error;

/// An error parsing a MAC address from a string.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("Invalid MAC address '{0}'")]
pub struct ParseMacAddressError(String);

/// MAC address of a Bluetooth device.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MacAddress([u8; 6]);

impl Display for MacAddress {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        UpperHex::fmt(self, f)
    }
}

impl Debug for MacAddress {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        UpperHex::fmt(self, f)
    }
}

impl UpperHex for MacAddress {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}

impl LowerHex for MacAddress {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}

impl FromStr for MacAddress {
    type Err = ParseMacAddressError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(MacAddress(
            s.split(':')
                .map(|octet| {
                    if octet.len() != 2 {
                        Err(ParseMacAddressError(s.to_string()))
                    } else {
                        u8::from_str_radix(octet, 16)
                            .map_err(|_| ParseMacAddressError(s.to_string()))
                    }
                })
                .collect::<Result<Vec<u8>, _>>()?
                .try_into()
                .map_err(|_| ParseMacAddressError(s.to_string()))?,
        ))
    }
}
