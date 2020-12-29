use serde_derive::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Node {
    pub name: Option<String>,
    #[serde(rename = "interface", default)]
    pub interfaces: Vec<Interface>,
    #[serde(rename = "node", default)]
    pub nodes: Vec<Node>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Interface {
    pub name: String,
    #[serde(rename = "method", default)]
    pub methods: Vec<Method>,
    #[serde(rename = "signal", default)]
    pub signals: Vec<Signal>,
    #[serde(rename = "property", default)]
    pub properties: Vec<Property>,
    #[serde(rename = "annotation", default)]
    pub annotations: Vec<Annotation>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Method {
    pub name: String,
    #[serde(rename = "arg", default)]
    pub args: Vec<MethodArg>,
    #[serde(rename = "annotation", default)]
    pub annotations: Vec<Annotation>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Signal {
    pub name: String,
    #[serde(rename = "arg", default)]
    pub args: Vec<SignalArg>,
    #[serde(rename = "annotation", default)]
    pub annotations: Vec<Annotation>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Property {
    pub name: String,
    #[serde(rename = "type")]
    pub dbustype: String,
    pub access: Access,
    #[serde(rename = "annotation", default)]
    pub annotations: Vec<Annotation>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct MethodArg {
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub dbustype: String,
    #[serde(default = "default_method_arg_direction")]
    pub direction: Direction,
    #[serde(rename = "annotation", default)]
    pub annotations: Vec<Annotation>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct SignalArg {
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub dbustype: String,
    #[serde(default = "default_signal_arg_direction")]
    pub direction: Direction,
    #[serde(rename = "annotation", default)]
    pub annotations: Vec<Annotation>,
}

fn default_method_arg_direction() -> Direction {
    Direction::In
}

fn default_signal_arg_direction() -> Direction {
    Direction::Out
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Annotation {
    pub name: String,
    pub value: String,
}

#[derive(Copy, Clone, Debug, Deserialize, PartialEq, Eq)]
pub enum Direction {
    #[serde(rename = "in")]
    In,
    #[serde(rename = "out")]
    Out,
}

#[derive(Copy, Clone, Debug, Deserialize, PartialEq, Eq)]
pub enum Access {
    #[serde(rename = "readwrite")]
    ReadWrite,
    #[serde(rename = "read")]
    Read,
    #[serde(rename = "write")]
    Write,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn introspect_root() {
        let value: Node = serde_xml_rs::from_str(
            r#"<!DOCTYPE node PUBLIC "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN"
            "http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd">
            <node>
                <interface name="org.freedesktop.DBus.Introspectable">
                    <method name="Introspect">
                        <arg name="xml" type="s" direction="out"/>
                    </method>
                </interface>
                <interface name="org.freedesktop.DBus.ObjectManager">
                    <method name="GetManagedObjects">
                        <arg name="objects" type="a{oa{sa{sv}}}" direction="out"/>
                    </method>
                    <signal name="InterfacesAdded">
                        <arg name="object" type="o"/>
                        <arg name="interfaces" type="a{sa{sv}}"/>
                    </signal>
                    <signal name="InterfacesRemoved">
                        <arg name="object" type="o"/>
                        <arg name="interfaces" type="as"/>
                    </signal>
                </interface>
                <node name="org"/>
            </node>"#,
        )
        .unwrap();
        assert_eq!(
            value,
            Node {
                nodes: vec![Node {
                    nodes: vec![],
                    interfaces: vec![],
                    name: Some("org".to_string()),
                }],
                interfaces: vec![
                    Interface {
                        name: "org.freedesktop.DBus.Introspectable".to_string(),
                        methods: vec![Method {
                            name: "Introspect".to_string(),
                            annotations: vec![],
                            args: vec![MethodArg {
                                name: Some("xml".to_string()),
                                dbustype: "s".to_string(),
                                direction: Direction::Out,
                                annotations: vec![],
                            }],
                        }],
                        signals: vec![],
                        properties: vec![],
                        annotations: vec![],
                    },
                    Interface {
                        name: "org.freedesktop.DBus.ObjectManager".to_string(),
                        methods: vec![Method {
                            name: "GetManagedObjects".to_string(),
                            annotations: vec![],
                            args: vec![MethodArg {
                                name: Some("objects".to_string()),
                                dbustype: "a{oa{sa{sv}}}".to_string(),
                                direction: Direction::Out,
                                annotations: vec![],
                            }],
                        }],
                        signals: vec![
                            Signal {
                                name: "InterfacesAdded".to_string(),
                                annotations: vec![],
                                args: vec![
                                    SignalArg {
                                        name: Some("object".to_string()),
                                        dbustype: "o".to_string(),
                                        direction: Direction::Out,
                                        annotations: vec![],
                                    },
                                    SignalArg {
                                        name: Some("interfaces".to_string()),
                                        dbustype: "a{sa{sv}}".to_string(),
                                        direction: Direction::Out,
                                        annotations: vec![],
                                    }
                                ],
                            },
                            Signal {
                                name: "InterfacesRemoved".to_string(),
                                annotations: vec![],
                                args: vec![
                                    SignalArg {
                                        name: Some("object".to_string()),
                                        dbustype: "o".to_string(),
                                        direction: Direction::Out,
                                        annotations: vec![],
                                    },
                                    SignalArg {
                                        name: Some("interfaces".to_string()),
                                        dbustype: "as".to_string(),
                                        direction: Direction::Out,
                                        annotations: vec![],
                                    }
                                ],
                            }
                        ],
                        properties: vec![],
                        annotations: vec![],
                    },
                ],
                name: None,
            }
        )
    }
}
