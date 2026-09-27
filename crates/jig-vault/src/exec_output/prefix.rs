use aho_corasick::{Anchored, automaton::Automaton, nfa::noncontiguous::NFA};
use zeroize::Zeroizing;

use crate::{Result, VaultError, VaultErrorKind};

/// Tracks the longest output suffix that could grow into a secret. Standard
/// Aho-Corasick failure transitions retain precisely the longest trie prefix;
/// unlike leftmost matching, they keep searching after a completed match.
pub(super) struct PrefixMatcher {
    automaton: NFA,
    depths: Vec<usize>,
}

impl PrefixMatcher {
    pub(super) fn new(patterns: &[Zeroizing<Vec<u8>>]) -> Result<Self> {
        // Removing the last byte makes the trie contain every *proper* prefix.
        // A complete secret can be emitted as a marker immediately unless it
        // is also a prefix of a longer secret.
        let automaton = NFA::new(patterns.iter().map(|pattern| &pattern[..pattern.len() - 1]))
            .map_err(|_| {
                VaultError::new(
                    VaultErrorKind::Internal,
                    "failed to build bounded vault output prefix matcher",
                )
            })?;
        let start = automaton
            .start_state(Anchored::No)
            .expect("NFA supports unanchored searches");
        let mut depths = vec![0; start.as_usize() + 1];
        for pattern in patterns {
            let mut state = start;
            for (index, &byte) in pattern[..pattern.len() - 1].iter().enumerate() {
                state = automaton.next_state(Anchored::No, state, byte);
                if depths.len() <= state.as_usize() {
                    depths.resize(state.as_usize() + 1, 0);
                }
                depths[state.as_usize()] = index + 1;
            }
        }
        Ok(Self { automaton, depths })
    }

    pub(super) fn suffix_len(&self, input: &[u8]) -> usize {
        let mut state = self
            .automaton
            .start_state(Anchored::No)
            .expect("NFA supports unanchored searches");
        for &byte in input {
            state = self.automaton.next_state(Anchored::No, state, byte);
        }
        self.depths[state.as_usize()]
    }
}
