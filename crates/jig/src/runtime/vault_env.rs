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
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use anyhow::{Context, Result, bail};
use jig_vault::{MAX_SECRET_VALUE_LEN, SecretBytes, VaultReference};
use zeroize::Zeroizing;

use crate::command::{
    VaultExecAssignment, VaultExecEnvironment, VaultExecValue, VaultImportAssignment,
    VaultImportEnvironment, VaultImportValueSource,
};

pub(crate) const MAX_VAULT_ENV_FILE_LEN: usize = 1024 * 1024;
pub(crate) const MAX_VAULT_ENV_ASSIGNMENTS: usize = 1024;
pub(crate) const MAX_VAULT_ENV_TOTAL_DECODED_LEN: usize = 1024 * 1024;

const PASSPHRASE_ENV: &str = "JIG_VAULT_PASSPHRASE";
const NEW_PASSPHRASE_ENV: &str = "JIG_VAULT_NEW_PASSPHRASE";

#[derive(Clone, Copy)]
enum EnvOperation {
    Exec,
    Import,
}

impl EnvOperation {
    const fn label(self) -> &'static str {
        match self {
            Self::Exec => "vault exec env",
            Self::Import => "vault import env",
        }
    }
}

pub(crate) fn parse_vault_env_file(path: &Path) -> Result<VaultExecEnvironment> {
    parse_vault_env_file_for(path, EnvOperation::Exec)
}

fn parse_vault_env_file_for(path: &Path, operation: EnvOperation) -> Result<VaultExecEnvironment> {
    parse_vault_env_file_for_with_cancellation(path, operation, &|| false)
}

fn parse_vault_env_file_for_with_cancellation(
    path: &Path,
    operation: EnvOperation,
    cancelled: &dyn Fn() -> bool,
) -> Result<VaultExecEnvironment> {
    if path == Path::new("-") {
        match operation {
            EnvOperation::Exec => {
                bail!("vault exec rejects --env-file - so the child can inherit stdin")
            }
            EnvOperation::Import => bail!("vault import env rejects --env-file -"),
        }
    }
    let bytes = read_bounded_file(path, operation, cancelled)?;
    parse_vault_env_bytes_for(bytes.as_slice(), operation)
}

pub(crate) fn parse_onepassword_env_file(
    path: &Path,
    item: &jig_vault::VaultItem,
) -> Result<VaultImportEnvironment> {
    parse_onepassword_env_file_with_cancellation(path, item, &|| false)
}

pub(crate) fn parse_onepassword_env_file_with_cancellation(
    path: &Path,
    item: &jig_vault::VaultItem,
    cancelled: &dyn Fn() -> bool,
) -> Result<VaultImportEnvironment> {
    let environment =
        parse_vault_env_file_for_with_cancellation(path, EnvOperation::Import, cancelled)?;
    if environment.assignments.is_empty() {
        bail!("vault import env must contain at least one assignment");
    }
    let mut assignments = Vec::with_capacity(environment.assignments.len());
    for assignment in environment.assignments {
        let reference = VaultReference::parse(&format!(
            "jig://{}/{}",
            item.as_str(),
            assignment.name
        ))
        .map_err(|_| {
            anyhow::anyhow!(
                "vault import env line {} variable '{}': destination field reference is invalid",
                assignment.line,
                assignment.name
            )
        })?;
        let source = match assignment.value {
            VaultExecValue::Field(_) => {
                bail!(
                    "vault import env line {} variable '{}': jig:// references are not import sources",
                    assignment.line,
                    assignment.name
                )
            }
            VaultExecValue::Literal(value) => {
                classify_onepassword_value(value, assignment.line, &assignment.name)?
            }
        };
        assignments.push(VaultImportAssignment {
            line: assignment.line,
            name: assignment.name,
            reference,
            source,
        });
    }
    Ok(VaultImportEnvironment { assignments })
}

#[cfg(test)]
pub(crate) fn parse_vault_env_bytes(input: &[u8]) -> Result<VaultExecEnvironment> {
    parse_vault_env_bytes_for(input, EnvOperation::Exec)
}

