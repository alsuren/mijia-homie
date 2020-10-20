use crate::values::{ColorFormat, EnumValue, Value, ValueError};
use std::collections::HashMap;
use std::fmt::{self, Debug, Display, Formatter};
use std::ops::RangeInclusive;
use std::str::FromStr;
use std::time::Duration;
use thiserror::Error;

/// The state of a Homie device according to the Homie
/// [device lifecycle](https://homieiot.github.io/specification/#device-lifecycle).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    /// The state of the device is not yet known to the controller because device discovery is still
    /// underway.
    Unknown,
    /// The device is connected to the MQTT broker but is not yet ready to operate.
    Init,
    /// The device is connected and operational.
    Ready,
    /// The device has cleanly disconnected from the MQTT broker.
    Disconnected,
    /// The device is currently sleeping.
    Sleeping,
    /// The device was uncleanly disconnected from the MQTT broker. This could happen due to a
    /// network issue, power failure or some other unexpected failure.
    Lost,
    /// The device is connected to the MQTT broker but something is wrong and it may require human
    /// intervention.
    Alert,
}

impl State {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Init => "init",
            Self::Ready => "ready",
            Self::Disconnected => "disconnected",
            Self::Sleeping => "sleeping",
            Self::Lost => "lost",
            Self::Alert => "alert",
        }
    }
}

/// An error which can be returned when parsing a `State` from a string, if the string does not
/// match a valid Homie
/// [device lifecycle](https://homieiot.github.io/specification/#device-lifecycle) state.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("Invalid state '{0}'")]
pub struct ParseStateError(String);

impl FromStr for State {
    type Err = ParseStateError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "init" => Ok(Self::Init),
            "ready" => Ok(Self::Ready),
            "disconnected" => Ok(Self::Disconnected),
            "sleeping" => Ok(Self::Sleeping),
            "lost" => Ok(Self::Lost),
            "alert" => Ok(Self::Alert),
            _ => Err(ParseStateError(s.to_owned())),
        }
    }
}

impl Display for State {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The data type of a Homie property.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Datatype {
    Integer,
    Float,
    Boolean,
    String,
    Enum,
    Color,
}

impl Datatype {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Integer => "integer",
            Self::Float => "float",
            Self::Boolean => "boolean",
            Self::String => "string",
            Self::Enum => "enum",
            Self::Color => "color",
        }
    }
}

impl Display for Datatype {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An error which can be returned when parsing a `Datatype` from a string, if the string does not
/// match a valid Homie `$datatype` attribute.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("Invalid datatype '{0}'")]
pub struct ParseDatatypeError(String);

impl FromStr for Datatype {
    type Err = ParseDatatypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "integer" => Ok(Self::Integer),
            "float" => Ok(Self::Float),
            "boolean" => Ok(Self::Boolean),
            "string" => Ok(Self::String),
            "enum" => Ok(Self::Enum),
            "color" => Ok(Self::Color),
            _ => Err(ParseDatatypeError(s.to_owned())),
        }
    }
}

/// A [property](https://homieiot.github.io/specification/#properties) of a Homie node.
///
/// The `id`, `name` and `datatype` are required, but might not be available immediately when the
/// property is first discovered. The other attributes are optional.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Property {
    /// The subtopic ID of the property. This is unique per node, and should follow the Homie
    /// [ID format](https://homieiot.github.io/specification/#topic-ids).
    pub id: String,

    /// The human-readable name of the property. This is a required attribute, but might not be
    /// available as soon as the property is first discovered.
    pub name: Option<String>,

    /// The data type of the property. This is a required attribute, but might not be available as
    /// soon as the property is first discovered.
    pub datatype: Option<Datatype>,

    /// Whether the property can be set by the Homie controller. This should be true for properties
    /// like the brightness or power state of a light, and false for things like the temperature
    /// reading of a sensor. It is false by default.
    pub settable: bool,

    /// Whether the property value is retained by the MQTT broker. This is true by default.
    pub retained: bool,

    /// The unit of the property, if any. This may be one of the
    /// [recommended units](https://homieiot.github.io/specification/#property-attributes), or any
    /// other custom unit.
    pub unit: Option<String>,

    /// The format of the property, if any. This should be specified if the datatype is `Enum` or
    /// `Color`, and may be specified if the datatype is `Integer` or `Float`.
    pub format: Option<String>,

    /// The current value of the property, if known. This may change frequently.
    pub value: Option<String>,
}

