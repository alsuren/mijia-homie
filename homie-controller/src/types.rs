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

/// A [property](https://homieiot.github.io/specification/#properties) of a Homie node.
#[derive(Clone, Debug)]
pub struct Property {
    pub id: String,
    pub name: Option<String>,
    pub datatype: Option<Datatype>,
    pub settable: bool,
    pub unit: Option<String>,
    pub format: Option<String>,
}

impl Property {
    pub(crate) fn new(id: &str) -> Property {
        Property {
            id: id.to_owned(),
            name: None,
            datatype: None,
            settable: false,
            unit: None,
            format: None,
        }
    }
}

/// A [node](https://homieiot.github.io/specification/#nodes) of a Homie device.
#[derive(Clone, Debug)]
pub struct Node {
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
}
