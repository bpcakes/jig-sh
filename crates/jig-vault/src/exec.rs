use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fmt;

use zeroize::Zeroizing;

use crate::exec_output::{
    MAX_EXEC_REDACTION_PATTERN_BYTES, MAX_EXEC_REDACTION_PATTERN_LEN, MAX_EXEC_REDACTION_PATTERNS,
    StreamingRedactor,
};
use crate::{EnvVarName, Result, SecretBytes, VaultError, VaultErrorKind, VaultReference};

pub const MAX_EXEC_ARGUMENTS: usize = 4_096;
pub const MAX_EXEC_ARGUMENT_BYTES: usize = 1024 * 1024;
pub const MAX_EXEC_ENV_BINDINGS: usize = 1_024;
pub const MAX_EXEC_ENV_VALUE_LEN: usize = 1024 * 1024;
pub const MAX_EXEC_ENV_TOTAL_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_EXEC_CONCEALED_VALUE_LEN: usize = 64 * 1024;
pub(crate) const MAX_EXEC_CONCEALED_BINDINGS: usize = 128;

pub(crate) const VAULT_PASSPHRASE_ENV: &str = "JIG_VAULT_PASSPHRASE";
pub(crate) const VAULT_NEW_PASSPHRASE_ENV: &str = "JIG_VAULT_NEW_PASSPHRASE";

/// One environment assignment for a transparent vault execution.
///
/// Literal values remain zeroizing bytes until process preparation. Field
/// bindings carry only a validated project-local reference. Debug output never
/// includes a literal value.
pub struct ExecEnvBinding {
    var: EnvVarName,
    value: ExecEnvValue,
}

pub(crate) enum ExecEnvValue {
    Literal(SecretBytes),
    Field(VaultReference),
}

impl ExecEnvBinding {
    /// Creates a validated literal environment assignment.
    ///
    /// # Errors
    ///
    /// Returns an invalid-input error when the value is oversized, is not
    /// UTF-8, or contains NUL. Error text contains the variable name but never
    /// the value.
    pub fn literal(var: EnvVarName, value: SecretBytes) -> Result<Self> {
        validate_literal_value(&var, &value)?;
        Ok(Self {
            var,
            value: ExecEnvValue::Literal(value),
        })
    }

    /// Creates a field-backed environment assignment.
    pub fn field(var: EnvVarName, reference: VaultReference) -> Self {
        Self {
            var,
            value: ExecEnvValue::Field(reference),
        }
    }

    pub(crate) fn var(&self) -> &EnvVarName {
        &self.var
    }

    pub(crate) fn literal_len(&self) -> usize {
        match &self.value {
            ExecEnvValue::Literal(value) => value.len(),
            ExecEnvValue::Field(_) => 0,
        }
    }

    pub(crate) fn value(&self) -> &ExecEnvValue {
        &self.value
    }

    pub(crate) fn into_parts(self) -> (EnvVarName, ExecEnvValue) {
        (self.var, self.value)
    }
}

impl fmt::Debug for ExecEnvBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("ExecEnvBinding");
        debug.field("var", &self.var);
        match &self.value {
            ExecEnvValue::Literal(value) => debug
                .field("kind", &"literal")
                .field("value_len", &value.len())
                .field("value", &"[REDACTED]"),
            ExecEnvValue::Field(reference) => {
                debug.field("kind", &"field").field("reference", reference)
            }
        };
        debug.finish()
    }
}

/// A bounded request for transparent command execution with vault-aware
/// environment assignments.
///
/// The command inherits the caller's ordinary environment and stdin when it is
/// executed. Bindings override inherited variables. Debug output hides all
/// argv because arguments may contain sensitive operator input even though
/// command-line secrets are discouraged.
pub struct VaultExec {
    command: Vec<OsString>,
    bindings: Vec<ExecEnvBinding>,
}

impl VaultExec {
    /// Creates and validates a transparent execution request without opening a
    /// vault.
    ///
    /// # Errors
    ///
    /// Returns an invalid-input error for an empty or oversized command,
    /// duplicate environment variables, too many or oversized literal
    /// assignments, or assignment to either Jig vault passphrase variable.
    pub fn new(command: Vec<OsString>, bindings: Vec<ExecEnvBinding>) -> Result<Self> {
        validate_command(&command)?;
        validate_bindings(&bindings)?;
        Ok(Self { command, bindings })
    }

    pub(crate) fn command_len(&self) -> usize {
        self.command.len()
    }

    pub(crate) fn bindings(&self) -> &[ExecEnvBinding] {
        &self.bindings
    }

    pub(crate) fn into_parts(self) -> (Vec<OsString>, Vec<ExecEnvBinding>) {
        (self.command, self.bindings)
    }
}

impl fmt::Debug for VaultExec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VaultExec")
            .field("argument_count", &self.command.len())
            .field("arguments", &"[REDACTED]")
            .field("bindings", &self.bindings)
            .finish()
    }
}

