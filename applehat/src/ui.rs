use std::collections::HashMap;

use homie_controller::{Datatype, Device, HomieController, Node, Property, State};
use log::{error, trace};
use rainbow_hat_rs::{alphanum4::Alphanum4, apa102::APA102};

const TEMPERATURE_PROPERTY_ID: &str = "temperature";
const HUMIDITY_PROPERTY_ID: &str = "humidity";
const PIXEL_BRIGHTNESS: f32 = 0.4;

#[derive(Debug)]
pub struct UiState {
    pub alphanum: Alphanum4,
    pub pixels: APA102,
    pub selected_device_id: Option<String>,
    pub selected_node_id: Option<String>,
    pub selected_property_id: String,
}

impl UiState {
    pub fn new(alphanum: Alphanum4, pixels: APA102) -> Self {
        Self {
            alphanum,
            pixels,
            selected_device_id: None,
            selected_node_id: None,
            selected_property_id: "temperature".to_string(),
        }
    }

    /// Updates the display based on the current state.
    pub fn update_display(&mut self, controller: &HomieController) {
        let devices = controller.devices();

        // Show first 7 nodes on RGB LEDs.
        let nodes = find_nodes(&devices);
        for i in 0..7 {
            let (r, g, b) = if let Some((device_id, node_id, node)) = nodes.get(i) {
                let selected = Some(*device_id) == self.selected_device_id.as_deref()
                    && Some(*node_id) == self.selected_node_id.as_deref();
                trace!("Showing node {:?}", node);
                colour_for_node(node, selected)
            } else {
                (0, 0, 0)
            };
            // TODO: Fix set_pixel brightness to work.
            self.pixels.pixels[i] = [r, g, b, (PIXEL_BRIGHTNESS * 31.0) as u8];
            //self.pixels.set_pixel(i, r, g, b, PIXEL_BRIGHTNESS);
        }
        if let Err(e) = self.pixels.show() {
            error!("Error setting RGB LEDs: {}", e);
        }

        if let (Some(selected_device_id), Some(selected_node_id)) =
            (&self.selected_device_id, &self.selected_node_id)
        {
            // Show currently selected value on alphanumeric display.
            if let Some(property) = get_property(
                &devices,
                selected_device_id,
                selected_node_id,
                &self.selected_property_id,
            ) {
                if let Some(value) = &property.value {
                    print_str_decimal(&mut self.alphanum, &value);
                } else {
                    self.alphanum.print_str("????", false);
                }
            } else {
                self.alphanum.print_str("gone", false);
            }
        } else {
            self.alphanum.print_str("    ", false);
        }
        if let Err(e) = self.alphanum.show() {
            error!("Error displaying: {}", e);
        }
    }
}

fn get_property<'a>(
    devices: &'a HashMap<String, Device>,
    device_id: &str,
    node_id: &str,
    property_id: &str,
) -> Option<&'a Property> {
    devices
        .get(device_id)?
        .nodes
        .get(node_id)?
        .properties
        .get(property_id)
}

fn print_str_decimal(alphanum: &mut Alphanum4, s: &str) {
    let padding = 4usize.saturating_sub(if s.contains('.') {
        s.len() - 1
    } else {
        s.len()
    });
    for position in 0..padding {
        alphanum.set_digit(position, ' ', false);
    }

    let mut position = padding;
    for c in s.chars() {
        if c == '.' {
            if position == 0 {
                alphanum.set_digit(position, '0', true);
                position += 1;
            } else {
                alphanum.set_decimal(position - 1, true);
            }
        } else {
            alphanum.set_digit(position, c, false);
            position += 1;
        }
        if position >= 4 {
            break;
        }
    }
}

/// Finds all nodes on active devices with temperature and humidity properties.
fn find_nodes(devices: &HashMap<String, Device>) -> Vec<(&str, &str, &Node)> {
    let mut nodes: Vec<(&str, &str, &Node)> = vec![];
    for (device_id, device) in devices {
        if device.state == State::Ready {
            for (node_id, node) in &device.nodes {
                if let (Some(temperature_node), Some(humidity_node)) = (
                    node.properties.get(TEMPERATURE_PROPERTY_ID),
                    node.properties.get(HUMIDITY_PROPERTY_ID),
                ) {
                    if temperature_node.datatype == Some(Datatype::Float)
                        && humidity_node.datatype == Some(Datatype::Integer)
                    {
                        nodes.push((device_id, node_id, node));
                    }
                }
            }
        }
    }
    nodes
}

/// Given a node with temperature and humidity properties, returns the appropriate RGB colour to
/// display for it.
fn colour_for_node(node: &Node, selected: bool) -> (u8, u8, u8) {
    let temperature: f64 = node
        .properties
        .get(TEMPERATURE_PROPERTY_ID)
        .unwrap()
        .value()
        .unwrap();
    let humidity: i64 = node
        .properties
        .get(HUMIDITY_PROPERTY_ID)
        .unwrap()
        .value()
        .unwrap();

    (
        scale_to_u8(temperature, 0.0, 40.0),
        (humidity * 255 / 100) as u8,
        if selected { 255 } else { 0 },
    )
}

fn scale_to_u8(value: f64, low: f64, high: f64) -> u8 {
    // Casts from floating point to integer types in Rust are saturating.
    (255.0 * (value - low) / (high - low)) as u8
}
