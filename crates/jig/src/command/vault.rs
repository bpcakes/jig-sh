//! Vault command DTOs.

use std::ffi::OsString;
use std::path::PathBuf;

use jig_vault::{
    BackupCreateRequest, BackupRestoreRequest, InjectionTemplate, SecretBytes, VaultItem,
    VaultReference,
};

#[derive(Debug)]
pub(crate) enum VaultCommand {
    Audit(VaultAuditCommand),
    Backup(VaultBackupCommand),
    Init(VaultInitRequest),
    Status(VaultStatusRequest),
    Tui(VaultTuiRequest),
    Migrate(VaultMigrateRequest),
    Passphrase(VaultPassphraseCommand),
    Exec(VaultExecRequest),
    Import(VaultImportCommand),
    Field(VaultFieldCommand),
    Inject(VaultInjectRequest),
    Read(VaultReadRequest),
    Secret(VaultSecretCommand),
    Run(VaultRunRequest),
}

#[derive(Debug)]
pub(crate) enum VaultAuditCommand {
    Verify(VaultAuditVerifyRequest),
}

#[derive(Debug)]
pub(crate) enum VaultBackupCommand {
    Create(Box<VaultBackupCreateRequest>),
    Restore(Box<VaultBackupRestoreRequest>),
}

#[derive(Debug)]
pub(crate) enum VaultPassphraseCommand {
    Change(VaultPassphraseChangeRequest),
}

#[derive(Debug)]
pub(crate) enum VaultSecretCommand {
    List(VaultSecretListRequest),
    Set(VaultSecretSetRequest),
    Remove(VaultSecretRemoveRequest),
}

#[derive(Debug)]
pub(crate) enum VaultFieldCommand {
    List(VaultFieldListRequest),
    Set(VaultFieldSetRequest),
    Remove(VaultFieldRemoveRequest),
}

#[derive(Debug)]
pub(crate) enum VaultImportCommand {
    OnePassword(VaultImportOnePasswordRequest),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct VaultRuntimeOptions {
    pub(crate) home: Option<PathBuf>,
    pub(crate) scope: VaultScopeSelection,
}

impl VaultRuntimeOptions {
    pub(crate) fn repo(
        scope_id: impl Into<String>,
        repo_name: impl Into<String>,
        repo_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            home: None,
            scope: VaultScopeSelection::Repo(VaultRepoScope {
                scope_id: scope_id.into(),
                repo_name: repo_name.into(),
                repo_root: repo_root.into(),
            }),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) enum VaultScopeSelection {
    #[default]
    Auto,
    Repo(VaultRepoScope),
    Global,
}

#[derive(Clone, Debug)]
pub(crate) struct VaultRepoScope {
    pub(crate) scope_id: String,
    pub(crate) repo_name: String,
    pub(crate) repo_root: PathBuf,
}

pub(crate) fn is_valid_vault_scope_id(scope_id: &str) -> bool {
    !scope_id.is_empty()
        && scope_id.len() <= 128
        && scope_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[derive(Debug)]
pub(crate) struct VaultInitRequest {
    pub(crate) vault: VaultRuntimeOptions,
}

#[derive(Debug)]
pub(crate) struct VaultStatusRequest {
    pub(crate) vault: VaultRuntimeOptions,
}

#[derive(Debug)]
pub(crate) struct VaultTuiRequest {
    pub(crate) vault: VaultRuntimeOptions,
}

#[derive(Debug)]
pub(crate) struct VaultMigrateRequest {
    pub(crate) target_version: u32,
    pub(crate) vault: VaultRuntimeOptions,
}

#[derive(Debug)]
pub(crate) struct VaultAuditVerifyRequest {
    pub(crate) vault: VaultRuntimeOptions,
}

pub(crate) struct VaultBackupCreateRequest {
    pub(crate) output: PathBuf,
    pub(crate) overwrite: bool,
    pub(crate) prepared: Option<BackupCreateRequest>,
    pub(crate) vault: VaultRuntimeOptions,
}

pub(crate) struct VaultBackupRestoreRequest {
    pub(crate) input: PathBuf,
    pub(crate) prepared: Option<BackupRestoreRequest>,
    pub(crate) vault: VaultRuntimeOptions,
}

#[derive(Debug)]
pub(crate) struct VaultPassphraseChangeRequest {
    pub(crate) vault: VaultRuntimeOptions,
}

impl std::fmt::Debug for VaultBackupCreateRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VaultBackupCreateRequest")
            .field("output", &self.output)
            .field("overwrite", &self.overwrite)
            .field("prepared", &self.prepared.as_ref().map(|_| "[REDACTED]"))
            .field("vault", &self.vault)
            .finish()
    }
}

impl std::fmt::Debug for VaultBackupRestoreRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VaultBackupRestoreRequest")
            .field("input", &self.input)
            .field("prepared", &self.prepared.as_ref().map(|_| "[REDACTED]"))
            .field("vault", &self.vault)
            .finish()
    }
}

