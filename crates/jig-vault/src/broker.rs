use crate::Result;
use crate::env_policy::{env_var_names_equal, is_preserved_env_var_name};
use crate::types::{EnvVarName, SecretName, VaultReference};

#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct BrokeredEnv {
    var: EnvVarName,
    secret_name: SecretName,
    reference: Option<VaultReference>,
}

impl BrokeredEnv {
    /// Creates a secret-to-environment mapping.
    ///
    /// # Errors
    ///
    /// Returns an error when the destination is a preserved environment
    /// variable that brokered secret injection must not replace.
    pub fn new(var: EnvVarName, secret_name: SecretName) -> Result<Self> {
        if is_preserved_env_var_name(var.as_str()) {
            return Err(crate::VaultError::new(
                crate::VaultErrorKind::InvalidInput,
                format!(
                    "vault env mapping cannot inject a secret into preserved environment variable '{}'",
                    var.as_str()
                ),
            ));
        }
        Ok(Self {
            var,
            secret_name,
            reference: None,
        })
    }

    /// Parses a `VAR=SECRET_NAME` or `VAR=jig://ITEM/FIELD` environment
    /// mapping. A canonical reference resolves the same field key as the
    /// legacy `ITEM/FIELD` secret name.
    ///
    /// # Errors
    ///
    /// Returns an error when the mapping shape, environment variable, secret
    /// name, or vault reference is invalid, or the destination environment
    /// variable is preserved.
    pub fn parse(value: &str) -> Result<Self> {
        let (var, source) = value.split_once('=').ok_or_else(|| {
            crate::VaultError::new(
                crate::VaultErrorKind::InvalidInput,
                format!(
                    "vault env mapping '{value}' must have the form VAR=SECRET_NAME or VAR=jig://ITEM/FIELD"
                ),
            )
        })?;
        if var.is_empty() || source.is_empty() {
            return Err(crate::VaultError::new(
                crate::VaultErrorKind::InvalidInput,
                format!("vault env mapping '{value}' must have non-empty VAR and SECRET_NAME"),
            ));
        }
        let (secret_name, reference) = parse_mapping_source("env", value, source)?;
        let mut mapping = Self::new(EnvVarName::parse(var)?, secret_name)?;
        mapping.reference = reference;
        Ok(mapping)
    }

    pub const fn var(&self) -> &EnvVarName {
        &self.var
    }

    pub const fn secret_name(&self) -> &SecretName {
        &self.secret_name
    }

    pub(crate) fn into_parts(self) -> (EnvVarName, SecretName, Option<VaultReference>) {
        (self.var, self.secret_name, self.reference)
    }
}

#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct BrokeredFile {
    var: EnvVarName,
    secret_name: SecretName,
    reference: Option<VaultReference>,
}

impl BrokeredFile {
    /// Creates a secret-to-temporary-file environment mapping.
    ///
    /// # Errors
    ///
    /// Returns an error on unsupported platforms or when the destination is a
    /// preserved environment variable.
    pub fn new(var: EnvVarName, secret_name: SecretName) -> Result<Self> {
        #[cfg(not(unix))]
        {
            Err(crate::VaultError::new(
                crate::VaultErrorKind::InvalidInput,
                format!(
                    "vault file mapping '{}={}' requires Unix-style owner-only temporary files; use --env on this platform",
                    var.as_str(),
                    secret_name.as_str()
                ),
            ))
        }

        #[cfg(unix)]
        {
            if is_preserved_env_var_name(var.as_str()) {
                return Err(crate::VaultError::new(
                    crate::VaultErrorKind::InvalidInput,
                    format!(
                        "vault file mapping cannot inject a path into preserved environment variable '{}'",
                        var.as_str()
                    ),
                ));
            }
            Ok(Self {
                var,
                secret_name,
                reference: None,
            })
        }
    }

