//! Session identifiers shared by providers that name sessions with UUIDs.

use anyhow::{Result, bail};
use jig_tui::sanitize_text;

/// Validates a hyphenated UUID and returns the lowercase spelling providers store.
pub(crate) fn normalize(input: &str, provider: &str) -> Result<String> {
    let bytes = input.as_bytes();
    let valid = bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        });
    if !valid {
        bail!(
            "Invalid {provider} session ID '{}'; expected a UUID",
            sanitize_text(input)
        )
    }
    Ok(input.to_ascii_lowercase())
}
