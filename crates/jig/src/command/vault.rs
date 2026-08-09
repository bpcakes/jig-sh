//! Vault command DTOs.

use std::ffi::OsString;
use std::path::PathBuf;

use jig_vault::{InjectionTemplate, SecretBytes, VaultItem, VaultReference};

#[derive(Debug)]
pub(crate) enum VaultCommand {
    Audit(VaultAuditCommand),
    Init(VaultInitRequest),
    Status(VaultStatusRequest),
    Migrate(VaultMigrateRequest),
    Exec(VaultExecRequest),
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
pub(crate) struct VaultMigrateRequest {
    pub(crate) target_version: u32,
    pub(crate) vault: VaultRuntimeOptions,
}

#[derive(Debug)]
pub(crate) struct VaultAuditVerifyRequest {
    pub(crate) vault: VaultRuntimeOptions,
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
        VaultRuntimeOptions, is_valid_vault_scope_id,
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
}