    /// Parses a `VAR=SECRET_NAME` or `VAR=jig://ITEM/FIELD` temporary-file
    /// mapping. A canonical reference resolves the same field key as the
    /// legacy `ITEM/FIELD` secret name.
    ///
    /// # Errors
    ///
    /// Returns an error when the mapping shape, environment variable, secret
    /// name, or vault reference is invalid, the destination is preserved, or
    /// the platform cannot provide owner-only temporary secret files.
    pub fn parse(value: &str) -> Result<Self> {
        let (var, source) = value.split_once('=').ok_or_else(|| {
            crate::VaultError::new(
                crate::VaultErrorKind::InvalidInput,
                format!(
                    "vault file mapping '{value}' must have the form VAR=SECRET_NAME or VAR=jig://ITEM/FIELD"
                ),
            )
        })?;
        if var.is_empty() {
            return Err(crate::VaultError::new(
                crate::VaultErrorKind::InvalidInput,
                format!("vault file mapping '{value}' must have a non-empty VAR"),
            ));
        }
        if source.is_empty() {
            return Err(crate::VaultError::new(
                crate::VaultErrorKind::InvalidInput,
                format!("vault file mapping '{value}' must have a non-empty SECRET_NAME"),
            ));
        }
        let (secret_name, reference) = parse_mapping_source("file", value, source)?;
        let mut mapping = Self::new(EnvVarName::parse(var)?, secret_name)?;
        mapping.reference = reference;
        Ok(mapping)
    }

    pub const fn var(&self) -> &EnvVarName {
        &self.var
    }

    pub const fn secret_name(&self) -> &SecretName {
        &self.secret_name
    }

    pub(crate) fn into_parts(self) -> (EnvVarName, SecretName, Option<VaultReference>) {
        (self.var, self.secret_name, self.reference)
    }
}

/// Parses the source half of a brokered mapping.
///
/// Legacy secret names cannot contain ':', so a case-insensitive `jig:` prefix
/// unambiguously selects strict canonical reference parsing; `SecretName`
/// itself stays strict. The reference maps through
/// [`VaultReference::to_secret_name`] to the same key in version 1 and 2
/// vaults, so resolution, scope selection, and audit details are unchanged.
fn parse_mapping_source(
    mapping_kind: &str,
    mapping: &str,
    source: &str,
) -> Result<(SecretName, Option<VaultReference>)> {
    let is_reference = source
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("jig:"));
    if !is_reference {
        return Ok((SecretName::parse(source)?, None));
    }
    let reference = VaultReference::parse(source).map_err(|error| {
        crate::VaultError::new(
            crate::VaultErrorKind::InvalidInput,
            format!(
                "vault {mapping_kind} mapping '{mapping}' has an invalid vault reference: {error}; use VAR=jig://ITEM/FIELD or a legacy VAR=SECRET_NAME"
            ),
        )
    })?;
    Ok((reference.to_secret_name(), Some(reference)))
}

#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct BrokeredRun {
    command: Vec<String>,
    env: Vec<BrokeredEnv>,
    files: Vec<BrokeredFile>,
}

impl BrokeredRun {
    /// Creates a brokered command with environment mappings.
    ///
    /// # Errors
    ///
    /// Returns an error when the command is empty or mappings target duplicate
    /// or conflicting environment variables.
    pub fn new(command: Vec<String>, env: Vec<BrokeredEnv>) -> Result<Self> {
        Self::with_files(command, env, Vec::new())
    }

