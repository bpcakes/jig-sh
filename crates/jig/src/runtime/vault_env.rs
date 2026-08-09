//! Restricted dotenv parsing for transparent vault execution.
//!
//! This deliberately is not a general dotenv implementation. Input is bounded
//! UTF-8. Blank lines and full-line comments may contain leading ASCII spaces
//! or tabs, while assignments must be exact `NAME=VALUE` lines with no
//! whitespace around the name or `=`. Unquoted values accept no raw whitespace,
//! `#`, or quote characters and support only `\\`, `\ `, `\#`, `\'`, `\"`,
//! `\n`, `\r`, and `\t`. Single-quoted values are literal and do not process
//! backslashes. Double-quoted values support only `\\`, `\"`, `\n`, `\r`, and
//! `\t`. Dollar signs and backticks are rejected in every value form, so no
//! interpolation or command substitution syntax is accepted.

use std::collections::HashSet;
use std::io::{ErrorKind, Read};
use std::path::Path;

use anyhow::{Context, Result, bail};
use jig_vault::{MAX_SECRET_VALUE_LEN, SecretBytes, VaultReference};
use zeroize::Zeroizing;

use crate::command::{VaultExecAssignment, VaultExecEnvironment, VaultExecValue};

pub(crate) const MAX_VAULT_ENV_FILE_LEN: usize = 1024 * 1024;
pub(crate) const MAX_VAULT_ENV_ASSIGNMENTS: usize = 1024;
pub(crate) const MAX_VAULT_ENV_TOTAL_DECODED_LEN: usize = 1024 * 1024;

const PASSPHRASE_ENV: &str = "JIG_VAULT_PASSPHRASE";
const NEW_PASSPHRASE_ENV: &str = "JIG_VAULT_NEW_PASSPHRASE";

pub(crate) fn parse_vault_env_file(path: &Path) -> Result<VaultExecEnvironment> {
    if path == Path::new("-") {
        bail!("vault exec rejects --env-file - so the child can inherit stdin");
    }
    let bytes = read_bounded_file(path)?;
    parse_vault_env_bytes(bytes.as_slice())
}

pub(crate) fn parse_vault_env_bytes(input: &[u8]) -> Result<VaultExecEnvironment> {
    if input.len() > MAX_VAULT_ENV_FILE_LEN {
        bail!("vault exec env file exceeds the {MAX_VAULT_ENV_FILE_LEN} byte limit");
    }
    if let Some(offset) = input.iter().position(|byte| *byte == 0) {
        bail!(
            "vault exec env line {} contains a NUL byte",
            line_number_at(input, offset)
        );
    }
    let text = std::str::from_utf8(input).map_err(|error| {
        anyhow::anyhow!(
            "vault exec env line {} is not valid UTF-8",
            line_number_at(input, error.valid_up_to())
        )
    })?;

    let mut assignments = Vec::new();
    let mut names = HashSet::new();
    let mut total_decoded_len = 0_usize;

    for (index, raw_line) in text.split('\n').enumerate() {
        let line_number = index + 1;
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let indented = line.trim_start_matches([' ', '\t']);
        if indented.is_empty() || indented.starts_with('#') {
            continue;
        }
        if line.as_bytes().contains(&b'\r') {
            bail!("vault exec env line {line_number} contains an unsupported control character");
        }

        let Some((name, raw_value)) = line.split_once('=') else {
            bail!("vault exec env line {line_number} must be an exact NAME=VALUE assignment");
        };
        if !valid_env_name(name) {
            bail!("vault exec env line {line_number} has an invalid environment variable name");
        }
        if reserved_env_name(name) {
            bail!("vault exec env line {line_number} may not assign reserved variable '{name}'");
        }
        let comparison_name = comparable_env_name(name);
        if !names.insert(comparison_name) {
            bail!("vault exec env line {line_number} duplicates variable '{name}'");
        }
        if assignments.len() >= MAX_VAULT_ENV_ASSIGNMENTS {
            bail!(
                "vault exec env line {line_number} exceeds the {MAX_VAULT_ENV_ASSIGNMENTS} assignment limit"
            );
        }

        let decoded = decode_value(raw_value.as_bytes()).map_err(|reason| {
            anyhow::anyhow!("vault exec env line {line_number} variable '{name}': {reason}")
        })?;
        if decoded.len() > MAX_SECRET_VALUE_LEN {
            bail!(
                "vault exec env line {line_number} variable '{name}' exceeds the {MAX_SECRET_VALUE_LEN} byte value limit"
            );
        }
        total_decoded_len = total_decoded_len
            .checked_add(name.len())
            .and_then(|total| total.checked_add(decoded.len()))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "vault exec env line {line_number} variable '{name}' exceeds the total decoded data limit"
                )
            })?;
        if total_decoded_len > MAX_VAULT_ENV_TOTAL_DECODED_LEN {
            bail!(
                "vault exec env line {line_number} variable '{name}' exceeds the {MAX_VAULT_ENV_TOTAL_DECODED_LEN} byte total decoded data limit"
            );
        }

        let value = classify_value(decoded).map_err(|reason| {
            anyhow::anyhow!("vault exec env line {line_number} variable '{name}': {reason}")
        })?;
        assignments.push(VaultExecAssignment {
            line: line_number,
            name: name.to_owned(),
            value,
        });
    }

    Ok(VaultExecEnvironment { assignments })
}