#[derive(Debug)]
pub(crate) struct VaultSecretListRequest {
    pub(crate) vault: VaultRuntimeOptions,
}

#[derive(Debug)]
pub(crate) struct VaultFieldListRequest {
    pub(crate) item: Option<VaultItem>,
    pub(crate) vault: VaultRuntimeOptions,
}

#[derive(Debug)]
pub(crate) struct VaultSecretSetRequest {
    pub(crate) name: String,
    pub(crate) value_source: VaultSecretValueSource,
    pub(crate) vault: VaultRuntimeOptions,
}

#[derive(Debug)]
pub(crate) struct VaultFieldSetRequest {
    pub(crate) reference: VaultReference,
    pub(crate) text: bool,
    pub(crate) value_source: VaultSecretValueSource,
    pub(crate) vault: VaultRuntimeOptions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VaultSecretValueSource {
    Auto,
    Stdin,
    Prompt,
}

#[derive(Debug)]
pub(crate) struct VaultSecretRemoveRequest {
    pub(crate) name: String,
    pub(crate) vault: VaultRuntimeOptions,
}

#[derive(Debug)]
pub(crate) struct VaultFieldRemoveRequest {
    pub(crate) reference: VaultReference,
    pub(crate) vault: VaultRuntimeOptions,
}

#[derive(Debug)]
pub(crate) struct VaultReadRequest {
    pub(crate) reference: VaultReference,
    pub(crate) reveal: bool,
    pub(crate) out_file: Option<PathBuf>,
    pub(crate) overwrite: bool,
    pub(crate) vault: VaultRuntimeOptions,
}

#[derive(Debug)]
pub(crate) struct VaultInjectRequest {
    pub(crate) input: PathBuf,
    pub(crate) template: Option<InjectionTemplate>,
    pub(crate) reveal: bool,
    pub(crate) out_file: Option<PathBuf>,
    pub(crate) overwrite: bool,
    pub(crate) vault: VaultRuntimeOptions,
}

pub(crate) struct VaultExecRequest {
    pub(crate) env_file: PathBuf,
    pub(crate) environment: Option<VaultExecEnvironment>,
    pub(crate) command: Vec<OsString>,
    pub(crate) vault: VaultRuntimeOptions,
}

pub(crate) struct VaultImportOnePasswordRequest {
    pub(crate) env_file: PathBuf,
    pub(crate) environment: Option<VaultImportEnvironment>,
    pub(crate) destination_exists: Option<bool>,
    pub(crate) item: VaultItem,
    pub(crate) out_env: PathBuf,
    pub(crate) replace: bool,
    pub(crate) overwrite: bool,
    pub(crate) dry_run: bool,
    pub(crate) vault: VaultRuntimeOptions,
}

pub(crate) struct VaultImportEnvironment {
    pub(crate) assignments: Vec<VaultImportAssignment>,
}

pub(crate) struct VaultImportAssignment {
    pub(crate) line: usize,
    pub(crate) name: String,
    pub(crate) reference: VaultReference,
    pub(crate) source: VaultImportValueSource,
}

pub(crate) enum VaultImportValueSource {
    Literal(SecretBytes),
    OnePassword(SecretBytes),
}

impl std::fmt::Debug for VaultImportOnePasswordRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VaultImportOnePasswordRequest")
            .field("env_file", &self.env_file)
            .field("environment", &self.environment)
            .field("destination_exists", &self.destination_exists)
            .field("item", &self.item)
            .field("out_env", &self.out_env)
            .field("replace", &self.replace)
            .field("overwrite", &self.overwrite)
            .field("dry_run", &self.dry_run)
            .field("vault", &self.vault)
            .finish()
    }
}

impl std::fmt::Debug for VaultImportEnvironment {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VaultImportEnvironment")
            .field("assignments", &self.assignments)
            .finish()
    }
}

impl std::fmt::Debug for VaultImportAssignment {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VaultImportAssignment")
            .field("line", &self.line)
            .field("name", &self.name)
            .field("reference", &self.reference)
            .field("source", &self.source)
            .finish()
    }
}

impl std::fmt::Debug for VaultImportValueSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (kind, value) = match self {
            Self::Literal(value) => ("literal", value),
            Self::OnePassword(value) => ("onepassword", value),
        };
        formatter
            .debug_struct("VaultImportValueSource")
            .field("kind", &kind)
            .field("len", &value.len())
            .field("value", &"[REDACTED]")
            .finish()
    }
}

