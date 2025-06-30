use crate::types::Datatype;
use std::fmt::{self, Debug, Display, Formatter};
use std::num::ParseIntError;
use std::str::FromStr;
use thiserror::Error;

/// An error encountered while parsing the value or format of a property.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ValueError {
    /// The value of the property or attribute is not yet known, or not set by the device.
    #[error("Value not yet known.")]
    Unknown,
    /// The method call expected the property to have a particular datatype, but the datatype sent
    /// by the device was something different.
    #[error("Expected value of type {expected} but was {actual}.")]
    WrongDatatype {
        /// The datatype expected by the method call.
        expected: Datatype,
        /// The actual datatype of the property, as sent by the device.
        actual: Datatype,
    },
    /// The format of the property couldn't be parsed or didn't match what was expected by the
    /// method call.
    #[error("Invalid or unexpected format {format}.")]
    WrongFormat {
        /// The format string of the property.
        format: String,
    },
    /// The value of the property couldn't be parsed as the expected type.
    #[error("Parsing {value} as datatype {datatype} failed.")]
    ParseFailed {
        /// The string value of the property.
        value: String,
        /// The datatype as which the value was attempted to be parsed.
        datatype: Datatype,
    },
}

/// The value of a Homie property. This has implementations corresponding to the possible property datatypes.
pub trait Value: ToString + FromStr {
    /// The Homie datatype corresponding to this type.
    fn datatype() -> Datatype;

    /// Check whether this value type is valid for the given property datatype and format string.
    ///
    /// Returns `Ok(())` if so, or `Err(WrongFormat(...))` or `Err(WrongDatatype(...))` if not.
    ///
    /// The default implementation checks the datatype, and delegates to `valid_for_format` to check
    /// the format.
    fn valid_for(datatype: Option<Datatype>, format: &Option<String>) -> Result<(), ValueError> {
        // If the datatype is known and it doesn't match what is being asked for, that's an error.
        // If it's not known, maybe parsing will succeed.
        if let Some(actual) = datatype {
            let expected = Self::datatype();
            if actual != expected {
                return Err(ValueError::WrongDatatype { expected, actual });
            }
        }

        if let Some(ref format) = format {
            Self::valid_for_format(format)
        } else {
            Ok(())
        }
    }

    /// Check whether this value type is valid for the given property format string.
    ///
    /// Returns `Ok(())` if so, or `Err(WrongFormat(...))` if not.
    fn valid_for_format(_format: &str) -> Result<(), ValueError> {
        Ok(())
    }
}

impl Value for i64 {
    fn datatype() -> Datatype {
        Datatype::Integer
    }
}

impl Value for f64 {
    fn datatype() -> Datatype {
        Datatype::Float
    }
}

impl Value for bool {
    fn datatype() -> Datatype {
        Datatype::Boolean
    }
}

// TODO: What about &str?
impl Value for String {
    fn datatype() -> Datatype {
        Datatype::String
    }
}

/// The format of a [colour](https://homieiot.github.io/specification/#color) property, either RGB
/// or HSV.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ColorFormat {
    /// The colour is in red-green-blue format.
    Rgb,
    /// The colour is in hue-saturation-value format.
    Hsv,
}

impl ColorFormat {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Rgb => "rgb",
            Self::Hsv => "hsv",
        }
    }
}

impl FromStr for ColorFormat {
    type Err = ValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "rgb" => Ok(Self::Rgb),
            "hsv" => Ok(Self::Hsv),
            _ => Err(ValueError::WrongFormat {
                format: s.to_owned(),
            }),
        }
    }
}

impl Display for ColorFormat {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub trait Color: Value {
    fn format() -> ColorFormat;
}

impl<T: Color> Value for T {
    fn datatype() -> Datatype {
        Datatype::Color
    }

    fn valid_for_format(format: &str) -> Result<(), ValueError> {
        if format == Self::format().as_str() {
            Ok(())
        } else {
            Err(ValueError::WrongFormat {
                format: format.to_owned(),
            })
        }
    }
}

/// An error while attempting to parse a `Color` from a string.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("Failed to parse color.")]
pub struct ParseColorError();

impl From<ParseIntError> for ParseColorError {
    fn from(_: ParseIntError) -> Self {
        ParseColorError()
    }
}

/// A [colour](https://homieiot.github.io/specification/#color) in red-green-blue format.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ColorRgb {
    /// The red channel of the colour, between 0 and 255.
    pub r: u8,
    /// The green channel of the colour, between 0 and 255.
    pub g: u8,
    /// The blue channel of the colour, between 0 and 255.
    pub b: u8,
}