fn read_bounded_file(path: &Path) -> Result<SecretBytes> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("failed to open vault exec env file {}", path.display()))?;
    let capacity = MAX_VAULT_ENV_FILE_LEN
        .checked_add(1)
        .expect("vault env file limit leaves room for an overflow byte");
    let mut bytes = SecretBytes::with_capacity(capacity);
    let mut chunk = Zeroizing::new([0_u8; 8 * 1024]);
    loop {
        let remaining = capacity - bytes.len();
        let chunk_len = remaining.min(chunk.len());
        let read = match file.read(&mut chunk[..chunk_len]) {
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to read vault exec env file {}", path.display())
                });
            }
        };
        if read == 0 {
            break;
        }
        bytes
            .extend_from_slice(&chunk[..read])
            .expect("the bounded vault env buffer was preallocated exactly");
        if bytes.len() > MAX_VAULT_ENV_FILE_LEN {
            bail!(
                "vault exec env file {} exceeds the {MAX_VAULT_ENV_FILE_LEN} byte limit",
                path.display()
            );
        }
    }
    Ok(bytes)
}

fn decode_value(raw: &[u8]) -> std::result::Result<SecretBytes, &'static str> {
    if raw.iter().any(|byte| matches!(byte, b'$' | b'`')) {
        return Err("interpolation and command substitution syntax is not supported");
    }
    match raw.first() {
        Some(b'\'') => decode_single_quoted(raw),
        Some(b'"') => decode_double_quoted(raw),
        _ => decode_unquoted(raw),
    }
}

fn decode_single_quoted(raw: &[u8]) -> std::result::Result<SecretBytes, &'static str> {
    if raw.len() < 2 || raw.last() != Some(&b'\'') {
        return Err("single-quoted value is not terminated exactly");
    }
    let contents = &raw[1..raw.len() - 1];
    if contents.contains(&b'\'') {
        return Err("single-quoted value has trailing or embedded content");
    }
    if contents.iter().any(|byte| byte.is_ascii_control()) {
        return Err("single-quoted value contains an unsupported control character");
    }
    Ok(SecretBytes::new(contents.to_vec()))
}

fn decode_double_quoted(raw: &[u8]) -> std::result::Result<SecretBytes, &'static str> {
    if raw.len() < 2 || raw.last() != Some(&b'"') {
        return Err("double-quoted value is not terminated exactly");
    }
    decode_escaped(&raw[1..raw.len() - 1], EscapeMode::DoubleQuoted)
}

fn decode_unquoted(raw: &[u8]) -> std::result::Result<SecretBytes, &'static str> {
    decode_escaped(raw, EscapeMode::Unquoted)
}

