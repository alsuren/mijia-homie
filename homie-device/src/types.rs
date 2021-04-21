use std::fmt::{self, Debug, Display, Formatter};
use std::ops::Range;

use crate::values::ColorFormat;

/// The data type for a Homie property.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Datatype {
    /// A [64-bit signed integer](https://homieiot.github.io/specification/#integer).
    Integer,
    /// A [64-bit floating-point number](https://homieiot.github.io/specification/#float).
    Float,
    /// A [boolean value](https://homieiot.github.io/specification/#boolean).
    Boolean,
    /// A [UTF-8 encoded string](https://homieiot.github.io/specification/#string).
    String,
    /// An [enum value](https://homieiot.github.io/specification/#enum) from a set of possible
    /// values specified by the property format.
    Enum,
    /// An RGB or HSV [color](https://homieiot.github.io/specification/#color), depending on the
    /// property format.
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

impl Into<Vec<u8>> for Datatype {
    fn into(self) -> Vec<u8> {
        self.as_str().into()
    }
}

/// A [property](https://homieiot.github.io/specification/#properties) of a Homie node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Property {
    /// The subtopic ID of the property. This must be unique per node, and should follow the Homie
    /// [ID format](https://homieiot.github.io/specification/#topic-ids).
    pub id: String,

    /// The human-readable name of the property.
    pub name: String,

    /// The data type of the property.
    pub datatype: Datatype,

    /// Whether the property can be set by the Homie controller. This should be true for properties
    /// like the brightness or power state of a light, and false for things like the temperature
    /// reading of a sensor.
    pub settable: bool,

    /// Whether the property value is persisted by the MQTT broker. A non-retained property can be
    /// used for a momentary event, like a doorbell being pressed.
    pub retained: bool,

    /// The unit of the property, if any. This may be one of the
    /// [recommended units](https://homieiot.github.io/specification/#property-attributes), or any
    /// other custom unit.
    pub unit: Option<String>,

    /// The format of the property, if any. This must be specified if the datatype is `Enum` or
    /// `Color`, and may be specified if the datatype is `Integer` or `Float`.
    pub format: Option<String>,
}

