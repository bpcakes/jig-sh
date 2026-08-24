use base64::Engine;
use base64::engine::general_purpose::{
    STANDARD as B64, STANDARD_NO_PAD as B64_NOPAD, URL_SAFE as B64_URL,
    URL_SAFE_NO_PAD as B64_URL_NOPAD,
};
use std::fmt;

use crate::SecretBytes;

pub const MIN_REDACTABLE_LEN: usize = 4;

#[derive(Clone, Default)]
pub struct Redactor {
    text_needles: Vec<TextNeedle>,
    raw_needles: Vec<RawNeedle>,
}

impl fmt::Debug for Redactor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Redactor")
            .field("text_needle_count", &self.text_needles.len())
            .field("raw_needle_count", &self.raw_needles.len())
            .finish()
    }
}

#[derive(Clone)]
struct TextNeedle {
    value: String,
    marker: &'static str,
}

impl Drop for TextNeedle {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.value);
    }
}

#[derive(Clone)]
struct RawNeedle {
    value: zeroize::Zeroizing<Vec<u8>>,
}

impl Redactor {
    pub fn from_secret_values(values: &[SecretBytes]) -> Self {
        Self::from_secret_slices(values.iter().map(SecretBytes::as_slice))
    }

    pub(crate) fn from_secret_slices<'a>(values: impl IntoIterator<Item = &'a [u8]>) -> Self {
        let mut text_needles = Vec::new();
        let mut raw_needles = Vec::new();
        for value in values {
            if value.is_empty() {
                continue;
            }
            push_unique_raw(&mut raw_needles, value);
            push_unique_text(&mut text_needles, B64.encode(value), "[REDACTED_B64]");
            push_unique_text(&mut text_needles, B64_NOPAD.encode(value), "[REDACTED_B64]");
            push_unique_text(
                &mut text_needles,
                B64_URL.encode(value),
                "[REDACTED_B64URL]",
            );
            push_unique_text(
                &mut text_needles,
                B64_URL_NOPAD.encode(value),
                "[REDACTED_B64URL]",
            );
            let lower_hex = hex(value, false);
            push_unique_text(&mut text_needles, lower_hex, "[REDACTED_HEX]");
            let upper_hex = hex(value, true);
            push_unique_text(&mut text_needles, upper_hex, "[REDACTED_HEX]");
            push_unique_text(&mut text_needles, base32(value, true), "[REDACTED_BASE32]");
            push_unique_text(&mut text_needles, base32(value, false), "[REDACTED_BASE32]");
            push_unique_text(
                &mut text_needles,
                base32(value, true).to_ascii_lowercase(),
                "[REDACTED_BASE32]",
            );
            push_unique_text(
                &mut text_needles,
                base32(value, false).to_ascii_lowercase(),
                "[REDACTED_BASE32]",
            );
            match std::str::from_utf8(value) {
                Ok(raw) => {
                    push_unique_text(&mut text_needles, raw.to_string(), "[REDACTED]");
                    push_unique_text(
                        &mut text_needles,
                        percent_encode(raw, true),
                        "[REDACTED_URL]",
                    );
                    push_unique_text(
                        &mut text_needles,
                        percent_encode(raw, false),
                        "[REDACTED_URL]",
                    );
                    push_unique_text(
                        &mut text_needles,
                        double_url_encode(raw, true),
                        "[REDACTED_URL2]",
                    );
                    push_unique_text(
                        &mut text_needles,
                        double_url_encode(raw, false),
                        "[REDACTED_URL2]",
                    );
                    push_unique_text(
                        &mut text_needles,
                        doubled_percent_encode(raw, true),
                        "[REDACTED_DOUBLE_PERCENT]",
                    );
                    push_unique_text(
                        &mut text_needles,
                        doubled_percent_encode(raw, false),
                        "[REDACTED_DOUBLE_PERCENT]",
                    );
                    push_unique_text(&mut text_needles, json_escape(raw), "[REDACTED_JSON]");
                    push_unique_text(
                        &mut text_needles,
                        html_decimal_escape(raw),
                        "[REDACTED_HTML]",
                    );
                    push_unique_text(
                        &mut text_needles,
                        html_hex_escape(raw, false, false),
                        "[REDACTED_HTML]",
                    );
                    push_unique_text(
                        &mut text_needles,
                        html_hex_escape(raw, false, true),
                        "[REDACTED_HTML]",
                    );
                    push_unique_text(
                        &mut text_needles,
                        html_hex_escape(raw, true, false),
                        "[REDACTED_HTML]",
                    );
                    push_unique_text(
                        &mut text_needles,
                        html_hex_escape(raw, true, true),
                        "[REDACTED_HTML]",
                    );
                    push_unique_text(
                        &mut text_needles,
                        unicode_escape(raw, false),
                        "[REDACTED_UNICODE]",
                    );
                    push_unique_text(
                        &mut text_needles,
                        unicode_escape(raw, true),
                        "[REDACTED_UNICODE]",
                    );
                }
                Err(_) => {
                    // Binary secrets are still covered by raw byte, base64, and hex redaction.
                }
            }
        }
        text_needles.sort_by_key(|needle| std::cmp::Reverse(needle.value.len()));
        raw_needles.sort_by_key(|needle| std::cmp::Reverse(needle.value.len()));
        Self {
            text_needles,
            raw_needles,
        }
    }

    pub(crate) fn append_streaming_patterns(
        &self,
        patterns: &mut Vec<zeroize::Zeroizing<Vec<u8>>>,
        total_bytes: &mut usize,
        max_patterns: usize,
        max_total_bytes: usize,
        max_pattern_len: usize,
    ) -> crate::Result<()> {
        for value in self
            .raw_needles
            .iter()
            .map(|needle| needle.value.as_slice())
            .chain(
                self.text_needles
                    .iter()
                    .map(|needle| needle.value.as_bytes()),
            )
        {
            if value.len() > max_pattern_len {
                return Err(crate::VaultError::new(
                    crate::VaultErrorKind::InvalidInput,
                    "concealed field generates an output-redaction pattern above the supported length limit",
                ));
            }
            *total_bytes = total_bytes.checked_add(value.len()).ok_or_else(|| {
                crate::VaultError::new(
                    crate::VaultErrorKind::InvalidInput,
                    "concealed fields generate too much output-redaction pattern data",
                )
            })?;
            if *total_bytes > max_total_bytes {
                return Err(crate::VaultError::new(
                    crate::VaultErrorKind::InvalidInput,
                    "concealed fields generate too much output-redaction pattern data",
                ));
            }
            if patterns.len() >= max_patterns {
                return Err(crate::VaultError::new(
                    crate::VaultErrorKind::InvalidInput,
                    "concealed fields generate too many output-redaction patterns",
                ));
            }
            patterns.push(zeroize::Zeroizing::new(value.to_vec()));
        }
        Ok(())
    }

    pub fn redact_str(&self, input: &str) -> String {
        // The v1 broker applies redaction once to capped 1 MiB streams. This
        // straightforward scan-per-needle approach is intentionally bounded by
        // that cap; switch to multi-pattern matching before using it on larger
        // or streaming outputs.
        let mut output = input.to_string();
        for needle in &self.text_needles {
            if !needle.value.is_empty() {
                output = output.replace(&needle.value, needle.marker);
            }
        }
        output
    }

    pub fn redact_bytes_lossy(&self, input: &[u8]) -> String {
        // Raw-byte redaction intentionally runs before lossy UTF-8 decoding so
        // binary secret values are covered. This can replace coincidental byte
        // matches in non-secret binary output; redaction is a safety net.
        let redacted = self.redact_raw_bytes(input);
        self.redact_str(&String::from_utf8_lossy(&redacted))
    }

    fn redact_raw_bytes(&self, input: &[u8]) -> Vec<u8> {
        // See `redact_str` for the bounded-cost assumption behind the simple
        // per-needle replacement loop.
        let mut output = input.to_vec();
        for needle in &self.raw_needles {
            output = replace_bytes(&output, &needle.value, b"[REDACTED]");
        }
        output
    }
}