/// Portable child outcome from transparent vault execution.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecOutcome {
    /// The child's exit code, or the conventional `128 + signal` code on Unix.
    pub exit_status: i32,
    /// The terminating Unix signal when applicable.
    pub exit_signal: Option<i32>,
}

fn validate_literal_value(var: &EnvVarName, value: &SecretBytes) -> Result<()> {
    if value.len() > MAX_EXEC_ENV_VALUE_LEN {
        return Err(invalid_input(format!(
            "literal environment value for {} exceeds the {MAX_EXEC_ENV_VALUE_LEN} byte limit",
            var.as_str()
        )));
    }
    if std::str::from_utf8(value.as_slice()).is_err() {
        return Err(invalid_input(format!(
            "literal environment value for {} must be valid UTF-8",
            var.as_str()
        )));
    }
    if value.as_slice().contains(&0) {
        return Err(invalid_input(format!(
            "literal environment value for {} must not contain NUL",
            var.as_str()
        )));
    }
    Ok(())
}

fn validate_command(command: &[OsString]) -> Result<()> {
    if command.is_empty() || command[0].is_empty() {
        return Err(invalid_input(
            "vault exec requires a nonempty command after --",
        ));
    }
    if command.len() > MAX_EXEC_ARGUMENTS {
        return Err(invalid_input(format!(
            "vault exec command has more than {MAX_EXEC_ARGUMENTS} arguments"
        )));
    }
    let mut total_bytes = 0_usize;
    for argument in command {
        let bytes = argument.as_encoded_bytes();
        if bytes.contains(&0) {
            return Err(invalid_input(
                "vault exec command arguments must not contain NUL",
            ));
        }
        total_bytes = total_bytes.checked_add(bytes.len()).ok_or_else(|| {
            invalid_input("vault exec command argument length exceeds supported bounds")
        })?;
        if total_bytes > MAX_EXEC_ARGUMENT_BYTES {
            return Err(invalid_input(format!(
                "vault exec command arguments exceed the {MAX_EXEC_ARGUMENT_BYTES} byte total limit"
            )));
        }
    }
    Ok(())
}

fn validate_bindings(bindings: &[ExecEnvBinding]) -> Result<()> {
    if bindings.len() > MAX_EXEC_ENV_BINDINGS {
        return Err(invalid_input(format!(
            "vault exec has more than {MAX_EXEC_ENV_BINDINGS} environment bindings"
        )));
    }

    let mut names = BTreeSet::new();
    let mut literal_bytes = 0_usize;
    for binding in bindings {
        let name = binding.var().as_str();
        if is_vault_passphrase_env(name) {
            return Err(invalid_input(format!(
                "vault exec must not assign reserved environment variable {name}"
            )));
        }
        let comparison_name = comparable_env_name(name);
        if !names.insert(comparison_name) {
            return Err(invalid_input(format!(
                "vault exec contains duplicate environment variable {name}"
            )));
        }
        literal_bytes = literal_bytes
            .checked_add(binding.literal_len())
            .ok_or_else(|| invalid_input("vault exec literal environment data is too large"))?;
        if literal_bytes > MAX_EXEC_ENV_TOTAL_BYTES {
            return Err(invalid_input(format!(
                "vault exec literal environment data exceeds the {MAX_EXEC_ENV_TOTAL_BYTES} byte total limit"
            )));
        }
    }
    Ok(())
}

pub(crate) fn is_vault_passphrase_env(name: &str) -> bool {
    env_names_equal(name, VAULT_PASSPHRASE_ENV) || env_names_equal(name, VAULT_NEW_PASSPHRASE_ENV)
}

fn comparable_env_name(name: &str) -> String {
    name.to_owned()
}

pub(crate) fn env_names_equal(left: &str, right: &str) -> bool {
    left == right
}

fn invalid_input(message: impl Into<String>) -> VaultError {
    VaultError::new(VaultErrorKind::InvalidInput, message)
}

