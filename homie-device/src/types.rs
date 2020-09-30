use std::fmt::Debug;

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

impl Into<Vec<u8>> for Datatype {
    fn into(self) -> Vec<u8> {
        match self {
            Self::Integer => "integer",
            Self::Float => "float",
            Self::Boolean => "boolean",
            Self::String => "string",
            Self::Enum => "enum",
            Self::Color => "color",
        }
        .into()
    }
}

/// A [property](https://homieiot.github.io/specification/#properties) of a Homie node.
#[derive(Clone, Debug)]
pub struct Property {
    pub id: String,
    pub name: String,
    pub datatype: Datatype,
    pub settable: bool,
    pub unit: Option<String>,
    pub format: Option<String>,
}

impl Property {
    /// Create a new property with the given attributes.
    ///
    /// # Arguments
    /// * `id`: The topic ID for the property. This must be unique per node, and follow the Homie
    ///   [ID format](https://homieiot.github.io/specification/#topic-ids).
    /// * `name`: The human-readable name of the property.
    /// * `datatype`: The data type of the property.
    /// * `settable`: Whether the property can be set by the Homie controller. This should be true
    ///   for properties like the brightness or power state of a light, and false for things like
    ///   the temperature reading of a sensor.
    /// * `unit`: The unit for the property, if any. This may be one of the
    ///   [recommended units](https://homieiot.github.io/specification/#property-attributes), or
    ///   any other custom unit.
    /// * `format`: The format for the property, if any. This must be specified if the datatype is
    ///   `Enum` or `Color`, and may be specified if the datatype is `Integer` or `Float`.
    pub fn new(
        id: &str,
        name: &str,
        datatype: Datatype,
        settable: bool,
        unit: Option<&str>,
        format: Option<&str>,
    ) -> Property {
        Property {
            id: id.to_owned(),
            name: name.to_owned(),
            datatype,
            settable,
            unit: unit.map(|s| s.to_owned()),
            format: format.map(|s| s.to_owned()),
        }
    }
}

/// A [node](https://homieiot.github.io/specification/#nodes) of a Homie device.
#[derive(Clone, Debug)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub node_type: String,
    pub properties: Vec<Property>,
}

impl Node {
    /// Create a new node with the given attributes.
    ///
    /// # Arguments
    /// * `id`: The topic ID for the node. This must be unique per device, and follow the Homie
    ///   [ID format](https://homieiot.github.io/specification/#topic-ids).
    /// * `name`: The human-readable name of the node.
    /// * `type`: The type of the node. This is an arbitrary string.
    /// * `property`: The properties of the node. There should be at least one.
    pub fn new(id: &str, name: &str, node_type: &str, properties: Vec<Property>) -> Node {
        Node {
            id: id.to_owned(),
            name: name.to_owned(),
            node_type: node_type.to_owned(),
            properties,
        }
    }
}