impl Property {
    /// Create a new property with the given ID.
    ///
    /// # Arguments
    /// * `id`: The subtopic ID for the property. This must be unique per device, and follow the
    ///   Homie [ID format](https://homieiot.github.io/specification/#topic-ids).
    pub(crate) fn new(id: &str) -> Property {
        Property {
            id: id.to_owned(),
            name: None,
            datatype: None,
            settable: false,
            retained: true,
            unit: None,
            format: None,
            value: None,
        }
    }

    /// Returns whether all the required
    /// [attributes](https://homieiot.github.io/specification/#property-attributes) of the property
    /// are filled in.
    pub fn has_required_attributes(&self) -> bool {
        self.name.is_some() && self.datatype.is_some()
    }

    pub fn value<T: Value>(&self) -> Result<T, ValueError> {
        T::valid_for(self.datatype, &self.format)?;

        match self.value {
            None => Err(ValueError::Unknown),
            Some(ref value) => value.parse().map_err(|_| ValueError::ParseFailed {
                value: value.to_owned(),
                datatype: T::datatype(),
            }),
        }
    }

    /// If the datatype of the property is `Color`, returns the color format.
    pub fn color_format(&self) -> Result<ColorFormat, ValueError> {
        // If the datatype is known and it isn't color, that's an error. If it's not known, maybe
        // parsing the format will succeed, so try anyway.
        if let Some(actual) = self.datatype {
            if actual != Datatype::Color {
                return Err(ValueError::WrongDatatype {
                    expected: Datatype::Color,
                    actual,
                });
            }
        }

        match self.format {
            None => Err(ValueError::Unknown),
            Some(ref format) => format.parse(),
        }
    }

    /// If the datatype of the property is `Enum`, gets the possible values of the enum.
    pub fn enum_values(&self) -> Result<Vec<&str>, ValueError> {
        EnumValue::valid_for(self.datatype, &self.format)?;

        match self.format {
            None => Err(ValueError::Unknown),
            Some(ref format) => {
                if format.is_empty() {
                    Err(ValueError::WrongFormat {
                        format: "".to_owned(),
                    })
                } else {
                    Ok(format.split(',').collect())
                }
            }
        }
    }

    pub fn range<T: Value + Copy>(&self) -> Result<RangeInclusive<T>, ValueError> {
        T::valid_for(self.datatype, &self.format)?;

        match self.format {
            None => Err(ValueError::Unknown),
            Some(ref format) => {
                if let [Ok(start), Ok(end)] = format
                    .splitn(2, ':')
                    .map(|part| part.parse())
                    .collect::<Vec<_>>()
                    .as_slice()
                {
                    Ok(RangeInclusive::new(*start, *end))
                } else {
                    Err(ValueError::WrongFormat {
                        format: format.to_owned(),
                    })
                }
            }
        }
    }
}

/// A [node](https://homieiot.github.io/specification/#nodes) of a Homie device.
///
/// All attributes are required, but might not be available immediately when the node is first
/// discovered.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Node {
    /// The subtopic ID of the node. This is unique per device, and should follow the Homie
    /// [ID format](https://homieiot.github.io/specification/#topic-ids).
    pub id: String,

    /// The human-readable name of the node. This is a required attribute, but might not be
    /// available as soon as the node is first discovered.
    pub name: Option<String>,

    /// The type of the node. This is an arbitrary string. It is a required attribute, but might not
    /// be available as soon as the node is first discovered.
    pub node_type: Option<String>,

    /// The properties of the node, keyed by their IDs. There should be at least one.
    pub properties: HashMap<String, Property>,
}

impl Node {
    /// Create a new node with the given ID.
    ///
    /// # Arguments
    /// * `id`: The subtopic ID for the node. This must be unique per device, and follow the Homie
    ///   [ID format](https://homieiot.github.io/specification/#topic-ids).
    pub(crate) fn new(id: &str) -> Node {
        Node {
            id: id.to_owned(),
            name: None,
            node_type: None,
            properties: HashMap::new(),
        }
    }

    /// Add the given property to the node's set of properties.
    pub(crate) fn add_property(&mut self, property: Property) {
        self.properties.insert(property.id.clone(), property);
    }

    /// Returns whether all the required
    /// [attributes](https://homieiot.github.io/specification/#node-attributes) of the node and its
    /// properties are filled in.
    pub fn has_required_attributes(&self) -> bool {
        self.name.is_some()
            && self.node_type.is_some()
            && !self.properties.is_empty()
            && self
                .properties
                .values()
                .all(|property| property.has_required_attributes())
    }
}