fn parse_vault_env_bytes_for(
    input: &[u8],
    operation: EnvOperation,
) -> Result<VaultExecEnvironment> {
    let label = operation.label();
    if input.len() > MAX_VAULT_ENV_FILE_LEN {
        bail!("{label} file exceeds the {MAX_VAULT_ENV_FILE_LEN} byte limit");
    }
    if let Some(offset) = input.iter().position(|byte| *byte == 0) {
        bail!(
            "{label} line {} contains a NUL byte",
            line_number_at(input, offset)
        );
    }
    let text = std::str::from_utf8(input).map_err(|error| {
        anyhow::anyhow!(
            "{label} line {} is not valid UTF-8",
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
            bail!("{label} line {line_number} contains an unsupported control character");
        }

        let Some((name, raw_value)) = line.split_once('=') else {
            bail!("{label} line {line_number} must be an exact NAME=VALUE assignment");
        };
        if !valid_env_name(name) {
            bail!("{label} line {line_number} has an invalid environment variable name");
        }
        if reserved_env_name(name) {
            bail!("{label} line {line_number} may not assign reserved variable '{name}'");
        }
        let comparison_name = comparable_env_name(name);
        if !names.insert(comparison_name) {
            bail!("{label} line {line_number} duplicates variable '{name}'");
        }
        if assignments.len() >= MAX_VAULT_ENV_ASSIGNMENTS {
            bail!(
                "{label} line {line_number} exceeds the {MAX_VAULT_ENV_ASSIGNMENTS} assignment limit"
            );
        }

        let decoded = decode_value(raw_value.as_bytes()).map_err(|reason| {
            anyhow::anyhow!("{label} line {line_number} variable '{name}': {reason}")
        })?;
        if decoded.len() > MAX_SECRET_VALUE_LEN {
            bail!(
                "{label} line {line_number} variable '{name}' exceeds the {MAX_SECRET_VALUE_LEN} byte value limit"
            );
        }
        total_decoded_len = total_decoded_len
            .checked_add(name.len())
            .and_then(|total| total.checked_add(decoded.len()))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{label} line {line_number} variable '{name}' exceeds the total decoded data limit"
                )
            })?;
        if total_decoded_len > MAX_VAULT_ENV_TOTAL_DECODED_LEN {
            bail!(
                "{label} line {line_number} variable '{name}' exceeds the {MAX_VAULT_ENV_TOTAL_DECODED_LEN} byte total decoded data limit"
            );
        }

        let value = classify_value(decoded).map_err(|reason| {
            anyhow::anyhow!("{label} line {line_number} variable '{name}': {reason}")
        })?;
        assignments.push(VaultExecAssignment {
            line: line_number,
            name: name.to_owned(),
            value,
        });
    }

    Ok(VaultExecEnvironment { assignments })
}

fn read_bounded_file(
    path: &Path,
    operation: EnvOperation,
    cancelled: &dyn Fn() -> bool,
) -> Result<SecretBytes> {
    let label = operation.label();
    ensure_env_read_active(label, cancelled)?;
    let mut file = open_regular_env_file(path, label)?;
    let capacity = MAX_VAULT_ENV_FILE_LEN
        .checked_add(1)
        .expect("vault env file limit leaves room for an overflow byte");
    let mut bytes = SecretBytes::with_capacity(capacity);
    let mut chunk = Zeroizing::new([0_u8; 8 * 1024]);
    loop {
        ensure_env_read_active(label, cancelled)?;
        let remaining = capacity - bytes.len();
        let chunk_len = remaining.min(chunk.len());
        let read = match file.read(&mut chunk[..chunk_len]) {
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to read {label} file {}", path.display()));
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
                "{label} file {} exceeds the {MAX_VAULT_ENV_FILE_LEN} byte limit",
                path.display()
            );
        }
    }
    Ok(bytes)
}

fn open_regular_env_file(path: &Path, label: &str) -> Result<File> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            bail!(
                "{label} file {} must not be a symbolic link",
                path.display()
            )
        }
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect {label} file {}", path.display()));
        }
    }

    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = options
        .open(path)
        .with_context(|| format!("failed to open {label} file {}", path.display()))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("failed to inspect opened {label} file {}", path.display()))?;
    if !metadata.is_file() {
        bail!("{label} file {} must be a regular file", path.display());
    }
    Ok(file)
}

fn ensure_env_read_active(label: &str, cancelled: &dyn Fn() -> bool) -> Result<()> {
    if cancelled() {
        bail!("{label} file read was cancelled");
    }
    Ok(())
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

fn classify_onepassword_value(
    value: SecretBytes,
    line: usize,
    name: &str,
) -> Result<VaultImportValueSource> {
    let bytes = value.as_slice();
    if bytes.starts_with(b"op://") {
        let reference =
            std::str::from_utf8(bytes).expect("restricted dotenv input was validated as UTF-8");
        let path = &reference[5..];
        let segment_count = path.split('/').count();
        let segments_valid =
            matches!(segment_count, 3 | 4) && path.split('/').all(|segment| !segment.is_empty());
        if !segments_valid {
            bail!(
                "vault import env line {line} variable '{name}': 1Password reference must be exact op://VAULT/ITEM/FIELD or op://VAULT/ITEM/SECTION/FIELD"
            );
        }
        return Ok(VaultImportValueSource::OnePassword(value));
    }
    if bytes
        .get(..3)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"op:"))
    {
        bail!(
            "vault import env line {line} variable '{name}': 1Password-looking value must use an exact op:// reference"
        );
    }
    Ok(VaultImportValueSource::Literal(value))
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
    name.to_owned()
}