fn push_unique_text(needles: &mut Vec<TextNeedle>, value: String, marker: &'static str) {
    let mut value = value;
    if value.len() < MIN_REDACTABLE_LEN {
        zeroize::Zeroize::zeroize(&mut value);
        return;
    }
    if needles.iter().any(|needle| needle.value == value) {
        zeroize::Zeroize::zeroize(&mut value);
        return;
    }
    needles.push(TextNeedle { value, marker });
}

fn push_unique_raw(needles: &mut Vec<RawNeedle>, value: &[u8]) {
    if value.len() < MIN_REDACTABLE_LEN {
        return;
    }
    if needles
        .iter()
        .any(|needle| needle.value.as_slice() == value)
    {
        return;
    }
    needles.push(RawNeedle {
        value: zeroize::Zeroizing::new(value.to_vec()),
    });
}

fn replace_bytes(input: &[u8], needle: &[u8], marker: &[u8]) -> Vec<u8> {
    if needle.is_empty() {
        return input.to_vec();
    }
    let mut output = Vec::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(offset) = input[cursor..]
        .windows(needle.len())
        .position(|window| window == needle)
    {
        let start = cursor + offset;
        output.extend_from_slice(&input[cursor..start]);
        output.extend_from_slice(marker);
        cursor = start + needle.len();
    }
    output.extend_from_slice(&input[cursor..]);
    output
}

