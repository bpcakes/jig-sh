use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;

use crate::{Result, SecretBytes, VaultError, VaultErrorKind, VaultReference};

pub const MAX_TEMPLATE_INPUT_LEN: usize = 16 * 1024 * 1024;
pub const MAX_TEMPLATE_OUTPUT_LEN: usize = 16 * 1024 * 1024;

const MAX_TEMPLATE_PLACEHOLDERS: usize = 4_096;
const MAX_TEMPLATE_REFERENCES: usize = 1_024;

/// A validated, bounded byte template for controlled vault injection.
///
/// Parsing performs no vault access. Callers can therefore reject malformed
/// input before capturing a passphrase or touching encrypted state. The type
/// deliberately exposes neither source bytes nor its reference list.
pub struct InjectionTemplate {
    source: SecretBytes,
    chunks: Vec<TemplateChunk>,
    references: Vec<VaultReference>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TemplateChunk {
    Literal(Range<usize>),
    Reference(usize),
}

impl InjectionTemplate {
    /// Parses a deterministic Jig byte template.
    ///
    /// # Errors
    ///
    /// Returns an invalid-input error when the template exceeds its bound or
    /// contains a malformed Jig placeholder.
    pub fn parse(source: SecretBytes) -> Result<Self> {
        if source.len() > MAX_TEMPLATE_INPUT_LEN {
            return Err(invalid_template(format!(
                "template input exceeds the {MAX_TEMPLATE_INPUT_LEN} byte limit"
            )));
        }

        let bytes = source.as_slice();
        let mut chunks = Vec::new();
        let mut references = Vec::new();
        let mut reference_indexes = BTreeMap::new();
        let mut cursor = 0;
        let mut search_from = 0;
        let mut placeholder_count = 0;

        while let Some(relative_open) = find_pair(&bytes[search_from..], b'{', b'{') {
            let open = search_from + relative_open;
            let body_start = open + 2;
            let Some(relative_close) = find_pair(&bytes[body_start..], b'}', b'}') else {
                if looks_like_jig_placeholder(&bytes[body_start..]) {
                    return Err(invalid_template(
                        "malformed Jig placeholder: missing closing braces",
                    ));
                }
                break;
            };
            let close = body_start + relative_close;
            let body = trim_ascii_whitespace(&bytes[body_start..close]);

            if is_canonical_jig_prefix(body) {
                placeholder_count += 1;
                if placeholder_count > MAX_TEMPLATE_PLACEHOLDERS {
                    return Err(invalid_template(format!(
                        "template contains more than {MAX_TEMPLATE_PLACEHOLDERS} Jig placeholders"
                    )));
                }
                let spelling = std::str::from_utf8(body).map_err(|_| {
                    invalid_template("malformed Jig placeholder: reference must be ASCII")
                })?;
                let reference = VaultReference::parse(spelling).map_err(|error| {
                    invalid_template(format!("malformed Jig placeholder: {}", error.message()))
                })?;
                let reference_index = match reference_indexes.get(&reference).copied() {
                    Some(index) => index,
                    None => {
                        if references.len() >= MAX_TEMPLATE_REFERENCES {
                            return Err(invalid_template(format!(
                                "template contains more than {MAX_TEMPLATE_REFERENCES} distinct Jig references"
                            )));
                        }
                        let index = references.len();
                        reference_indexes.insert(reference.clone(), index);
                        references.push(reference);
                        index
                    }
                };
                if cursor < open {
                    chunks.push(TemplateChunk::Literal(cursor..open));
                }
                chunks.push(TemplateChunk::Reference(reference_index));
                cursor = close + 2;
                search_from = cursor;
            } else if looks_like_jig_placeholder(body) {
                return Err(invalid_template(
                    "malformed Jig placeholder: expected {{ jig://ITEM/FIELD }}",
                ));
            } else {
                // This is some other application's brace expression. Preserve
                // it byte-for-byte and continue after its close delimiter.
                search_from = close + 2;
            }
        }

        if cursor < bytes.len() {
            chunks.push(TemplateChunk::Literal(cursor..bytes.len()));
        }

        Ok(Self {
            source,
            chunks,
            references,
        })
    }

    pub(crate) fn references(&self) -> &[VaultReference] {
        &self.references
    }

