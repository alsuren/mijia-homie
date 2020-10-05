use std::collections::HashMap;
use std::fmt::{self, Debug, Display, Formatter};
use std::str::FromStr;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Unknown,
    Init,
    Ready,
    Disconnected,
    Sleeping,
    Lost,
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

/// The data type for a Homie property.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Datatype {
    Integer,
    Float,
    Boolean,
    String,
    Enum,
    Color,
}

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
#[derive(Clone, Debug)]
pub struct Property {
    // Required attributes, but might not be available immediately.
    pub id: String,
    pub name: Option<String>,
    pub datatype: Option<Datatype>,
    // Optional attributes.
    pub settable: bool,
    pub retained: bool,
    pub unit: Option<String>,
    pub format: Option<String>,
    // Value
    pub value: Option<String>,
}

impl Property {
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
#[derive(Clone, Debug)]
pub struct Node {
    // All attributes are required, but might not be available immediately.
    pub id: String,
    pub name: Option<String>,
    pub node_type: Option<String>,
    pub properties: HashMap<String, Property>,
}

impl Node {
    /// Create a new node with the given ID.
    ///
    /// # Arguments
    /// * `id`: The topic ID for the node. This must be unique per device, and follow the Homie
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

#[derive(Clone, Debug)]
pub struct Device {
    pub id: String,
    pub homie_version: String,
    pub name: Option<String>,
    pub state: State,
    pub implementation: Option<String>,
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