impl Property {
    /// Create a new property with the given attributes.
    ///
    /// This constructor allows you to create a property with any datatype, but doesn't check that
    /// the `format` is valid. If possible, use the datatype-specific constructors instead.
    ///
    /// # Arguments
    /// * `id`: The subtopic ID for the property. This must be unique per node, and follow the Homie
    ///   [ID format](https://homieiot.github.io/specification/#topic-ids).
    /// * `name`: The human-readable name of the property.
    /// * `datatype`: The data type of the property.
    /// * `settable`: Whether the property can be set by the Homie controller. This should be true
    ///   for properties like the brightness or power state of a light, and false for things like
    ///   the temperature reading of a sensor.
    /// * `retained`: Whether the property value is persisted by the MQTT broker. A non-retained
    ///   property can be used for a momentary event, like a doorbell being pressed.
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
        retained: bool,
        unit: Option<&str>,
        format: Option<&str>,
    ) -> Property {
        Property::make(
            id,
            name,
            datatype,
            settable,
            retained,
            unit,
            format.map(|s| s.to_owned()),
        )
    }

    /// Create a new integer property with the given attributes.
    ///
    /// # Arguments
    /// * `id`: The subtopic ID for the property. This must be unique per node, and follow the Homie
    ///   [ID format](https://homieiot.github.io/specification/#topic-ids).
    /// * `name`: The human-readable name of the property.
    /// * `settable`: Whether the property can be set by the Homie controller. This should be true
    ///   for properties like the brightness or power state of a light, and false for things like
    ///   the temperature reading of a sensor.
    /// * `retained`: Whether the property value is persisted by the MQTT broker. A non-retained
    ///   property can be used for a momentary event, like a doorbell being pressed.
    /// * `unit`: The unit for the property, if any. This may be one of the
    ///   [recommended units](https://homieiot.github.io/specification/#property-attributes), or
    ///   any other custom unit.
    /// * `format`: The valid range for the property, if any.
    pub fn integer(
        id: &str,
        name: &str,
        settable: bool,
        retained: bool,
        unit: Option<&str>,
        format: Option<Range<i64>>,
    ) -> Property {
        let format = format.map(|f| format!("{}:{}", f.start, f.end));
        Property::make(
            id,
            name,
            Datatype::Integer,
            settable,
            retained,
            unit,
            format,
        )
    }

    /// Create a new floating-point property with the given attributes.
    ///
    /// # Arguments
    /// * `id`: The subtopic ID for the property. This must be unique per node, and follow the Homie
    ///   [ID format](https://homieiot.github.io/specification/#topic-ids).
    /// * `name`: The human-readable name of the property.
    /// * `settable`: Whether the property can be set by the Homie controller. This should be true
    ///   for properties like the brightness or power state of a light, and false for things like
    ///   the temperature reading of a sensor.
    /// * `retained`: Whether the property value is persisted by the MQTT broker. A non-retained
    ///   property can be used for a momentary event, like a doorbell being pressed.
    /// * `unit`: The unit for the property, if any. This may be one of the
    ///   [recommended units](https://homieiot.github.io/specification/#property-attributes), or
    ///   any other custom unit.
    /// * `format`: The valid range for the property, if any.
    pub fn float(
        id: &str,
        name: &str,
        settable: bool,
        retained: bool,
        unit: Option<&str>,
        format: Option<Range<f64>>,
    ) -> Property {
        let format = format.map(|f| format!("{}:{}", f.start, f.end));
        Property::make(id, name, Datatype::Float, settable, retained, unit, format)
    }

    /// Create a new boolean property with the given attributes.
    ///
    /// # Arguments
    /// * `id`: The subtopic ID for the property. This must be unique per node, and follow the Homie
    ///   [ID format](https://homieiot.github.io/specification/#topic-ids).
    /// * `name`: The human-readable name of the property.
    /// * `settable`: Whether the property can be set by the Homie controller. This should be true
    ///   for properties like the brightness or power state of a light, and false for things like
    ///   the temperature reading of a sensor.
    /// * `retained`: Whether the property value is persisted by the MQTT broker. A non-retained
    ///   property can be used for a momentary event, like a doorbell being pressed.
    /// * `unit`: The unit for the property, if any. This may be one of the
    ///   [recommended units](https://homieiot.github.io/specification/#property-attributes), or
    ///   any other custom unit.
    pub fn boolean(
        id: &str,
        name: &str,
        settable: bool,
        retained: bool,
        unit: Option<&str>,
    ) -> Property {
        Property::make(
            id,
            name,
            Datatype::Boolean,
            settable,
            retained,
            unit,
            None::<String>,
        )
    }

    /// Create a new string property with the given attributes.
    ///
    /// # Arguments
    /// * `id`: The subtopic ID for the property. This must be unique per node, and follow the Homie
    ///   [ID format](https://homieiot.github.io/specification/#topic-ids).
    /// * `name`: The human-readable name of the property.
    /// * `settable`: Whether the property can be set by the Homie controller. This should be true
    ///   for properties like the brightness or power state of a light, and false for things like
    ///   the temperature reading of a sensor.
    /// * `retained`: Whether the property value is persisted by the MQTT broker. A non-retained
    ///   property can be used for a momentary event, like a doorbell being pressed.
    /// * `unit`: The unit for the property, if any. This may be one of the
    ///   [recommended units](https://homieiot.github.io/specification/#property-attributes), or
    ///   any other custom unit.
    pub fn string(
        id: &str,
        name: &str,
        settable: bool,
        retained: bool,
        unit: Option<&str>,
    ) -> Property {
        Property::make(
            id,
            name,
            Datatype::String,
            settable,
            retained,
            unit,
            None::<String>,
        )
    }

    /// Create a new enum property with the given attributes.
    ///
    /// # Arguments
    /// * `id`: The subtopic ID for the property. This must be unique per node, and follow the Homie
    ///   [ID format](https://homieiot.github.io/specification/#topic-ids).
    /// * `name`: The human-readable name of the property.
    /// * `settable`: Whether the property can be set by the Homie controller. This should be true
    ///   for properties like the brightness or power state of a light, and false for things like
    ///   the temperature reading of a sensor.
    /// * `retained`: Whether the property value is persisted by the MQTT broker. A non-retained
    ///   property can be used for a momentary event, like a doorbell being pressed.
    /// * `unit`: The unit for the property, if any. This may be one of the
    ///   [recommended units](https://homieiot.github.io/specification/#property-attributes), or
    ///   any other custom unit.
    /// * `format`: The possible values for the enum.
    pub fn enumeration(
        id: &str,
        name: &str,
        settable: bool,
        retained: bool,
        unit: Option<&str>,
        format: &[&str],
    ) -> Property {
        Property::make(
            id,
            name,
            Datatype::Enum,
            settable,
            retained,
            unit,
            Some(format.join(",")),
        )
    }

    /// Create a new color property with the given attributes.
    ///
    /// # Arguments
    /// * `id`: The subtopic ID for the property. This must be unique per node, and follow the Homie
    ///   [ID format](https://homieiot.github.io/specification/#topic-ids).
    /// * `name`: The human-readable name of the property.
    /// * `settable`: Whether the property can be set by the Homie controller. This should be true
    ///   for properties like the brightness or power state of a light, and false for things like
    ///   the temperature reading of a sensor.
    /// * `retained`: Whether the property value is persisted by the MQTT broker. A non-retained
    ///   property can be used for a momentary event, like a doorbell being pressed.
    /// * `unit`: The unit for the property, if any. This may be one of the
    ///   [recommended units](https://homieiot.github.io/specification/#property-attributes), or
    ///   any other custom unit.
    /// * `format`: The color format used for the property.
    pub fn color(
        id: &str,
        name: &str,
        settable: bool,
        retained: bool,
        unit: Option<&str>,
        format: ColorFormat,
    ) -> Property {
        Property::make(
            id,
            name,
            Datatype::Color,
            settable,
            retained,
            unit,
            Some(format.to_string()),
        )
    }

    pub fn make(
        id: &str,
        name: &str,
        datatype: Datatype,
        settable: bool,
        retained: bool,
        unit: Option<&str>,
        format: Option<String>,
    ) -> Property {
        Property {
            id: id.to_owned(),
            name: name.to_owned(),
            datatype,
            settable,
            retained,
            unit: unit.map(|s| s.to_owned()),
            format,
        }
    }
}