pub(crate) fn redactor_from_concealed_values(values: &[&[u8]]) -> Result<StreamingRedactor> {
    if values.len() > MAX_EXEC_CONCEALED_BINDINGS {
        return Err(invalid_input(format!(
            "vault exec has more than {MAX_EXEC_CONCEALED_BINDINGS} concealed field bindings"
        )));
    }

    let mut patterns: Vec<Zeroizing<Vec<u8>>> = Vec::new();
    let mut total_bytes = 0_usize;
    for value in values {
        if value.len() > MAX_EXEC_CONCEALED_VALUE_LEN {
            return Err(invalid_input(format!(
                "concealed vault exec field exceeds the {MAX_EXEC_CONCEALED_VALUE_LEN} byte redaction input limit"
            )));
        }
        let redactor = crate::redact::Redactor::from_secret_slices([*value]);
        redactor.append_streaming_patterns(
            &mut patterns,
            &mut total_bytes,
            MAX_EXEC_REDACTION_PATTERNS,
            MAX_EXEC_REDACTION_PATTERN_BYTES,
            MAX_EXEC_REDACTION_PATTERN_LEN,
        )?;
    }
    StreamingRedactor::new(patterns)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(name: &str) -> EnvVarName {
        EnvVarName::parse(name).unwrap()
    }

    #[test]
    fn literal_validation_is_value_free_and_pre_vault() {
        let invalid_utf8 =
            ExecEnvBinding::literal(var("TOKEN"), SecretBytes::new(vec![b's', b'e', b'c', 0xff]))
                .unwrap_err();
        assert_eq!(invalid_utf8.kind(), VaultErrorKind::InvalidInput);
        assert!(invalid_utf8.to_string().contains("TOKEN"));
        assert!(!invalid_utf8.to_string().contains("sec"));

        let nul =
            ExecEnvBinding::literal(var("TOKEN"), SecretBytes::new(b"hidden\0value".to_vec()))
                .unwrap_err();
        assert!(nul.to_string().contains("NUL"));
        assert!(!nul.to_string().contains("hidden"));

        let oversized = ExecEnvBinding::literal(
            var("TOKEN"),
            SecretBytes::new(vec![b'x'; MAX_EXEC_ENV_VALUE_LEN + 1]),
        )
        .unwrap_err();
        assert!(oversized.to_string().contains("byte limit"));
    }

    #[test]
    fn request_rejects_empty_oversized_duplicate_and_reserved_inputs() {
        assert!(VaultExec::new(Vec::new(), Vec::new()).is_err());
        assert!(VaultExec::new(vec![OsString::new()], Vec::new()).is_err());
        assert!(
            VaultExec::new(
                vec![OsString::from("cmd"); MAX_EXEC_ARGUMENTS + 1],
                Vec::new()
            )
            .is_err()
        );
        assert!(
            VaultExec::new(
                vec![OsString::from("x".repeat(MAX_EXEC_ARGUMENT_BYTES + 1))],
                Vec::new()
            )
            .is_err()
        );

        let first =
            ExecEnvBinding::literal(var("TOKEN"), SecretBytes::new(b"one".to_vec())).unwrap();
        let second = ExecEnvBinding::field(
            var("TOKEN"),
            VaultReference::parse("jig://Production/TOKEN").unwrap(),
        );
        assert!(VaultExec::new(vec![OsString::from("cmd")], vec![first, second]).is_err());

        for reserved in [VAULT_PASSPHRASE_ENV, VAULT_NEW_PASSPHRASE_ENV] {
            let binding = ExecEnvBinding::field(
                var(reserved),
                VaultReference::parse("jig://Production/TOKEN").unwrap(),
            );
            assert!(VaultExec::new(vec![OsString::from("cmd")], vec![binding]).is_err());
        }
    }

    #[test]
    fn debug_hides_literal_values_and_every_argument() {
        let binding = ExecEnvBinding::literal(
            var("TOKEN"),
            SecretBytes::new(b"debug-secret-sentinel".to_vec()),
        )
        .unwrap();
        let binding_debug = format!("{binding:?}");
        assert!(binding_debug.contains("[REDACTED]"));
        assert!(!binding_debug.contains("debug-secret-sentinel"));

        let request = VaultExec::new(
            vec![
                OsString::from("secret-program-sentinel"),
                OsString::from("secret-argument-sentinel"),
            ],
            vec![binding],
        )
        .unwrap();
        let request_debug = format!("{request:?}");
        assert!(request_debug.contains("[REDACTED]"));
        assert!(!request_debug.contains("secret-program-sentinel"));
        assert!(!request_debug.contains("secret-argument-sentinel"));
        assert!(!request_debug.contains("debug-secret-sentinel"));
    }

    #[test]
    fn environment_name_comparison_is_case_sensitive() {
        let lower =
            ExecEnvBinding::literal(var("token"), SecretBytes::new(b"one".to_vec())).unwrap();
        let upper =
            ExecEnvBinding::literal(var("TOKEN"), SecretBytes::new(b"two".to_vec())).unwrap();
        assert!(VaultExec::new(vec![OsString::from("cmd")], vec![lower, upper]).is_ok());
    }

    #[test]
    fn concealed_values_build_raw_and_encoded_streaming_patterns() {
        let values = [SecretBytes::new(b"secret-value".to_vec())];
        let concealed = [values[0].as_slice()];
        let mut redactor = redactor_from_concealed_values(&concealed).unwrap();
        let mut output = Vec::new();
        redactor
            .push_chunk(
                b"raw=secret-value b64=c2VjcmV0LXZhbHVl hex=7365637265742d76616c7565",
                &mut output,
            )
            .unwrap();
        redactor.finish(&mut output).unwrap();
        assert_eq!(output, b"raw=[REDACTED] b64=[REDACTED] hex=[REDACTED]");
    }

    #[test]
    fn concealed_pattern_generation_rejects_oversized_values_before_building() {
        let values = [SecretBytes::new(vec![
            b'x';
            MAX_EXEC_CONCEALED_VALUE_LEN + 1
        ])];
        let concealed = [values[0].as_slice()];
        let error = redactor_from_concealed_values(&concealed).unwrap_err();
        assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
        assert!(error.to_string().contains("redaction input limit"));
    }
}
