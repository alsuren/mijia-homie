use std::collections::HashMap;
use std::fmt::{self, Debug, Display, Formatter};
use std::str::FromStr;
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
#[derive(Error, Debug)]
#[error("Invalid state '{0}'")]
pub struct StateParseError(String);

impl FromStr for State {
    type Err = StateParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "init" => Ok(Self::Init),
            "ready" => Ok(Self::Ready),
            "disconnected" => Ok(Self::Disconnected),
            "sleeping" => Ok(Self::Sleeping),
            "lost" => Ok(Self::Lost),
            "alert" => Ok(Self::Alert),
            _ => Err(StateParseError(s.to_owned())),
        }
    }
}

impl Display for State {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
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

/// An error which can be returned when parsing a `Datatype` from a string, if the string does not
/// match a valid Homie `$datatype` attribute.
#[derive(Error, Debug)]
#[error("Invalid datatype '{0}'")]
pub struct DatatypeParseError(String);

impl FromStr for Datatype {
    type Err = DatatypeParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "integer" => Ok(Self::Integer),
            "float" => Ok(Self::Float),
            "boolean" => Ok(Self::Boolean),
            "string" => Ok(Self::String),
            "enum" => Ok(Self::Enum),
            "color" => Ok(Self::Color),
            _ => Err(DatatypeParseError(s.to_owned())),
        }
    }
}

/// A [property](https://homieiot.github.io/specification/#properties) of a Homie node.
///
/// The `id`, `name` and `datatype` are required, but might not be available immediately when the
/// property is first discovered. The other attributes are optional.
#[derive(Clone, Debug)]
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
}

/// A [node](https://homieiot.github.io/specification/#nodes) of a Homie device.
///
/// All attributes are required, but might not be available immediately when the node is first
/// discovered.
#[derive(Clone, Debug)]
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

/// A Homie [device](https://homieiot.github.io/specification/#devices) which has been discovered.
///
/// The `id`, `homie_version`, `name` and `state` are required, but might not be available
/// immediately when the device is first discovered. The `implementation` is optional.
#[derive(Clone, Debug)]
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
}

impl Device {
    pub(crate) fn new(id: &str, homie_version: &str) -> Device {
        Device {
            id: id.to_owned(),
            homie_version: homie_version.to_owned(),
            name: None,
            state: State::Unknown,
            implementation: None,
            nodes: HashMap::new(),
        }
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