    /// Creates a brokered command with environment and temporary-file mappings.
    ///
    /// # Errors
    ///
    /// Returns an error when the command is empty, either mapping set contains
    /// duplicate destinations, or environment and file mappings conflict.
    pub fn with_files(
        command: Vec<String>,
        env: Vec<BrokeredEnv>,
        files: Vec<BrokeredFile>,
    ) -> Result<Self> {
        if command.is_empty() {
            return Err(crate::VaultError::new(
                crate::VaultErrorKind::InvalidInput,
                "vault run requires a command after --",
            ));
        }
        for (index, mapping) in env.iter().enumerate() {
            if env[..index]
                .iter()
                .any(|prior| same_env_var(prior, mapping))
            {
                return Err(crate::VaultError::new(
                    crate::VaultErrorKind::InvalidInput,
                    format!(
                        "vault env mapping specifies environment variable '{}' more than once",
                        mapping.var().as_str()
                    ),
                ));
            }
        }
        for (index, mapping) in files.iter().enumerate() {
            if files[..index]
                .iter()
                .any(|prior| same_file_var(prior, mapping))
            {
                return Err(crate::VaultError::new(
                    crate::VaultErrorKind::InvalidInput,
                    format!(
                        "vault file mapping specifies environment variable '{}' more than once",
                        mapping.var().as_str()
                    ),
                ));
            }
            if env
                .iter()
                .any(|prior| env_var_names_equal(prior.var().as_str(), mapping.var().as_str()))
            {
                return Err(crate::VaultError::new(
                    crate::VaultErrorKind::InvalidInput,
                    format!(
                        "vault mapping specifies environment variable '{}' as both an env and file mapping",
                        mapping.var().as_str()
                    ),
                ));
            }
        }
        Ok(Self {
            command,
            env,
            files,
        })
    }

    pub fn command(&self) -> &[String] {
        &self.command
    }

    pub fn env(&self) -> &[BrokeredEnv] {
        &self.env
    }

    pub fn files(&self) -> &[BrokeredFile] {
        &self.files
    }

    pub(crate) fn into_parts(self) -> (Vec<String>, Vec<BrokeredEnv>, Vec<BrokeredFile>) {
        (self.command, self.env, self.files)
    }
}

fn same_env_var(left: &BrokeredEnv, right: &BrokeredEnv) -> bool {
    env_var_names_equal(left.var().as_str(), right.var().as_str())
}