impl ColorRgb {
    /// Construct a new RGB colour.
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        ColorRgb { r, g, b }
    }
}

impl Display for ColorRgb {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{},{},{}", self.r, self.g, self.b)
    }
}

impl FromStr for ColorRgb {
    type Err = ParseColorError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<_> = s.split(',').collect();
        if let [r, g, b] = parts.as_slice() {
            Ok(ColorRgb {
                r: r.parse()?,
                g: g.parse()?,
                b: b.parse()?,
            })
        } else {
            Err(ParseColorError())
        }
    }
}

impl Color for ColorRgb {
    fn format() -> ColorFormat {
        ColorFormat::Rgb
    }
}

/// A [colour](https://homieiot.github.io/specification/#color) in hue-saturation-value format.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ColorHsv {
    /// The hue of the colour, between 0 and 360.
    pub h: u16,
    /// The saturation of the colour, between 0 and 100.
    pub s: u8,
    /// The value of the colour, between 0 and 100.
    pub v: u8,
}

impl ColorHsv {
    /// Construct a new HSV colour, or panic if the values given are out of range.
    pub fn new(h: u16, s: u8, v: u8) -> Self {
        assert!(h <= 360);
        assert!(s <= 100);
        assert!(v <= 100);
        ColorHsv { h, s, v }
    }
}

impl Display for ColorHsv {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{},{},{}", self.h, self.s, self.v)
    }
}

impl FromStr for ColorHsv {
    type Err = ParseColorError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<_> = s.split(',').collect();
        if let [h, s, v] = parts.as_slice() {
            let h = h.parse()?;
            let s = s.parse()?;
            let v = v.parse()?;
            if h <= 360 && s <= 100 && v <= 100 {
                return Ok(ColorHsv { h, s, v });
            }
        }
        Err(ParseColorError())
    }
}

impl Color for ColorHsv {
    fn format() -> ColorFormat {
        ColorFormat::Hsv
    }
}

/// The value of a Homie [enum](https://homieiot.github.io/specification/#enum) property.
///
/// This must be a non-empty string.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct EnumValue(String);

impl EnumValue {
    pub fn new(s: &str) -> Self {
        assert!(!s.is_empty());
        EnumValue(s.to_owned())
    }
}

/// An error while attempting to parse an `EnumValue` from a string, because the string is empty.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("Empty string is not a valid enum value.")]
pub struct ParseEnumError();

impl FromStr for EnumValue {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            Err(ParseEnumError())
        } else {
            Ok(EnumValue::new(s))
        }
    }
}

impl Display for EnumValue {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Value for EnumValue {
    fn datatype() -> Datatype {
        Datatype::Enum
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_rgb_to_from_string() {
        let color = ColorRgb::new(111, 222, 42);
        assert_eq!(color.to_string().parse(), Ok(color));
    }

    #[test]
    fn color_hsv_to_from_string() {
        let color = ColorHsv::new(231, 88, 77);
        assert_eq!(color.to_string().parse(), Ok(color));
    }

    #[test]
    fn color_rgb_parse_invalid() {
        assert_eq!("".parse::<ColorRgb>(), Err(ParseColorError()));
        assert_eq!("1,2".parse::<ColorRgb>(), Err(ParseColorError()));
        assert_eq!("1,2,3,4".parse::<ColorRgb>(), Err(ParseColorError()));
        assert_eq!("1,2,256".parse::<ColorRgb>(), Err(ParseColorError()));
        assert_eq!("1,256,3".parse::<ColorRgb>(), Err(ParseColorError()));
        assert_eq!("256,2,3".parse::<ColorRgb>(), Err(ParseColorError()));
        assert_eq!("1,-2,3".parse::<ColorRgb>(), Err(ParseColorError()));
    }

    #[test]
    fn color_hsv_parse_invalid() {
        assert_eq!("".parse::<ColorHsv>(), Err(ParseColorError()));
        assert_eq!("1,2".parse::<ColorHsv>(), Err(ParseColorError()));
        assert_eq!("1,2,3,4".parse::<ColorHsv>(), Err(ParseColorError()));
        assert_eq!("1,2,101".parse::<ColorHsv>(), Err(ParseColorError()));
        assert_eq!("1,101,3".parse::<ColorHsv>(), Err(ParseColorError()));
        assert_eq!("361,2,3".parse::<ColorHsv>(), Err(ParseColorError()));
        assert_eq!("1,-2,3".parse::<ColorHsv>(), Err(ParseColorError()));
    }
}