fn env_names_equal(left: &str, right: &str) -> bool {
    left == right
}

fn line_number_at(input: &[u8], offset: usize) -> usize {
    1 + input[..offset.min(input.len())]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::{
        ffi::CString,
        fs::OpenOptions,
        os::{unix::ffi::OsStrExt, unix::fs::OpenOptionsExt},
    };

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
    fn duplicate_matching_is_case_sensitive() {
        let result = parse_vault_env_bytes(b"Name=one\nNAME=two\n");
        assert!(result.is_ok());
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

    #[cfg(unix)]
    #[test]
    fn env_file_rejects_symlinks_and_nonblocking_fifos() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.env");
        let link = temp.path().join("source-link.env");
        std::fs::write(&source, b"MODE=production\n").unwrap();
        std::os::unix::fs::symlink(&source, &link).unwrap();

        let link_error = parse_vault_env_file(&link).unwrap_err().to_string();
        assert!(
            link_error.contains("must not be a symbolic link"),
            "{link_error}"
        );

        let fifo = temp.path().join("source.fifo");
        let fifo_path = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: `fifo_path` is a live NUL-terminated string and the mode
        // contains only ordinary permission bits.
        assert_eq!(unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) }, 0);
        // Keep both peers open so this regression test remains bounded even if
        // the subject accidentally drops O_NONBLOCK in the future.
        let _reader = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&fifo)
            .unwrap();
        let _writer = OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&fifo)
            .unwrap();

        let fifo_error = parse_vault_env_file(&fifo).unwrap_err().to_string();
        assert!(
            fifo_error.contains("must be a regular file"),
            "{fifo_error}"
        );
    }

    #[test]
    fn cancelled_env_read_stops_before_opening_the_path() {
        let error = parse_onepassword_env_file_with_cancellation(
            Path::new("missing.env"),
            &jig_vault::VaultItem::parse("jig://Production").unwrap(),
            &|| true,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("cancelled"), "{error}");
        assert!(!error.contains("failed to open"), "{error}");
    }

    #[test]
    fn onepassword_import_classifies_exact_references_and_keeps_interior_op_text_literal() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.env");
        std::fs::write(
            &source,
            b"TOKEN=op://Team/Login/password\nSECTION='op://Team/Login/credentials/password'\nTEXT=stop:value\n",
        )
        .unwrap();
        let item = jig_vault::VaultItem::parse("jig://Production").unwrap();

        let parsed = parse_onepassword_env_file(&source, &item).unwrap();

        assert_eq!(parsed.assignments.len(), 3);
        assert_eq!(
            parsed.assignments[0].reference.to_string(),
            "jig://Production/TOKEN"
        );
        assert!(matches!(
            &parsed.assignments[0].source,
            VaultImportValueSource::OnePassword(_)
        ));
        assert!(matches!(
            &parsed.assignments[1].source,
            VaultImportValueSource::OnePassword(_)
        ));
        match &parsed.assignments[2].source {
            VaultImportValueSource::Literal(value) => assert_eq!(value.as_slice(), b"stop:value"),
            VaultImportValueSource::OnePassword(_) => panic!("expected literal"),
        }
    }

    #[test]
    fn onepassword_import_rejects_empty_and_malformed_sources_with_import_diagnostics() {
        let temp = tempfile::tempdir().unwrap();
        let item = jig_vault::VaultItem::parse("jig://Production").unwrap();
        let cases: &[(&[u8], &str)] = &[
            (b"", "at least one assignment"),
            (b"TOKEN=op://Team/Login\n", "op://VAULT/ITEM/FIELD"),
            (b"TOKEN=op://Team//password\n", "op://VAULT/ITEM/FIELD"),
            (b"TOKEN=OP://Team/Login/password\n", "exact op://"),
            (b"TOKEN=jig://Production/TOKEN\n", "not import sources"),
        ];

        for (index, (contents, expected)) in cases.iter().enumerate() {
            let source = temp.path().join(format!("invalid-{index}.env"));
            std::fs::write(&source, contents).unwrap();
            let error = parse_onepassword_env_file(&source, &item)
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("vault import env"),
                "unexpected error: {error}"
            );
            assert!(error.contains(expected), "unexpected error: {error}");
            assert!(
                !error.contains("vault exec env"),
                "unexpected error: {error}"
            );
        }

        let error = parse_onepassword_env_file(Path::new("-"), &item)
            .unwrap_err()
            .to_string();
        assert!(error.contains("vault import env"));
    }
}
