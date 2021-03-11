use std::fmt::{self, Debug, Display, Formatter};
use std::str::FromStr;
use thiserror::Error;

/// An error parsing a MAC address from a string.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("Invalid MAC address '{0}'")]
pub struct ParseMacAddressError(String);

/// MAC address of a Bluetooth device.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MacAddress(pub(crate) String);

impl Display for MacAddress {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for MacAddress {
    type Err = ParseMacAddressError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let octets: Vec<_> = s.split(':').collect();
        if octets.len() != 6 {
            return Err(ParseMacAddressError(s.to_owned()));
        }
        for octet in octets {
            if octet.len() != 2 {
                return Err(ParseMacAddressError(s.to_owned()));
            }
            if !octet.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(ParseMacAddressError(s.to_owned()));
            }
        }
        Ok(MacAddress(s.to_uppercase()))
    }
}