pub(crate) struct VaultExecEnvironment {
    pub(crate) assignments: Vec<VaultExecAssignment>,
}

pub(crate) struct VaultExecAssignment {
    pub(crate) line: usize,
    pub(crate) name: String,
    pub(crate) value: VaultExecValue,
}

pub(crate) enum VaultExecValue {
    Literal(SecretBytes),
    Field(VaultReference),
}

impl std::fmt::Debug for VaultExecRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VaultExecRequest")
            .field("env_file", &self.env_file)
            .field("environment", &self.environment)
            .field("command", &"[REDACTED]")
            .field("command_len", &self.command.len())
            .field("vault", &self.vault)
            .finish()
    }
}

impl std::fmt::Debug for VaultExecEnvironment {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VaultExecEnvironment")
            .field("assignments", &self.assignments)
            .finish()
    }
}

impl std::fmt::Debug for VaultExecAssignment {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VaultExecAssignment")
            .field("line", &self.line)
            .field("name", &self.name)
            .field("value", &self.value)
            .finish()
    }
}

impl std::fmt::Debug for VaultExecValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Literal(value) => formatter
                .debug_struct("Literal")
                .field("len", &value.len())
                .field("value", &"[REDACTED]")
                .finish(),
            Self::Field(reference) => formatter.debug_tuple("Field").field(reference).finish(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct VaultRunRequest {
    pub(crate) env: Vec<String>,
    pub(crate) files: Vec<String>,
    pub(crate) command: Vec<String>,
    pub(crate) vault: VaultRuntimeOptions,
}

#[cfg(test)]
mod tests {
    use super::{
        VaultExecAssignment, VaultExecEnvironment, VaultExecRequest, VaultExecValue,
        VaultImportAssignment, VaultImportEnvironment, VaultImportOnePasswordRequest,
        VaultImportValueSource, VaultRuntimeOptions, is_valid_vault_scope_id,
    };

    #[test]
    fn vault_scope_id_validator_rejects_path_and_length_boundaries() {
        assert!(is_valid_vault_scope_id("abc_123-XYZ"));
        assert!(!is_valid_vault_scope_id(""));
        assert!(!is_valid_vault_scope_id("../shared"));
        assert!(!is_valid_vault_scope_id("scope/child"));
        assert!(!is_valid_vault_scope_id(&"a".repeat(129)));
    }

    #[test]
    fn exec_request_debug_redacts_literals_and_argv() {
        let request = VaultExecRequest {
            env_file: ".env.jig".into(),
            environment: Some(VaultExecEnvironment {
                assignments: vec![VaultExecAssignment {
                    line: 1,
                    name: "TOKEN".to_owned(),
                    value: VaultExecValue::Literal(jig_vault::SecretBytes::new(
                        b"literal-do-not-print".to_vec(),
                    )),
                }],
            }),
            command: vec![std::ffi::OsString::from("argv-do-not-print")],
            vault: VaultRuntimeOptions::default(),
        };

        let debug = format!("{request:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("literal-do-not-print"));
        assert!(!debug.contains("argv-do-not-print"));
    }

    #[test]
    fn import_request_debug_redacts_literal_and_onepassword_values() {
        let reference = jig_vault::VaultReference::parse("jig://Production/TOKEN").unwrap();
        let request = VaultImportOnePasswordRequest {
            env_file: ".env.op".into(),
            environment: Some(VaultImportEnvironment {
                assignments: vec![
                    VaultImportAssignment {
                        line: 1,
                        name: "TOKEN".to_owned(),
                        reference,
                        source: VaultImportValueSource::OnePassword(jig_vault::SecretBytes::new(
                            b"op://debug-do-not-print/item/field".to_vec(),
                        )),
                    },
                    VaultImportAssignment {
                        line: 2,
                        name: "MODE".to_owned(),
                        reference: jig_vault::VaultReference::parse("jig://Production/MODE")
                            .unwrap(),
                        source: VaultImportValueSource::Literal(jig_vault::SecretBytes::new(
                            b"literal-do-not-print".to_vec(),
                        )),
                    },
                ],
            }),
            destination_exists: Some(false),
            item: jig_vault::VaultItem::parse("jig://Production").unwrap(),
            out_env: ".env.jig".into(),
            replace: false,
            overwrite: false,
            dry_run: false,
            vault: VaultRuntimeOptions::default(),
        };

        let debug = format!("{request:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("op://debug-do-not-print"));
        assert!(!debug.contains("literal-do-not-print"));
    }
}
