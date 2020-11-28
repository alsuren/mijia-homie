pub mod comfort_level;
pub mod readings;
pub mod temperature_unit;
pub mod time;

use eyre::bail;

const TEMPERATURE_MAX: f32 = i16::MAX as f32 * 0.01;
const TEMPERATURE_MIN: f32 = i16::MIN as f32 * 0.01;

fn decode_temperature(bytes: [u8; 2]) -> f32 {
    i16::from_le_bytes(bytes) as f32 * 0.01
}

fn encode_temperature(temperature: f32) -> Result<[u8; 2], eyre::Report> {
    if temperature < TEMPERATURE_MIN || temperature > TEMPERATURE_MAX {
        bail!("Temperature {} out of range.", temperature);
    }
    let temperature_fixed = (temperature * 100.0) as i16;
    Ok(temperature_fixed.to_le_bytes())
}