/// A Homie [extension](https://homieiot.github.io/extensions/) supported by a device.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Extension {
    /// The identifier of the extension. This should be a reverse domain name followed by some
    /// suffix.
    pub id: String,
    /// The version of the extension.
    pub version: String,
    /// The versions of the Homie spec which the extension supports.
    pub homie_versions: Vec<String>,
}

/// An error which can be returned when parsing an `Extension` from a string.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("Invalid extension '{0}'")]
pub struct ParseExtensionError(String);

impl FromStr for Extension {
    type Err = ParseExtensionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<_> = s.split(':').collect();
        if let [id, version, homie_versions] = parts.as_slice() {
            if let Some(homie_versions) = homie_versions.strip_prefix("[") {
                if let Some(homie_versions) = homie_versions.strip_suffix("]") {
                    return Ok(Extension {
                        id: (*id).to_owned(),
                        version: (*version).to_owned(),
                        homie_versions: homie_versions.split(';').map(|p| p.to_owned()).collect(),
                    });
                }
            }
        }
        Err(ParseExtensionError(s.to_owned()))
    }
}

/// A Homie [device](https://homieiot.github.io/specification/#devices) which has been discovered.
///
/// The `id`, `homie_version`, `name` and `state` are required, but might not be available
/// immediately when the device is first discovered. The `implementation` is optional.
#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    /// The subtopic ID of the device. This is unique per Homie base topic, and should follow the
    /// Homie [ID format](https://homieiot.github.io/specification/#topic-ids).
    pub id: String,

    /// The version of the Homie convention which the device implements.
    pub homie_version: String,

    /// The human-readable name of the device. This is a required attribute, but might not be
    /// available as soon as the device is first discovered.
    pub name: Option<String>,

    /// The current state of the device according to the Homie
    /// [device lifecycle](https://homieiot.github.io/specification/#device-lifecycle).
    pub state: State,

    /// An identifier for the Homie implementation which the device uses.
    pub implementation: Option<String>,

    /// The nodes of the device, keyed by their IDs.
    pub nodes: HashMap<String, Node>,

    /// The Homie extensions implemented by the device.
    pub extensions: Vec<Extension>,

    /// The IP address of the device on the local network.
    pub local_ip: Option<String>,

    /// The MAC address of the device's network interface.
    pub mac: Option<String>,

    /// The name of the firmware running on the device.
    pub firmware_name: Option<String>,

    /// The version of the firware running on the device.
    pub firmware_version: Option<String>,

    /// The interval at which the device refreshes its stats.
    pub stats_interval: Option<Duration>,

    /// The amount of time since the device booted.
    pub stats_uptime: Option<Duration>,

    /// The device's signal strength in %.
    pub stats_signal: Option<i64>,

    /// The device's CPU temperature in °C.
    pub stats_cputemp: Option<f64>,

    /// The device's CPU load in %, averaged across all CPUs over the last `stats_interval`.
    pub stats_cpuload: Option<i64>,

    /// The device's battery level in %.
    pub stats_battery: Option<i64>,

    /// The device's free heap space in bytes.
    pub stats_freeheap: Option<u64>,

    /// The device's power supply voltage in volts.
    pub stats_supply: Option<f64>,
}

impl Device {
    /// Create a new device with the given ID.
    ///
    /// # Arguments
    /// * `id`: The subtopic ID for the device. This must be unique per Homie base topic, and follow
    ///   the Homie [ID format](https://homieiot.github.io/specification/#topic-ids).
    /// * `homie_version`: The version of the Homie convention which the device implements.
    pub(crate) fn new(id: &str, homie_version: &str) -> Device {
        Device {
            id: id.to_owned(),
            homie_version: homie_version.to_owned(),
            name: None,
            state: State::Unknown,
            implementation: None,
            nodes: HashMap::new(),
            extensions: Vec::default(),
            local_ip: None,
            mac: None,
            firmware_name: None,
            firmware_version: None,
            stats_interval: None,
            stats_uptime: None,
            stats_signal: None,
            stats_cputemp: None,
            stats_cpuload: None,
            stats_battery: None,
            stats_freeheap: None,
            stats_supply: None,
        }
    }

    /// Add the given node to the devices's set of nodes.
    pub(crate) fn add_node(&mut self, node: Node) {
        self.nodes.insert(node.id.clone(), node);
    }

