use crate::decode::{check_length, DecodeError};
use std::convert::TryInto;
use std::ops::Range;

/// Decode a range of indices encoded as a last index and count into a Rust half-open `Range`.
pub(crate) fn decode_range(value: &[u8]) -> Result<Range<u32>, DecodeError> {
    check_length(value.len(), 8)?;

    let last_index = u32::from_le_bytes(value[0..4].try_into().unwrap());
    let count = u32::from_le_bytes(value[4..8].try_into().unwrap());

    let end = last_index + 1;
    let start = end - count;

    Ok(start..end)
}
