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
mod redact;
mod run;
mod secret;
mod store;
mod template;
mod types;
mod vault;

pub use audit::AuditVerification;
pub use backup::{
    BACKUP_FORMAT_VERSION, BackupCreateRequest, BackupCreateResult, BackupRestoreRequest,
    BackupRestoreResult, MAX_BACKUP_ARCHIVE_BYTES,
};
pub use broker::{BrokeredEnv, BrokeredFile, BrokeredRun};
pub use error::{Result, VaultError, VaultErrorKind};
pub use exec::{
    ExecEnvBinding, ExecOutcome, MAX_EXEC_ARGUMENT_BYTES, MAX_EXEC_ARGUMENTS,
    MAX_EXEC_ENV_BINDINGS, MAX_EXEC_ENV_TOTAL_BYTES, MAX_EXEC_ENV_VALUE_LEN, VaultExec,
};
pub use output::PreparedPrivateFile;
pub use redact::Redactor;
pub use run::RunOutput;
pub use secret::{SecretBytes, SecretBytesCapacityError};
pub use template::{InjectionTemplate, MAX_TEMPLATE_INPUT_LEN, MAX_TEMPLATE_OUTPUT_LEN};
pub use types::{EnvVarName, FieldKind, SecretName, VaultItem, VaultReference};
pub use vault::{
    FieldBatchResult, FieldMutation, FieldRecord, MAX_SECRET_VALUE_LEN, MIN_MASTER_PASSPHRASE_LEN,
    RevealResult, SecretRecord, Vault, VaultMigration, VaultStatus, validate_new_vault_passphrase,
};
