mod aad;
mod acl;
mod audit;
mod backup;
mod broker;
mod crypto;
mod env_policy;
mod error;
mod exec;
mod exec_output;
mod exec_process;
mod format;
mod output;
mod passphrase_policy;
mod path_security;
mod redact;
mod run;
mod secret;
mod store;
mod template;
#[cfg(test)]
mod test_fixtures;
mod types;
mod vault;

pub use audit::{
    AuditVerification, MAX_VAULT_ACTIVITY_RECORDS, VaultActivityRecord, VerifiedVaultActivity,
};
pub use backup::{
    BACKUP_FORMAT_VERSION, BackupCreateRequest, BackupCreateResult, BackupRestoreRequest,
    BackupRestoreResult, MAX_BACKUP_ARCHIVE_BYTES,
};
pub use broker::{BrokeredEnv, BrokeredFile, BrokeredRun};
pub use error::{Result, VaultError, VaultErrorKind};
pub use exec::{
    ExecEnvBinding, ExecOutcome, MAX_EXEC_ARGUMENT_BYTES, MAX_EXEC_ARGUMENTS,
    MAX_EXEC_ENV_BINDINGS, MAX_EXEC_ENV_TOTAL_BYTES, MAX_EXEC_ENV_VALUE_LEN,
    VAULT_NEW_PASSPHRASE_ENV, VAULT_PASSPHRASE_ENV, VaultExec, is_vault_passphrase_env,
};
pub use output::{PreparedPrivateFile, PrivateFilePrecondition};
pub use passphrase_policy::{
    MIN_MASTER_PASSPHRASE_GUESSES, MIN_MASTER_PASSPHRASE_LEN, NEW_VAULT_PASSPHRASE_POLICY,
    validate_new_vault_passphrase, validate_new_vault_passphrase_bytes,
};
pub use redact::Redactor;
pub use run::RunOutput;
pub use secret::{SecretBytes, SecretBytesCapacityError};
pub use template::{InjectionTemplate, MAX_TEMPLATE_INPUT_LEN, MAX_TEMPLATE_OUTPUT_LEN};
pub use types::{EnvVarName, FieldKind, SecretName, VaultItem, VaultReference};
pub use vault::{
    FieldBatchResult, FieldKindChangeResult, FieldMutation, FieldRecord, LegacyConversionResult,
    MAX_SECRET_VALUE_LEN, RevealResult, SecretRecord, Vault, VaultHomeState,
    VaultImportPrecondition, VaultMigration, VaultMutation, VaultRevision, VaultSnapshot,
    VaultStatus, VaultWriteMode,
};

/// Crash injection for tests of the witnessed transaction protocol. Only
/// this crate's tests and `test-utils` consumers can arm a crash point; it
/// applies to the arming thread's next matching protocol step.
#[cfg(any(test, feature = "test-utils"))]
#[doc(hidden)]
pub mod test_support {
    pub use crate::store::FaultPoint as TransactionFaultPoint;

    pub fn arm_transaction_fault(point: TransactionFaultPoint) {
        crate::store::arm_fault_for_test(point);
    }
}

/// Envelope format created by initialization and the newest explicit
/// migration target.
pub const LATEST_VAULT_FORMAT_VERSION: u32 = format::LATEST_FORMAT_VERSION;
