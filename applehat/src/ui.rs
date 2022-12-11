use homie_controller::{Datatype, Device, HomieController, Node, Property, State};
use log::{debug, error, trace};
use rainbow_hat_rs::{alphanum4::Alphanum4, apa102::APA102, touch::Buttons};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    task::{self, JoinHandle},
    time::sleep,
};

const TEMPERATURE_PROPERTY_ID: &str = "temperature";
const HUMIDITY_PROPERTY_ID: &str = "humidity";
const PROPERTY_IDS: [&str; 2] = [TEMPERATURE_PROPERTY_ID, HUMIDITY_PROPERTY_ID];
const PIXEL_BRIGHTNESS: f32 = 0.2;
const BUTTON_POLL_PERIOD: Duration = Duration::from_millis(100);

#[derive(Debug)]
pub struct UiState {
    controller: Arc<HomieController>,
    alphanum: Alphanum4,
    pixels: APA102,
    selected_device_id: Option<String>,
    selected_node_id: Option<String>,
    selected_property_id: String,
    button_state: [bool; 3],
}

impl UiState {
    pub fn new(controller: Arc<HomieController>, alphanum: Alphanum4, pixels: APA102) -> Self {
        Self {
            controller,
            alphanum,
            pixels,
            selected_device_id: None,
            selected_node_id: None,
            selected_property_id: TEMPERATURE_PROPERTY_ID.to_string(),
            button_state: Default::default(),
        }
    }

    /// Updates the display based on the current state.
    pub fn update_display(&mut self) {
        let devices = self.controller.devices();

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

        if self.selected_device_id.is_none() || self.selected_node_id.is_none() {
            if let Some((device_id, node_id, _)) = nodes.get(0) {
                self.selected_device_id = Some(device_id.to_string());
                self.selected_node_id = Some(node_id.to_string());
            }
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
                    print_str_decimal(
                        &mut self.alphanum,
                        &value,
                        if self.selected_property_id == HUMIDITY_PROPERTY_ID {
                            Some('%')
                        } else {
                            None
                        },
                    );
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

    fn button_pressed(&mut self, button_index: usize) {
        debug!("Button {} pressed.", button_index);
        match button_index {
            0 => {
                // Select next node.
                let devices = self.controller.devices();
                let nodes = find_nodes(&devices);
                if !nodes.is_empty() {
                    let new_index = if let Some(selected_node_id) = &self.selected_node_id {
                        if let Some(current_index) = nodes
                            .iter()
                            .position(|(_, node_id, _)| node_id == selected_node_id)
                        {
                            (current_index + 1) % nodes.len()
                        } else {
                            0
                        }
                    } else {
                        0
                    };
                    self.selected_node_id = Some(nodes[new_index].1.to_string());
                }
            }
            1 => {
                // Select next property.
                let current_index = PROPERTY_IDS
                    .iter()
                    .position(|x| x == &self.selected_property_id)
                    .unwrap_or(0);
                self.selected_property_id =
                    PROPERTY_IDS[(current_index + 1) % PROPERTY_IDS.len()].to_string();
            }
            _ => {}
        }
        self.update_display();
    }

    fn update_button_state(&mut self, new_state: [bool; 3]) {
        for i in 0..3 {
            if new_state[i] && !self.button_state[i] {
                self.button_pressed(i);
            }
        }
        self.button_state = new_state;
    }
}

pub fn spawn_button_poll_loop(
    mut buttons: Buttons,
    ui_state: Arc<Mutex<UiState>>,
) -> JoinHandle<()> {
    task::spawn(async move {
        loop {
            let new_state = [
                buttons.a.is_pressed(),
                buttons.b.is_pressed(),
                buttons.c.is_pressed(),
            ];
            ui_state.lock().unwrap().update_button_state(new_state);

            sleep(BUTTON_POLL_PERIOD).await;
        }
    })
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

fn print_str_decimal(alphanum: &mut Alphanum4, s: &str, unit: Option<char>) {
    let number_width = if unit.is_some() { 3usize } else { 4 };

    let padding = number_width.saturating_sub(if s.contains('.') {
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
        if position >= number_width {
            break;
        }
    }

    if let Some(unit) = unit {
        alphanum.set_digit(3, unit, false);
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
        if selected { 128 } else { 0 },
    )
}

fn scale_to_u8(value: f64, low: f64, high: f64) -> u8 {
    // Casts from floating point to integer types in Rust are saturating.
    (255.0 * (value - low) / (high - low)) as u8
}