/// A [node](https://homieiot.github.io/specification/#nodes) of a Homie device.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Node {
    /// The subtopic ID of the node. This must be unique per device, and should follow the Homie
    /// [ID format](https://homieiot.github.io/specification/#topic-ids).
    pub id: String,

    /// The human-readable name of the node.
    pub name: String,

    /// The type of the node. This is an arbitrary string.
    pub node_type: String,

    /// The properties of the node. There should be at least one.
    pub properties: Vec<Property>,
}

impl Node {
    /// Create a new node with the given attributes.
    ///
    /// # Arguments
    /// * `id`: The subtopic ID for the node. This must be unique per device, and follow the Homie
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_property_format() {
        assert_eq!(
            Property::color("id", "name", false, true, None, ColorFormat::RGB).format,
            Some("rgb".to_string())
        );
        assert_eq!(
            Property::color("id", "name", false, true, None, ColorFormat::HSV).format,
            Some("hsv".to_string())
        );
    }

    #[test]
    fn integer_property_format() {
        assert_eq!(
            Property::integer("id", "name", false, true, None, None).format,
            None
        );
        assert_eq!(
            Property::integer("id", "name", false, true, None, Some(-2..5)).format,
            Some("-2:5".to_string())
        );
    }

    #[test]
    fn float_property_format() {
        assert_eq!(
            Property::float("id", "name", false, true, None, None).format,
            None
        );
        assert_eq!(
            Property::float("id", "name", false, true, None, Some(-2.3..5.0)).format,
            Some("-2.3:5".to_string())
        );
    }

    #[test]
    fn enum_property_format() {
        assert_eq!(
            Property::enumeration("id", "name", false, true, None, &["ab", "cd"]).format,
            Some("ab,cd".to_string())
        );
    }
}
