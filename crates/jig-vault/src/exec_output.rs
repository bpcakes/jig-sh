use std::fmt;
use std::io::{self, Write};
use std::sync::Arc;

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, AhoCorasickKind, MatchKind};
use zeroize::{Zeroize, Zeroizing};

use crate::redact::MIN_REDACTABLE_LEN;
use crate::{Result, VaultError, VaultErrorKind};

pub(crate) const EXEC_REDACTION_MARKER: &[u8] = b"[REDACTED]";
pub(crate) const MAX_EXEC_REDACTION_PATTERNS: usize = 4_096;
pub(crate) const MAX_EXEC_REDACTION_PATTERN_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_EXEC_REDACTION_PATTERN_LEN: usize = 512 * 1024;
pub(crate) const MAX_EXEC_OUTPUT_CHUNK_LEN: usize = 64 * 1024;

/// One stateful byte stream backed by a shared immutable matcher.
///
/// Each stdout/stderr stream must use an independent state so bytes are never
/// matched across logical streams. Pending overlap is zeroized on every
/// compaction and on drop.
pub(crate) struct StreamingRedactor {
    matcher: Option<Arc<AhoCorasick>>,
    pattern_count: usize,
    max_pattern_len: usize,
    pending: Zeroizing<Vec<u8>>,
}

impl StreamingRedactor {
    pub(crate) fn new(mut patterns: Vec<Zeroizing<Vec<u8>>>) -> Result<Self> {
        validate_patterns(&patterns)?;
        patterns.sort_by(|left, right| left.as_slice().cmp(right.as_slice()));
        patterns.dedup_by(|left, right| left.as_slice() == right.as_slice());

        let pattern_count = patterns.len();
        let max_pattern_len = patterns
            .iter()
            .map(|pattern| pattern.len())
            .max()
            .unwrap_or(0);
        let matcher = if patterns.is_empty() {
            None
        } else {
            Some(Arc::new(
                AhoCorasickBuilder::new()
                    .match_kind(MatchKind::LeftmostLongest)
                    .kind(Some(AhoCorasickKind::ContiguousNFA))
                    .build(patterns.iter().map(|pattern| pattern.as_slice()))
                    .map_err(|_| {
                        VaultError::new(
                            VaultErrorKind::Internal,
                            "failed to build bounded vault output redaction matcher",
                        )
                    })?,
            ))
        };
        let pending_capacity = max_pattern_len
            .saturating_sub(1)
            .checked_add(MAX_EXEC_OUTPUT_CHUNK_LEN)
            .ok_or_else(|| {
                VaultError::new(
                    VaultErrorKind::Internal,
                    "vault output redaction working-memory bound overflowed",
                )
            })?;
        Ok(Self {
            matcher,
            pattern_count,
            max_pattern_len,
            pending: Zeroizing::new(Vec::with_capacity(pending_capacity)),
        })
    }

    /// Creates independent overlap state over the same immutable automaton.
    pub(crate) fn independent_stream(&self) -> Self {
        Self {
            matcher: self.matcher.clone(),
            pattern_count: self.pattern_count,
            max_pattern_len: self.max_pattern_len,
            pending: Zeroizing::new(Vec::with_capacity(self.pending.capacity())),
        }
    }

    pub(crate) fn push_chunk<W: Write + ?Sized>(
        &mut self,
        chunk: &[u8],
        writer: &mut W,
    ) -> io::Result<()> {
        if chunk.len() > MAX_EXEC_OUTPUT_CHUNK_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "vault output chunk exceeds the {MAX_EXEC_OUTPUT_CHUNK_LEN} byte matcher bound"
                ),
            ));
        }
        let Some(matcher) = &self.matcher else {
            return writer.write_all(chunk);
        };
        if chunk.is_empty() {
            return Ok(());
        }

        debug_assert!(
            self.pending.len() + chunk.len() <= self.pending.capacity(),
            "streaming redaction pending allocation exceeded its constructor bound"
        );
        self.pending.extend_from_slice(chunk);
        let safe_start_limit = self
            .pending
            .len()
            .saturating_sub(self.max_pattern_len.saturating_sub(1));
        if safe_start_limit == 0 {
            return Ok(());
        }

        let consumed = emit_matches(matcher, self.pending.as_slice(), safe_start_limit, writer)?;
        discard_prefix(&mut self.pending, consumed);
        Ok(())
    }

    pub(crate) fn finish<W: Write + ?Sized>(mut self, writer: &mut W) -> io::Result<()> {
        match &self.matcher {
            Some(matcher) => {
                let pending_len = self.pending.len();
                let consumed = emit_matches(matcher, self.pending.as_slice(), pending_len, writer)?;
                debug_assert_eq!(consumed, pending_len);
                discard_prefix(&mut self.pending, consumed);
            }
            None => {
                debug_assert!(self.pending.is_empty());
            }
        }
        Ok(())
    }

    #[cfg(test)]
    fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