fn same_file_var(left: &BrokeredFile, right: &BrokeredFile) -> bool {
    env_var_names_equal(left.var().as_str(), right.var().as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::SecretString;

    use crate::VaultErrorKind;
    use crate::store::VaultStore;

    #[test]
    fn brokered_env_rejects_equals_in_secret_name() {
        let error = BrokeredEnv::parse("TOKEN=bad=name")
            .unwrap_err()
            .to_string();
        assert!(error.contains("unsupported characters"));
    }

    #[test]
    fn brokered_env_accepts_canonical_reference_as_field_key() {
        let mapping = BrokeredEnv::parse("TOKEN=jig://Production/TOKEN").unwrap();

        assert_eq!(mapping.var().as_str(), "TOKEN");
        assert_eq!(mapping.secret_name().as_str(), "Production/TOKEN");
        let (_, _, reference) = mapping.into_parts();
        assert_eq!(reference.unwrap().to_string(), "jig://Production/TOKEN");
    }

    #[cfg(unix)]
    #[test]
    fn brokered_file_accepts_canonical_reference_as_field_key() {
        let mapping = BrokeredFile::parse("TOKEN_FILE=jig://Production/TOKEN").unwrap();

        assert_eq!(mapping.var().as_str(), "TOKEN_FILE");
        assert_eq!(mapping.secret_name().as_str(), "Production/TOKEN");
        let (_, _, reference) = mapping.into_parts();
        assert_eq!(reference.unwrap().to_string(), "jig://Production/TOKEN");
    }

    #[test]
    fn brokered_mappings_keep_legacy_names() {
        for (value, name) in [
            ("TOKEN=api_token", "api_token"),
            ("TOKEN=Production/TOKEN", "Production/TOKEN"),
        ] {
            let mapping = BrokeredEnv::parse(value).unwrap();
            assert_eq!(mapping.secret_name().as_str(), name);
            let (_, _, reference) = mapping.into_parts();
            assert!(reference.is_none());
        }
        let error = BrokeredEnv::parse("TOKEN=bad name")
            .unwrap_err()
            .to_string();
        assert!(error.contains("unsupported characters"));
    }

    #[test]
    fn brokered_mappings_reject_noncanonical_references() {
        for source in [
            "jig://Production",
            "JIG://Production/TOKEN",
            "jig://Production/TOKEN?x",
            "jig://ExampleProject/Production/TOKEN",
            "jig:Production/TOKEN",
        ] {
            let mut errors = vec![BrokeredEnv::parse(&format!("TOKEN={source}")).unwrap_err()];
            #[cfg(unix)]
            errors.push(BrokeredFile::parse(&format!("TOKEN_FILE={source}")).unwrap_err());
            for error in errors {
                assert_eq!(error.kind(), VaultErrorKind::InvalidInput, "{source}");
                let message = error.to_string();
                assert!(message.contains("invalid vault reference"), "{message}");
                assert!(message.contains("VAR=jig://ITEM/FIELD"), "{message}");
                assert!(!message.contains("unsupported characters;"), "{message}");
            }
        }
    }

    #[test]
    fn brokered_mapping_shape_errors_name_both_source_forms() {
        let env = BrokeredEnv::parse("TOKEN").unwrap_err().to_string();
        assert!(env.contains("VAR=SECRET_NAME or VAR=jig://ITEM/FIELD"));
        #[cfg(unix)]
        {
            let file = BrokeredFile::parse("TOKEN_FILE").unwrap_err().to_string();
            assert!(file.contains("VAR=SECRET_NAME or VAR=jig://ITEM/FIELD"));
        }
    }

    #[test]
    fn brokered_env_rejects_preserved_environment_names() {
        let error = BrokeredEnv::parse("PATH=api_token")
            .unwrap_err()
            .to_string();
        assert!(error.contains("preserved environment variable"));
    }

    #[test]
    fn brokered_run_rejects_duplicate_env_names() {
        let error = BrokeredRun::new(
            vec!["true".into()],
            vec![
                BrokeredEnv::parse("TOKEN=api_token").unwrap(),
                BrokeredEnv::parse("TOKEN=other_token").unwrap(),
            ],
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("more than once"));
    }

    #[cfg(unix)]
    #[test]
    fn brokered_run_rejects_duplicate_file_and_env_names() {
        let error = BrokeredRun::with_files(
            vec!["true".into()],
            vec![BrokeredEnv::parse("TOKEN=api_token").unwrap()],
            vec![BrokeredFile::parse("TOKEN=api_token_file").unwrap()],
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("TOKEN"));
        assert!(error.contains("both an env and file mapping"));
    }

    #[cfg(unix)]
    #[test]
    fn brokered_file_rejects_preserved_environment_names() {
        let error = BrokeredFile::parse("PATH=api_token")
            .unwrap_err()
            .to_string();
        assert!(error.contains("preserved environment variable"));
    }

    #[cfg(not(unix))]
    #[test]
    fn brokered_file_rejects_file_mappings_on_non_unix() {
        let error = BrokeredFile::parse("TOKEN_FILE=api_token").unwrap_err();
        assert_eq!(error.kind(), VaultErrorKind::InvalidInput);
        assert!(
            error
                .to_string()
                .contains("requires Unix-style owner-only temporary files")
        );
    }

    #[test]
    fn brokered_file_reports_empty_mapping_side() {
        let missing_var = BrokeredFile::parse("=api_token").unwrap_err().to_string();
        assert!(missing_var.contains("non-empty VAR"));

        let missing_secret = BrokeredFile::parse("TOKEN_FILE=").unwrap_err().to_string();
        assert!(missing_secret.contains("non-empty SECRET_NAME"));
    }

    #[test]
    fn brokered_run_resolution_failure_records_failure_audit_event() {
        let temp = tempfile::tempdir().unwrap();
        let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
        let passphrase = SecretString::from("correct horse battery staple".to_string());
        store.init(&passphrase).unwrap();

        let error = store
            .run_brokered(
                &passphrase,
                BrokeredRun::new(
                    vec!["true".into()],
                    vec![
                        BrokeredEnv::new(
                            EnvVarName::parse("TOKEN").unwrap(),
                            SecretName::parse("missing_token").unwrap(),
                        )
                        .unwrap(),
                    ],
                )
                .unwrap(),
            )
            .unwrap_err();

        assert_eq!(error.kind(), VaultErrorKind::NotFound);
        let audit = store.read_audit_text().unwrap().unwrap();
        let events = audit
            .lines()
            .map(serde_json::from_str::<serde_json::Value>)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(events[1]["action"], "brokered_run_start");
        assert_eq!(events[2]["action"], "brokered_run_failed");
        assert_eq!(
            events[1]["details"]["run_id"],
            events[2]["details"]["run_id"]
        );
        assert_eq!(events[2]["details"]["stage"], "resolve");
        let verification = store.verify_audit(&passphrase).unwrap();
        assert_eq!(verification.event_count, 3);
    }

    #[test]
    fn brokered_run_process_failure_records_failure_audit_event() {
        let temp = tempfile::tempdir().unwrap();
        let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
        let passphrase = SecretString::from("correct horse battery staple".to_string());
        store.init(&passphrase).unwrap();

        let error = store
            .run_brokered(
                &passphrase,
                BrokeredRun::new(
                    vec!["jig-vault-command-that-does-not-exist".into()],
                    Vec::new(),
                )
                .unwrap(),
            )
            .unwrap_err();

        assert_eq!(error.kind(), VaultErrorKind::Process);
        let audit = store.read_audit_text().unwrap().unwrap();
        let events = audit
            .lines()
            .map(serde_json::from_str::<serde_json::Value>)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(events[1]["action"], "brokered_run_start");
        assert_eq!(events[2]["action"], "brokered_run_failed");
        assert_eq!(
            events[1]["details"]["run_id"],
            events[2]["details"]["run_id"]
        );
        assert_eq!(events[2]["details"]["stage"], "process");
        let verification = store.verify_audit(&passphrase).unwrap();
        assert_eq!(verification.event_count, 3);
    }

    #[cfg(unix)]
    #[test]
    fn concurrent_brokered_runs_keep_a_valid_audit_chain() {
        use std::sync::{Arc, Barrier};

        let temp = tempfile::tempdir().unwrap();
        let store = VaultStore::resolve_for_test(Some(temp.path().join("vault"))).unwrap();
        let passphrase = SecretString::from("correct horse battery staple".to_string());
        store.init(&passphrase).unwrap();
        store
            .set_secret(
                &passphrase,
                "api_token",
                crate::SecretBytes::new(b"secret-value".to_vec()),
            )
            .unwrap();

        let store = Arc::new(store);
        let barrier = Arc::new(Barrier::new(2));
        let mut handles = Vec::new();
        for _ in 0..2 {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                store
                    .run_brokered(
                        &SecretString::from("correct horse battery staple".to_string()),
                        BrokeredRun::new(
                            vec![
                                "sh".into(),
                                "-c".into(),
                                "sleep 0.05; printf '%s' \"$TOKEN\"".into(),
                            ],
                            vec![
                                BrokeredEnv::new(
                                    EnvVarName::parse("TOKEN").unwrap(),
                                    SecretName::parse("api_token").unwrap(),
                                )
                                .unwrap(),
                            ],
                        )
                        .unwrap(),
                    )
                    .unwrap()
            }));
        }

        for handle in handles {
            let output = handle.join().unwrap();
            assert_eq!(output.exit_status, 0);
            assert_eq!(output.stdout, "[REDACTED]");
        }
        let verification = store.verify_audit(&passphrase).unwrap();
        assert_eq!(verification.event_count, 6);
        assert_eq!(verification.torn_tail_bytes, 0);
    }
}