#[derive(Clone, Copy)]
enum EscapeMode {
    Unquoted,
    DoubleQuoted,
}

fn decode_escaped(raw: &[u8], mode: EscapeMode) -> std::result::Result<SecretBytes, &'static str> {
    let mut decoded = Zeroizing::new(Vec::with_capacity(raw.len()));
    let mut index = 0;
    while index < raw.len() {
        let byte = raw[index];
        if byte == b'\\' {
            let Some(escaped) = raw.get(index + 1).copied() else {
                return Err("value ends with an incomplete escape");
            };
            let decoded_byte = decode_escape(escaped, mode)?;
            decoded.push(decoded_byte);
            index += 2;
            continue;
        }
        if byte.is_ascii_control() {
            return Err("value contains an unsupported control character");
        }
        match mode {
            EscapeMode::Unquoted if matches!(byte, b' ' | b'#' | b'\'' | b'"') => {
                return Err(
                    "unquoted whitespace, #, and quote characters must be escaped or quoted",
                );
            }
            EscapeMode::DoubleQuoted if byte == b'"' => {
                return Err("double-quoted value has trailing or embedded content");
            }
            _ => decoded.push(byte),
        }
        index += 1;
    }
    Ok(SecretBytes::new(std::mem::take(&mut *decoded)))
}

fn decode_escape(escaped: u8, mode: EscapeMode) -> std::result::Result<u8, &'static str> {
    match escaped {
        b'\\' => Ok(b'\\'),
        b'n' => Ok(b'\n'),
        b'r' => Ok(b'\r'),
        b't' => Ok(b'\t'),
        b'"' => Ok(b'"'),
        b' ' | b'#' | b'\'' if matches!(mode, EscapeMode::Unquoted) => Ok(escaped),
        _ => Err("value contains an unsupported escape"),
    }
}

fn classify_value(value: SecretBytes) -> std::result::Result<VaultExecValue, &'static str> {
    let bytes = value.as_slice();
    if bytes.starts_with(b"jig://") {
        let spelling = std::str::from_utf8(bytes)
            .map_err(|_| "Jig reference must use its canonical ASCII spelling")?;
        let reference = VaultReference::parse(spelling)
            .map_err(|_| "Jig reference must be canonical jig://ITEM/FIELD")?;
        return Ok(VaultExecValue::Field(reference));
    }
    if contains_ascii_case_insensitive(bytes, b"jig:") {
        return Err("Jig-looking value must be canonical jig://ITEM/FIELD");
    }
    Ok(VaultExecValue::Literal(value))
}

fn contains_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

fn valid_env_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn reserved_env_name(name: &str) -> bool {
    env_names_equal(name, PASSPHRASE_ENV) || env_names_equal(name, NEW_PASSPHRASE_ENV)
}

fn comparable_env_name(name: &str) -> String {
    if cfg!(windows) {
        name.to_ascii_uppercase()
    } else {
        name.to_owned()
    }
}