impl fmt::Debug for StreamingRedactor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StreamingRedactor")
            .field("pattern_count", &self.pattern_count)
            .field("max_pattern_len", &self.max_pattern_len)
            .field("pending_len", &self.pending.len())
            .field("patterns", &"[REDACTED]")
            .field("pending", &"[REDACTED]")
            .finish()
    }
}

fn validate_patterns(patterns: &[Zeroizing<Vec<u8>>]) -> Result<()> {
    if patterns.len() > MAX_EXEC_REDACTION_PATTERNS {
        return Err(invalid_patterns(format!(
            "vault output redaction has more than {MAX_EXEC_REDACTION_PATTERNS} patterns"
        )));
    }
    let mut total = 0_usize;
    for pattern in patterns {
        if pattern.len() < MIN_REDACTABLE_LEN {
            return Err(invalid_patterns(format!(
                "vault output redaction patterns must be at least {MIN_REDACTABLE_LEN} bytes"
            )));
        }
        if pattern.len() > MAX_EXEC_REDACTION_PATTERN_LEN {
            return Err(invalid_patterns(format!(
                "vault output redaction pattern exceeds the {MAX_EXEC_REDACTION_PATTERN_LEN} byte length limit"
            )));
        }
        total = total.checked_add(pattern.len()).ok_or_else(|| {
            invalid_patterns("vault output redaction pattern bytes exceed supported bounds")
        })?;
        if total > MAX_EXEC_REDACTION_PATTERN_BYTES {
            return Err(invalid_patterns(format!(
                "vault output redaction patterns exceed the {MAX_EXEC_REDACTION_PATTERN_BYTES} byte total limit"
            )));
        }
    }
    Ok(())
}

fn invalid_patterns(message: impl Into<String>) -> VaultError {
    VaultError::new(VaultErrorKind::InvalidInput, message)
}

/// Emits every match whose start position is known to be final. The returned
/// prefix length may extend past `safe_start_limit` when such a match crosses
/// the boundary; consuming the complete match is both safe and necessary.
fn emit_matches<W: Write + ?Sized>(
    matcher: &AhoCorasick,
    input: &[u8],
    safe_start_limit: usize,
    writer: &mut W,
) -> io::Result<usize> {
    let mut cursor = 0_usize;
    let mut consumed = safe_start_limit;
    for matched in matcher.find_iter(input) {
        if matched.start() >= safe_start_limit {
            break;
        }
        writer.write_all(&input[cursor..matched.start()])?;
        writer.write_all(EXEC_REDACTION_MARKER)?;
        cursor = matched.end();
        consumed = consumed.max(cursor);
    }
    if cursor < safe_start_limit {
        writer.write_all(&input[cursor..safe_start_limit])?;
    }
    Ok(consumed)
}

