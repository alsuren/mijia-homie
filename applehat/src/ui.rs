use std::collections::HashMap;

use homie_controller::{Device, HomieController, Property};
use log::error;
use rainbow_hat_rs::alphanum4::Alphanum4;

#[derive(Debug)]
pub struct UiState {
    pub selected_device_id: String,
    pub selected_node_id: String,
    pub selected_property_id: String,
    pub alphanum: Alphanum4,
}

impl UiState {
    /// Updates the display based on the current state.
    pub fn update_display(&mut self, controller: &HomieController) {
        if let Some(property) = get_property(
            &controller.devices(),
            &self.selected_device_id,
            &self.selected_node_id,
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