    /// Returns whether all the required
    /// [attributes](https://homieiot.github.io/specification/#device-attributes) of the device and
    /// all its nodes and properties are filled in.
    pub fn has_required_attributes(&self) -> bool {
        self.name.is_some()
            && self.state != State::Unknown
            && self
                .nodes
                .values()
                .all(|node| node.has_required_attributes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::values::{ColorHSV, ColorRGB, EnumValue};

    #[test]
    fn extension_parse_succeeds() {
        let legacy_stats: Extension = "org.homie.legacy-stats:0.1.1:[4.x]".parse().unwrap();
        assert_eq!(legacy_stats.id, "org.homie.legacy-stats");
        assert_eq!(legacy_stats.version, "0.1.1");
        assert_eq!(legacy_stats.homie_versions, &["4.x"]);

        let meta: Extension = "eu.epnw.meta:1.1.0:[3.0.1;4.x]".parse().unwrap();
        assert_eq!(meta.id, "eu.epnw.meta");
        assert_eq!(meta.version, "1.1.0");
        assert_eq!(meta.homie_versions, &["3.0.1", "4.x"]);

        let minimal: Extension = "a:0:[]".parse().unwrap();
        assert_eq!(minimal.id, "a");
        assert_eq!(minimal.version, "0");
        assert_eq!(minimal.homie_versions, &[""]);
    }

    #[test]
    fn extension_parse_fails() {
        assert_eq!(
            "".parse::<Extension>(),
            Err(ParseExtensionError("".to_owned()))
        );
        assert_eq!(
            "test.blah:1.2.3".parse::<Extension>(),
            Err(ParseExtensionError("test.blah:1.2.3".to_owned()))
        );
        assert_eq!(
            "test.blah:1.2.3:4.x".parse::<Extension>(),
            Err(ParseExtensionError("test.blah:1.2.3:4.x".to_owned()))
        );
    }

    #[test]
    fn property_integer_parse() {
        let mut property = Property::new("property_id");

        // With no known value, parsing fails.
        assert_eq!(property.value::<i64>(), Err(ValueError::Unknown));

        // With an invalid value, parsing also fails.
        property.value = Some("-".to_owned());
        assert_eq!(
            property.value::<i64>(),
            Err(ValueError::ParseFailed {
                value: "-".to_owned(),
                datatype: Datatype::Integer,
            })
        );

        // With a valid value but unknown datatype, parsing succeeds.
        property.value = Some("42".to_owned());
        assert_eq!(property.value(), Ok(42));

        // With the correct datatype, parsing still succeeds.
        property.datatype = Some(Datatype::Integer);
        assert_eq!(property.value(), Ok(42));

        // Negative values can be parsed.
        property.value = Some("-66".to_owned());
        assert_eq!(property.value(), Ok(-66));

        // With the wrong datatype, parsing fails.
        property.datatype = Some(Datatype::Float);
        assert_eq!(
            property.value::<i64>(),
            Err(ValueError::WrongDatatype {
                actual: Datatype::Float,
                expected: Datatype::Integer,
            })
        );
    }

    #[test]
    fn property_float_parse() {
        let mut property = Property::new("property_id");

        // With no known value, parsing fails.
        assert_eq!(property.value::<f64>(), Err(ValueError::Unknown));

        // With an invalid value, parsing also fails.
        property.value = Some("-".to_owned());
        assert_eq!(
            property.value::<f64>(),
            Err(ValueError::ParseFailed {
                value: "-".to_owned(),
                datatype: Datatype::Float,
            })
        );

        // With a valid value but unknown datatype, parsing succeeds.
        property.value = Some("42.36".to_owned());
        assert_eq!(property.value(), Ok(42.36));

        // With the correct datatype, parsing still succeeds.
        property.datatype = Some(Datatype::Float);
        assert_eq!(property.value(), Ok(42.36));

        // With the wrong datatype, parsing fails.
        property.datatype = Some(Datatype::Integer);
        assert_eq!(
            property.value::<f64>(),
            Err(ValueError::WrongDatatype {
                actual: Datatype::Integer,
                expected: Datatype::Float,
            })
        );
    }

    #[test]
    fn property_color_parse() {
        let mut property = Property::new("property_id");

        // With no known value, parsing fails.
        assert_eq!(property.value::<ColorRGB>(), Err(ValueError::Unknown));
        assert_eq!(property.value::<ColorHSV>(), Err(ValueError::Unknown));

        // With an invalid value, parsing also fails.
        property.value = Some("".to_owned());
        assert_eq!(
            property.value::<ColorRGB>(),
            Err(ValueError::ParseFailed {
                value: "".to_owned(),
                datatype: Datatype::Color,
            })
        );

        // With a valid value but unknown datatype, parsing succeeds as either kind of colour.
        property.value = Some("12,34,56".to_owned());
        assert_eq!(
            property.value(),
            Ok(ColorRGB {
                r: 12,
                g: 34,
                b: 56
            })
        );
        assert_eq!(
            property.value(),
            Ok(ColorHSV {
                h: 12,
                s: 34,
                v: 56
            })
        );

        // With the correct datatype and no format, parsing succeeds as either kind of colour.
        property.datatype = Some(Datatype::Color);
        assert_eq!(
            property.value(),
            Ok(ColorRGB {
                r: 12,
                g: 34,
                b: 56
            })
        );
        assert_eq!(
            property.value(),
            Ok(ColorHSV {
                h: 12,
                s: 34,
                v: 56
            })
        );

        // With a format set, parsing succeeds only as the correct kind of colour.
        property.format = Some("rgb".to_owned());
        assert_eq!(
            property.value(),
            Ok(ColorRGB {
                r: 12,
                g: 34,
                b: 56
            })
        );
        assert_eq!(
            property.value::<ColorHSV>(),
            Err(ValueError::WrongFormat {
                format: "rgb".to_owned()
            })
        );

        // With the wrong datatype, parsing fails.
        property.datatype = Some(Datatype::Integer);
        assert_eq!(
            property.value::<ColorRGB>(),
            Err(ValueError::WrongDatatype {
                actual: Datatype::Integer,
                expected: Datatype::Color,
            })
        );
        assert_eq!(
            property.value::<ColorHSV>(),
            Err(ValueError::WrongDatatype {
                actual: Datatype::Integer,
                expected: Datatype::Color,
            })
        );
    }

    #[test]
    fn property_enum_parse() {
        let mut property = Property::new("property_id");

        // With no known value, parsing fails.
        assert_eq!(property.value::<EnumValue>(), Err(ValueError::Unknown));

        // With an invalid value, parsing also fails.
        property.value = Some("".to_owned());
        assert_eq!(
            property.value::<EnumValue>(),
            Err(ValueError::ParseFailed {
                value: "".to_owned(),
                datatype: Datatype::Enum,
            })
        );

        // With a valid value but unknown datatype, parsing succeeds.
        property.value = Some("anything".to_owned());
        assert_eq!(property.value(), Ok(EnumValue::new("anything")));

        // With the correct datatype, parsing still succeeds.
        property.datatype = Some(Datatype::Enum);
        assert_eq!(property.value(), Ok(EnumValue::new("anything")));

        // With the wrong datatype, parsing fails.
        property.datatype = Some(Datatype::String);
        assert_eq!(
            property.value::<EnumValue>(),
            Err(ValueError::WrongDatatype {
                actual: Datatype::String,
                expected: Datatype::Enum,
            })
        );
    }

    #[test]
    fn property_color_format() {
        let mut property = Property::new("property_id");

        // With no known format or datatype, format parsing fails.
        assert_eq!(property.color_format(), Err(ValueError::Unknown));

        // Parsing an invalid format fails.
        property.format = Some("".to_owned());
        assert_eq!(
            property.color_format(),
            Err(ValueError::WrongFormat {
                format: "".to_owned()
            })
        );

        // Parsing valid formats works even if datatype is unnkown.
        property.format = Some("rgb".to_owned());
        assert_eq!(property.color_format(), Ok(ColorFormat::RGB));
        property.format = Some("hsv".to_owned());
        assert_eq!(property.color_format(), Ok(ColorFormat::HSV));

        // With the wrong datatype, parsing fails.
        property.datatype = Some(Datatype::Integer);
        assert_eq!(
            property.color_format(),
            Err(ValueError::WrongDatatype {
                actual: Datatype::Integer,
                expected: Datatype::Color
            })
        );

        // With the correct datatype, parsing works.
        property.datatype = Some(Datatype::Color);
        assert_eq!(property.color_format(), Ok(ColorFormat::HSV));
    }

    #[test]
    fn property_enum_format() {
        let mut property = Property::new("property_id");

        // With no known format or datatype, format parsing fails.
        assert_eq!(property.enum_values(), Err(ValueError::Unknown));

        // An empty format string is invalid.
        property.format = Some("".to_owned());
        assert_eq!(
            property.enum_values(),
            Err(ValueError::WrongFormat {
                format: "".to_owned()
            })
        );

        // A single value is valid.
        property.format = Some("one".to_owned());
        assert_eq!(property.enum_values(), Ok(vec!["one"]));

        // Several values are parsed correctly.
        property.format = Some("one,two,three".to_owned());
        assert_eq!(property.enum_values(), Ok(vec!["one", "two", "three"]));

        // With the correct datatype, parsing works.
        property.datatype = Some(Datatype::Enum);
        assert_eq!(property.enum_values(), Ok(vec!["one", "two", "three"]));

        // With the wrong datatype, parsing fails.
        property.datatype = Some(Datatype::Color);
        assert_eq!(
            property.enum_values(),
            Err(ValueError::WrongDatatype {
                actual: Datatype::Color,
                expected: Datatype::Enum
            })
        );
    }

    #[test]
    fn property_numeric_format() {
        let mut property = Property::new("property_id");

        // With no known format or datatype, format parsing fails.
        assert_eq!(property.range::<i64>(), Err(ValueError::Unknown));
        assert_eq!(property.range::<f64>(), Err(ValueError::Unknown));

        // An empty format string is invalid.
        property.format = Some("".to_owned());
        assert_eq!(
            property.range::<i64>(),
            Err(ValueError::WrongFormat {
                format: "".to_owned()
            })
        );
        assert_eq!(
            property.range::<f64>(),
            Err(ValueError::WrongFormat {
                format: "".to_owned()
            })
        );

        // A valid range is parsed correctly.
        property.format = Some("1:10".to_owned());
        assert_eq!(property.range(), Ok(1..=10));
        assert_eq!(property.range(), Ok(1.0..=10.0));

        // A range with a decimal point must be a float.
        property.format = Some("3.6:4.2".to_owned());
        assert_eq!(property.range(), Ok(3.6..=4.2));
        assert_eq!(
            property.range::<i64>(),
            Err(ValueError::WrongFormat {
                format: "3.6:4.2".to_owned()
            })
        );

        // With the correct datatype, parsing works.
        property.datatype = Some(Datatype::Integer);
        property.format = Some("1:10".to_owned());
        assert_eq!(property.range(), Ok(1..=10));

        // For the wrong datatype, parsing fails.
        assert_eq!(
            property.range::<f64>(),
            Err(ValueError::WrongDatatype {
                actual: Datatype::Integer,
                expected: Datatype::Float
            })
        );
    }

    #[test]
    fn property_has_required_attributes() {
        let mut property = Property::new("property_id");
        assert_eq!(property.has_required_attributes(), false);

        property.name = Some("Property name".to_owned());
        assert_eq!(property.has_required_attributes(), false);

        property.datatype = Some(Datatype::Integer);
        assert_eq!(property.has_required_attributes(), true);
    }

    /// Construct a minimal `Property` with all the required attributes.
    fn property_with_required_attributes() -> Property {
        let mut property = Property::new("property_id");
        property.name = Some("Property name".to_owned());
        property.datatype = Some(Datatype::Integer);
        property
    }

    #[test]
    fn node_has_required_attributes() {
        let mut node = Node::new("node_id");
        assert_eq!(node.has_required_attributes(), false);

        node.name = Some("Node name".to_owned());
        assert_eq!(node.has_required_attributes(), false);

        node.node_type = Some("Node type".to_owned());
        assert_eq!(node.has_required_attributes(), false);

        node.add_property(property_with_required_attributes());
        assert_eq!(node.has_required_attributes(), true);

        node.add_property(Property::new("property_without_required_attributes"));
        assert_eq!(node.has_required_attributes(), false);
    }

    /// Construct a minimal `Node` with all the required attributes.
    fn node_with_required_attributes() -> Node {
        let mut node = Node::new("node_id");
        node.name = Some("Node name".to_owned());
        node.node_type = Some("Node type".to_owned());
        node.add_property(property_with_required_attributes());
        node
    }

    #[test]
    fn device_has_required_attributes() {
        let mut device = Device::new("device_id", "123");
        assert_eq!(device.has_required_attributes(), false);

        device.name = Some("Device name".to_owned());
        assert_eq!(device.has_required_attributes(), false);

        device.state = State::Init;
        assert_eq!(device.has_required_attributes(), true);

        device.add_node(node_with_required_attributes());
        assert_eq!(device.has_required_attributes(), true);

        device.add_node(Node::new("node_without_required_attributes"));
        assert_eq!(device.has_required_attributes(), false);
    }
}