fn env_names_equal(left: &str, right: &str) -> bool {
    if cfg!(windows) {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
}

fn line_number_at(input: &[u8], offset: usize) -> usize {
    1 + input[..offset.min(input.len())]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn literal(assignment: &VaultExecAssignment) -> &[u8] {
        match &assignment.value {
            VaultExecValue::Literal(value) => value.as_slice(),
            VaultExecValue::Field(reference) => {
                panic!("expected literal, got {reference}")
            }
        }
    }

    #[test]
    fn parses_exact_assignments_quotes_escapes_comments_and_references() {
        let parsed = parse_vault_env_bytes(
            b"\n  # comment\r\nPLAIN=value\nEMPTY=\nSPACE=two\\ words\nHASH=left\\#right\nSINGLE='literal # \\n'\nDOUBLE=\"line\\nquote=\\\" tab=\\t\"\nTOKEN=jig://Production/TOKEN\n",
        )
        .unwrap();

        assert_eq!(parsed.assignments.len(), 7);
        assert_eq!(parsed.assignments[0].line, 3);
        assert_eq!(literal(&parsed.assignments[0]), b"value");
        assert_eq!(literal(&parsed.assignments[1]), b"");
        assert_eq!(literal(&parsed.assignments[2]), b"two words");
        assert_eq!(literal(&parsed.assignments[3]), b"left#right");
        assert_eq!(literal(&parsed.assignments[4]), b"literal # \\n");
        assert_eq!(literal(&parsed.assignments[5]), b"line\nquote=\" tab=\t");
        match &parsed.assignments[6].value {
            VaultExecValue::Field(reference) => {
                assert_eq!(reference.to_string(), "jig://Production/TOKEN");
            }
            VaultExecValue::Literal(_) => panic!("expected field binding"),
        }
    }

    #[test]
    fn rejects_substitution_names_duplicates_reserved_variables_and_jig_ambiguity() {
        let cases: &[(&[u8], &str)] = &[
            (b"A=$HOME\n", "interpolation"),
            (b"A='${HOME}'\n", "interpolation"),
            (b"A=\"$(command)\"\n", "interpolation"),
            (b"A=`command`\n", "substitution"),
            (b"1BAD=value\n", "invalid environment variable name"),
            (b"A=one\nA=two\n", "duplicates variable 'A'"),
            (
                b"JIG_VAULT_PASSPHRASE=value\n",
                "may not assign reserved variable",
            ),
            (
                b"JIG_VAULT_NEW_PASSPHRASE=value\n",
                "may not assign reserved variable",
            ),
            (b"A=jig://OnlyItem\n", "canonical jig://ITEM/FIELD"),
            (b"A=prefix-jig://Prod/Field\n", "Jig-looking value"),
            (b"A=JIG://Prod/Field\n", "Jig-looking value"),
        ];

        for (input, expected) in cases {
            let error = parse_vault_env_bytes(input).unwrap_err().to_string();
            assert!(error.contains(expected), "unexpected error: {error}");
        }
    }

    #[test]
    fn rejects_malformed_lines_quotes_escapes_controls_utf8_and_nul_without_values() {
        let secret = "do-not-leak-this-value";
        let cases = [
            format!("A={secret} trailing\n").into_bytes(),
            format!("A=\"{secret}\"trailing\n").into_bytes(),
            format!("A='{secret}\n").into_bytes(),
            format!("A={secret}\\q\n").into_bytes(),
            format!("NO_ASSIGNMENT_{secret}\n").into_bytes(),
            format!("A={secret}\0\n").into_bytes(),
            vec![b'A', b'=', 0xff, b'\n'],
        ];

        for input in cases {
            let error = parse_vault_env_bytes(&input).unwrap_err().to_string();
            assert!(!error.contains(secret), "value leaked in error: {error}");
            assert!(error.contains("line 1"), "missing line number: {error}");
        }
    }

    #[test]
    fn duplicate_matching_uses_the_platform_environment_rule() {
        let result = parse_vault_env_bytes(b"Name=one\nNAME=two\n");
        assert_eq!(result.is_err(), cfg!(windows));
    }

    #[test]
    fn enforces_file_assignment_and_total_decoded_bounds() {
        let oversized = vec![b'#'; MAX_VAULT_ENV_FILE_LEN + 1];
        assert!(parse_vault_env_bytes(&oversized).is_err());

        let mut too_many = Vec::new();
        for index in 0..=MAX_VAULT_ENV_ASSIGNMENTS {
            too_many.extend_from_slice(format!("V{index}=x\n").as_bytes());
        }
        assert!(parse_vault_env_bytes(&too_many).is_err());

        let value = "x".repeat(MAX_VAULT_ENV_TOTAL_DECODED_LEN);
        let input = format!("A={value}\n");
        assert!(parse_vault_env_bytes(input.as_bytes()).is_err());
    }

    #[test]
    fn rejects_stdin_marker_before_opening_a_file() {
        let error = parse_vault_env_file(Path::new("-"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("inherit stdin"));
    }
}
