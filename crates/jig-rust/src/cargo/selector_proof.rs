use std::{collections::BTreeMap, fmt};

/// Opaque Cargo IDs are retained only to prove the one-to-one mapping used
/// during normalization. Neither serialization nor Debug exposes the IDs.
#[derive(Clone, Eq, PartialEq)]
pub(super) struct SelectorProof(BTreeMap<String, usize>);

impl SelectorProof {
    pub(super) fn new(ids: BTreeMap<String, usize>) -> Self {
        Self(ids)
    }

    pub(super) fn verifies_index(&self, index: usize) -> bool {
        self.0
            .values()
            .filter(|candidate| **candidate == index)
            .count()
            == 1
    }
}

impl fmt::Debug for SelectorProof {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SelectorProof")
            .finish_non_exhaustive()
    }
}
