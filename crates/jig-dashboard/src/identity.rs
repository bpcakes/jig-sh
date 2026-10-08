use std::{
    cmp::Ordering,
    hash::{Hash, Hasher},
};

/// A raw machine identity paired with independently prepared display text.
///
/// Equality, ordering, and hashing intentionally ignore the display form.
/// Selection and request routing must remain stable even when sanitization
/// makes two hostile identifiers look identical on screen.
#[derive(Clone, Debug)]
pub struct SelectableIdentity {
    raw: String,
    display: String,
}

impl SelectableIdentity {
    #[must_use]
    pub fn new(raw: impl Into<String>, display: impl Into<String>) -> Self {
        Self {
            raw: raw.into(),
            display: display.into(),
        }
    }

    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }

    #[must_use]
    pub fn display(&self) -> &str {
        &self.display
    }
}

impl PartialEq for SelectableIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl Eq for SelectableIdentity {}

impl PartialOrd for SelectableIdentity {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SelectableIdentity {
    fn cmp(&self, other: &Self) -> Ordering {
        self.raw.cmp(&other.raw)
    }
}

impl Hash for SelectableIdentity {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}
