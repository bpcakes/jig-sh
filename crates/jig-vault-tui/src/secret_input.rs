use std::fmt;

use jig_vault::{MAX_SECRET_VALUE_LEN, SecretBytes};

/// A bounded, non-cloneable protected editor backed by preallocated zeroizing
/// storage.
pub(crate) struct SecretInput {
    bytes: SecretBytes,
}

impl SecretInput {
    pub(crate) fn new() -> Self {
        Self {
            bytes: SecretBytes::with_capacity(MAX_SECRET_VALUE_LEN),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.bytes.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub(crate) fn push_char(&mut self, character: char) -> Result<(), InputTooLong> {
        let mut encoded = [0_u8; 4];
        self.bytes
            .extend_from_slice(character.encode_utf8(&mut encoded).as_bytes())
            .map_err(|_| InputTooLong)
    }

    /// Appends one paste atomically. A paste that does not fit is rejected in
    /// full so the operator never saves a silently truncated value.
    pub(crate) fn paste(&mut self, value: &str) -> Result<(), InputTooLong> {
        self.bytes
            .extend_from_slice(value.as_bytes())
            .map_err(|_| InputTooLong)
    }

    pub(crate) fn backspace(&mut self) {
        let len = self.bytes.len();
        if len == 0 {
            return;
        }
        let new_len = std::str::from_utf8(self.bytes.as_slice())
            .ok()
            .and_then(|value| value.char_indices().next_back().map(|(index, _)| index))
            .unwrap_or(len - 1);
        self.bytes.truncate(new_len);
    }

    pub(crate) fn clear(&mut self) {
        self.bytes.clear();
    }

    pub(crate) fn matches(&self, other: &Self) -> bool {
        self.bytes.as_slice() == other.bytes.as_slice()
    }

    pub(crate) fn take(&mut self) -> SecretBytes {
        std::mem::replace(
            &mut self.bytes,
            SecretBytes::with_capacity(MAX_SECRET_VALUE_LEN),
        )
    }

    /// Returns bullets and a byte count only. No source character is copied
    /// into the returned render string.
    pub(crate) fn render_label(&self) -> String {
        if self.is_empty() {
            return "(empty)".to_owned();
        }
        let characters = std::str::from_utf8(self.bytes.as_slice())
            .map(|value| value.chars().count())
            .unwrap_or_else(|_| self.len());
        let shown = characters.min(24);
        let mut label = "•".repeat(shown);
        if characters > shown {
            label.push('…');
        }
        label.push_str(&format!("  ({} bytes)", self.len()));
        label
    }
}

impl Default for SecretInput {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for SecretInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretInput")
            .field("len", &self.len())
            .field("value", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InputTooLong;