fn discard_prefix(buffer: &mut Vec<u8>, consumed: usize) {
    debug_assert!(consumed <= buffer.len());
    if consumed == 0 {
        return;
    }
    let remaining = buffer.len() - consumed;
    buffer.copy_within(consumed.., 0);
    buffer[remaining..].zeroize();
    buffer.truncate(remaining);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patterns(values: &[&[u8]]) -> Vec<Zeroizing<Vec<u8>>> {
        values
            .iter()
            .map(|value| Zeroizing::new(value.to_vec()))
            .collect()
    }

    fn redact_with_chunks(patterns: &[&[u8]], input: &[u8], chunks: &[usize]) -> Vec<u8> {
        let mut redactor = StreamingRedactor::new(self::patterns(patterns)).unwrap();
        let mut output = Vec::new();
        let mut cursor = 0;
        for &chunk_len in chunks {
            let end = (cursor + chunk_len).min(input.len());
            redactor
                .push_chunk(&input[cursor..end], &mut output)
                .unwrap();
            cursor = end;
        }
        if cursor < input.len() {
            redactor.push_chunk(&input[cursor..], &mut output).unwrap();
        }
        redactor.finish(&mut output).unwrap();
        output
    }

    #[test]
    fn redacts_matches_split_at_every_byte_boundary() {
        let input = b"prefix<secret-value>middle<c2VjcmV0LXZhbHVl>suffix";
        let expected = b"prefix<[REDACTED]>middle<[REDACTED]>suffix";
        let needles = [b"secret-value".as_slice(), b"c2VjcmV0LXZhbHVl".as_slice()];

        for split in 0..=input.len() {
            assert_eq!(
                redact_with_chunks(&needles, input, &[split, input.len() - split]),
                expected,
                "split at byte {split}"
            );
        }
        for chunk_len in 1..=input.len() {
            assert_eq!(
                redact_with_chunks(
                    &needles,
                    input,
                    &vec![chunk_len; input.len().div_ceil(chunk_len)]
                ),
                expected,
                "chunk length {chunk_len}"
            );
        }
    }

    #[test]
    fn uses_deterministic_leftmost_longest_matching_for_overlaps() {
        let input = b"zabcdefghq--abcdefgh";
        let needles = [
            b"abcd".as_slice(),
            b"abcdefgh".as_slice(),
            b"bcdefgh".as_slice(),
            b"cdefgh".as_slice(),
        ];
        let expected = b"z[REDACTED]q--[REDACTED]";

        for split in 0..=input.len() {
            assert_eq!(
                redact_with_chunks(&needles, input, &[split, input.len() - split]),
                expected,
                "overlap split at byte {split}"
            );
        }
    }

    #[test]
    fn preserves_nonmatching_binary_bytes_exactly_and_redacts_binary_patterns() {
        let input = [0xff, 0x00, 0x01, 0x02, 0x03, 0xfe, 0x80];
        let expected = [&[0xff][..], EXEC_REDACTION_MARKER, &[0xfe, 0x80][..]].concat();
        for split in 0..=input.len() {
            assert_eq!(
                redact_with_chunks(&[&[0x00, 0x01, 0x02, 0x03]], &input, &[split]),
                expected
            );
        }
    }

    #[test]
    fn independent_streams_never_match_across_their_boundary() {
        let mut stdout = StreamingRedactor::new(patterns(&[b"secret-value"])).unwrap();
        let mut stderr = stdout.independent_stream();
        let mut stdout_bytes = Vec::new();
        let mut stderr_bytes = Vec::new();
        stdout.push_chunk(b"secret", &mut stdout_bytes).unwrap();
        stderr.push_chunk(b"-value", &mut stderr_bytes).unwrap();
        stdout.finish(&mut stdout_bytes).unwrap();
        stderr.finish(&mut stderr_bytes).unwrap();
        assert_eq!(stdout_bytes, b"secret");
        assert_eq!(stderr_bytes, b"-value");
    }

    #[test]
    fn preserves_output_larger_than_one_mibibyte_without_a_capture_cap() {
        let input = vec![0xa5; 1024 * 1024 + 17];
        let mut redactor = StreamingRedactor::new(Vec::new()).unwrap();
        let mut output = Vec::new();
        for chunk in input.chunks(8 * 1024) {
            redactor.push_chunk(chunk, &mut output).unwrap();
        }
        redactor.finish(&mut output).unwrap();
        assert_eq!(output, input);
    }

    #[test]
    fn overlap_memory_stays_within_pattern_plus_chunk_bounds() {
        let needle = vec![b'n'; MAX_EXEC_REDACTION_PATTERN_LEN];
        let mut redactor = StreamingRedactor::new(vec![Zeroizing::new(needle)]).unwrap();
        let mut output = Vec::new();
        for _ in 0..4 {
            redactor
                .push_chunk(&vec![b'x'; MAX_EXEC_OUTPUT_CHUNK_LEN], &mut output)
                .unwrap();
            assert!(redactor.pending_len() < MAX_EXEC_REDACTION_PATTERN_LEN);
        }
    }

    #[test]
    fn constructor_enforces_count_total_length_and_minimum_bounds() {
        let too_many = (0..=MAX_EXEC_REDACTION_PATTERNS)
            .map(|index| Zeroizing::new(index.to_le_bytes().to_vec()))
            .collect();
        assert!(StreamingRedactor::new(too_many).is_err());

        assert!(
            StreamingRedactor::new(vec![Zeroizing::new(vec![
                b'x';
                MAX_EXEC_REDACTION_PATTERN_LEN + 1
            ])])
            .is_err()
        );
        assert!(
            StreamingRedactor::new(vec![Zeroizing::new(vec![b'x'; MIN_REDACTABLE_LEN - 1])])
                .is_err()
        );

        let total_overflow = (0..=MAX_EXEC_REDACTION_PATTERN_BYTES
            / MAX_EXEC_REDACTION_PATTERN_LEN)
            .map(|index| {
                let mut pattern = vec![b'x'; MAX_EXEC_REDACTION_PATTERN_LEN];
                pattern[..8].copy_from_slice(&(index as u64).to_le_bytes());
                Zeroizing::new(pattern)
            })
            .collect();
        assert!(StreamingRedactor::new(total_overflow).is_err());
    }

    #[test]
    fn duplicate_patterns_are_deduplicated_and_debug_hides_bytes() {
        let redactor =
            StreamingRedactor::new(patterns(&[b"duplicate-secret", b"duplicate-secret"])).unwrap();
        assert_eq!(redactor.pattern_count, 1);
        let debug = format!("{redactor:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("duplicate-secret"));
    }

    #[test]
    fn oversized_input_chunk_is_rejected_before_working_memory_grows() {
        let mut redactor = StreamingRedactor::new(patterns(&[b"secret-value"])).unwrap();
        let mut output = Vec::new();
        let error = redactor
            .push_chunk(&vec![b'x'; MAX_EXEC_OUTPUT_CHUNK_LEN + 1], &mut output)
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(output.is_empty());
        assert_eq!(redactor.pending_len(), 0);
    }
}