    pub(crate) fn render(self, values: &[SecretBytes]) -> Result<SecretBytes> {
        if values.len() != self.references.len() {
            return Err(invalid_template(
                "internal template resolution count does not match parsed references",
            ));
        }

        let mut output_len = 0_usize;
        for chunk in &self.chunks {
            let chunk_len = match chunk {
                TemplateChunk::Literal(range) => range.len(),
                TemplateChunk::Reference(index) => values[*index].len(),
            };
            output_len = output_len.checked_add(chunk_len).ok_or_else(|| {
                invalid_template("rendered template length exceeds supported bounds")
            })?;
            if output_len > MAX_TEMPLATE_OUTPUT_LEN {
                return Err(invalid_template(format!(
                    "rendered template exceeds the {MAX_TEMPLATE_OUTPUT_LEN} byte limit"
                )));
            }
        }

        let mut output = SecretBytes::with_capacity(output_len);
        for chunk in self.chunks {
            let bytes = match chunk {
                TemplateChunk::Literal(range) => &self.source.as_slice()[range],
                TemplateChunk::Reference(index) => values[index].as_slice(),
            };
            output.extend_from_slice(bytes).map_err(|_| {
                invalid_template("rendered template exceeded its validated allocation")
            })?;
        }
        Ok(output)
    }
}

impl fmt::Debug for InjectionTemplate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InjectionTemplate")
            .field("source_len", &self.source.len())
            .field("chunk_count", &self.chunks.len())
            .field("references", &self.references)
            .field("source", &"[REDACTED]")
            .finish()
    }
}

fn invalid_template(message: impl Into<String>) -> VaultError {
    VaultError::new(VaultErrorKind::InvalidInput, message)
}

fn find_pair(bytes: &[u8], first: u8, second: u8) -> Option<usize> {
    bytes
        .windows(2)
        .position(|window| window[0] == first && window[1] == second)
}

fn trim_ascii_whitespace(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn is_canonical_jig_prefix(bytes: &[u8]) -> bool {
    bytes.starts_with(b"jig://")
}

fn looks_like_jig_placeholder(bytes: &[u8]) -> bool {
    let bytes = trim_ascii_whitespace(bytes);
    bytes.windows(3).enumerate().any(|(index, window)| {
        window.eq_ignore_ascii_case(b"jig")
            && bytes
                .get(index + 3)
                .is_none_or(|next| next.is_ascii_whitespace() || matches!(next, b':' | b'/'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(bytes: &[u8]) -> SecretBytes {
        SecretBytes::new(bytes.to_vec())
    }

    #[test]
    fn parses_deduplicates_and_renders_binary_references() {
        let parsed = InjectionTemplate::parse(value(
            b"before={{ jig://Prod/TOKEN }}\0{{jig://Prod/TOKEN}}:{{\n jig://Prod/FLAG\t}}",
        ))
        .unwrap();
        assert_eq!(
            parsed.references(),
            &[
                VaultReference::parse("jig://Prod/TOKEN").unwrap(),
                VaultReference::parse("jig://Prod/FLAG").unwrap(),
            ]
        );

        let rendered = parsed
            .render(&[value(&[0xff, 0x00]), value(b"false")])
            .unwrap();
        assert_eq!(rendered.as_slice(), b"before=\xff\0\0\xff\0:false");
    }

    #[test]
    fn preserves_unrelated_braces_exactly() {
        let input = b"{{ other }} {single} {{not-a-reference}} tail";
        let parsed = InjectionTemplate::parse(value(input)).unwrap();
        let rendered = parsed.render(&[]).unwrap();
        assert_eq!(rendered.as_slice(), input);
    }

    #[test]
    fn rejects_malformed_jig_placeholders() {
        for input in [
            b"{{ jig://Prod }}".as_slice(),
            b"{{ jig:Prod/TOKEN }}",
            b"{{ jig //Prod/TOKEN }}",
            b"{{ JIG://Prod/TOKEN }}",
            b"{{ prefix jig://Prod/TOKEN }}",
            b"{{ jig://Prod/TOKEN",
            b"{{ jig://Prod/TOKEN extra }}",
        ] {
            let error = InjectionTemplate::parse(value(input)).unwrap_err();
            assert!(error.to_string().contains("malformed Jig placeholder"));
            assert!(!error.to_string().contains("TOKEN extra"));
        }
    }

    #[test]
    fn enforces_input_and_rendered_output_limits() {
        let error =
            InjectionTemplate::parse(SecretBytes::new(vec![b'x'; MAX_TEMPLATE_INPUT_LEN + 1]))
                .unwrap_err();
        assert!(error.to_string().contains("template input exceeds"));

        let mut input = vec![b'x'; MAX_TEMPLATE_OUTPUT_LEN];
        input.extend_from_slice(b"{{jig://Prod/TOKEN}}");
        // The oversized source is rejected before parsing, so use a template
        // just below the input cap whose replacement pushes the output over.
        input.drain(..32);
        let parsed = InjectionTemplate::parse(SecretBytes::new(input)).unwrap();
        let error = parsed
            .render(&[value(b"a much longer value than placeholder")])
            .unwrap_err();
        assert!(error.to_string().contains("rendered template exceeds"));
    }

    #[test]
    fn debug_hides_template_source() {
        let parsed = InjectionTemplate::parse(value(b"private={{ jig://Prod/TOKEN }}")).unwrap();
        let debug = format!("{parsed:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("private="));
    }
}