fn hex(bytes: &[u8], upper: bool) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        if upper {
            write!(&mut output, "{byte:02X}").expect("writing to String cannot fail");
        } else {
            write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
        }
    }
    output
}

fn base32(bytes: &[u8], padded: bool) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

    let mut output = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let mut buffer = 0_u16;
    let mut bits = 0_u8;
    for byte in bytes {
        buffer = (buffer << 8) | u16::from(*byte);
        bits += 8;
        while bits >= 5 {
            let shift = bits - 5;
            let index = ((buffer >> shift) & 0b11111) as usize;
            output.push(ALPHABET[index] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        let index = ((buffer << (5 - bits)) & 0b11111) as usize;
        output.push(ALPHABET[index] as char);
    }
    if padded {
        while !output.len().is_multiple_of(8) {
            output.push('=');
        }
    }
    output
}

fn percent_encode(input: &str, upper: bool) -> String {
    let mut output = String::new();
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            output.push(byte as char);
        } else {
            use std::fmt::Write;
            if upper {
                write!(&mut output, "%{byte:02X}").expect("writing to String cannot fail");
            } else {
                write!(&mut output, "%{byte:02x}").expect("writing to String cannot fail");
            }
        }
    }
    output
}

fn double_url_encode(input: &str, upper: bool) -> String {
    percent_encode(&percent_encode(input, upper), upper)
}

fn doubled_percent_encode(input: &str, upper: bool) -> String {
    percent_encode(input, upper).replace('%', "%%")
}

fn json_escape(input: &str) -> String {
    // Match serde_json's canonical string escaping. Producers that choose
    // alternate legal JSON spellings, such as \u00XX for printable ASCII, are
    // outside this v1 redaction form and should still be caught by raw text.
    let quoted = serde_json::to_string(input).expect("serializing a string to JSON cannot fail");
    debug_assert!(quoted.starts_with('"') && quoted.ends_with('"'));
    quoted
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(&quoted)
        .to_string()
}

fn html_decimal_escape(input: &str) -> String {
    let mut output = String::new();
    for ch in input.chars() {
        use std::fmt::Write;
        write!(&mut output, "&#{};", ch as u32).expect("writing to String cannot fail");
    }
    output
}

fn html_hex_escape(input: &str, upper_x: bool, upper_digits: bool) -> String {
    let mut output = String::new();
    for ch in input.chars() {
        use std::fmt::Write;
        let prefix = if upper_x { "&#X" } else { "&#x" };
        match upper_digits {
            true => write!(&mut output, "{prefix}{:X};", ch as u32),
            false => write!(&mut output, "{prefix}{:x};", ch as u32),
        }
        .expect("writing to String cannot fail");
    }
    output
}

