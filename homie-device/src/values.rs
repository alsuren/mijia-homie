use std::fmt::{self, Debug, Display, Formatter};
use std::num::ParseIntError;
use std::str::FromStr;
use thiserror::Error;

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

impl Display for ColorFormat {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub trait Color {
    fn format() -> ColorFormat;
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