fn unicode_escape(input: &str, upper: bool) -> String {
    let mut output = String::new();
    for ch in input.chars() {
        if ch.is_ascii() {
            use std::fmt::Write;
            if upper {
                write!(&mut output, "\\u{:04X}", ch as u32).expect("writing to String cannot fail");
            } else {
                write!(&mut output, "\\u{:04x}", ch as u32).expect("writing to String cannot fail");
            }
        } else {
            // Non-ASCII code points have multiple JSON spellings, including
            // surrogate pairs above U+FFFF. Keep the raw character form here;
            // UTF-8 raw text and byte redaction cover the common path.
            output.push(ch);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_raw_and_encoded_forms() {
        let secret = SecretBytes::new(b"secret-value/123".to_vec());
        let redactor = Redactor::from_secret_values(&[secret]);
        let input = [
            "secret-value/123",
            "c2VjcmV0LXZhbHVlLzEyMw==",
            "c2VjcmV0LXZhbHVlLzEyMw",
            "7365637265742d76616c75652f313233",
            "7365637265742D76616C75652F313233",
            "secret-value%2F123",
            "secret-value%2f123",
            "secret-value%252F123",
            "secret-value%252f123",
            "secret-value%%2F123",
            "secret-value%%2f123",
            "ONSWG4TFOQWXMYLMOVSS6MJSGM======",
            "ONSWG4TFOQWXMYLMOVSS6MJSGM",
            "onswg4tfoqwxmylmovss6mjsgm======",
            "onswg4tfoqwxmylmovss6mjsgm",
            "&#115;&#101;&#99;&#114;&#101;&#116;&#45;&#118;&#97;&#108;&#117;&#101;&#47;&#49;&#50;&#51;",
            "&#x73;&#x65;&#x63;&#x72;&#x65;&#x74;&#x2d;&#x76;&#x61;&#x6c;&#x75;&#x65;&#x2f;&#x31;&#x32;&#x33;",
            "&#x73;&#x65;&#x63;&#x72;&#x65;&#x74;&#x2D;&#x76;&#x61;&#x6C;&#x75;&#x65;&#x2F;&#x31;&#x32;&#x33;",
            "&#X73;&#X65;&#X63;&#X72;&#X65;&#X74;&#X2d;&#X76;&#X61;&#X6c;&#X75;&#X65;&#X2f;&#X31;&#X32;&#X33;",
            "&#X73;&#X65;&#X63;&#X72;&#X65;&#X74;&#X2D;&#X76;&#X61;&#X6C;&#X75;&#X65;&#X2F;&#X31;&#X32;&#X33;",
            "\\u0073\\u0065\\u0063\\u0072\\u0065\\u0074\\u002d\\u0076\\u0061\\u006c\\u0075\\u0065\\u002f\\u0031\\u0032\\u0033",
            "\\u0073\\u0065\\u0063\\u0072\\u0065\\u0074\\u002D\\u0076\\u0061\\u006C\\u0075\\u0065\\u002F\\u0031\\u0032\\u0033",
        ]
        .join("\n");
        let redacted = redactor.redact_str(&input);
        assert!(!redacted.contains("secret-value/123"));
        assert!(!redacted.contains("c2VjcmV0"));
        assert!(!redacted.contains("736563"));
        assert!(!redacted.contains("ONSWG4TF"));
        assert!(!redacted.contains("&#115;"));
        assert!(!redacted.contains("%252F"));
        assert!(!redacted.contains("%%2F"));
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn preserves_line_count() {
        let secret = SecretBytes::new(b"secret-value".to_vec());
        let redactor = Redactor::from_secret_values(&[secret]);
        let input = "a\nsecret-value\nb\n";
        let redacted = redactor.redact_str(input);
        assert_eq!(input.lines().count(), redacted.lines().count());
    }

    #[test]
    fn redacts_encoded_binary_secret() {
        let secret = SecretBytes::new(vec![0, 159, 146, 150, 255]);
        let redactor = Redactor::from_secret_values(&[secret]);
        let redacted = redactor.redact_str("AJ+Slv8= 009f9296ff");
        assert!(!redacted.contains("AJ+Slv8="));
        assert!(!redacted.contains("009f9296ff"));
    }

    #[test]
    fn redacts_raw_binary_bytes_before_lossy_utf8_conversion() {
        let secret = SecretBytes::new(vec![0, 159, 146, 150, 255]);
        let redactor = Redactor::from_secret_values(&[secret]);
        let redacted = redactor.redact_bytes_lossy(&[b'a', 0, 159, 146, 150, 255, b'z']);
        assert_eq!(redacted, "a[REDACTED]z");
    }

    #[test]
    fn debug_output_does_not_include_secret_needles() {
        let secret = SecretBytes::new(b"secret-value".to_vec());
        let redactor = Redactor::from_secret_values(&[secret]);
        let debug = format!("{redactor:?}");
        assert!(!debug.contains("secret-value"));
        assert!(!debug.contains("c2VjcmV0LXZhbHVl"));
    }
}
