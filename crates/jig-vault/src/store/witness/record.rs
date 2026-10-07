//! Per-ID witness records: the committed checkpoint and at most one durable
//! pending transaction marker.

use anyhow::{Result as AnyResult, bail};
use serde::{Deserialize, Serialize};

pub(super) fn read_error(path: &std::path::Path, error: anyhow::Error) -> anyhow::Error {
    crate::error::classify_source(
        crate::VaultErrorKind::AuditTampered,
        format!(
            "cannot verify vault witness record at {path:?}; this can block other vaults on this user profile. Operator step: preserve the vault and witness data and investigate the reported record; never delete or edit the rollback witness to bypass this refusal. Agents must ask the operator."
        ),
        error,
    )
}

pub(crate) const RECORD_SCHEMA: u32 = 1;
/// Records exist only for vault IDs that have been witnessed as format 3.
pub(crate) const WITNESSED_MIN_FORMAT: u32 = 3;
const MAX_VAULT_ID_BYTES: usize = 128;

/// The operation a pending transaction completes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TransactionKind {
    Init,
    Edit,
    Migrate,
    PassphraseChange,
    Restore,
}

impl TransactionKind {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Init => "initialization",
            Self::Edit => "edit",
            Self::Migrate => "format migration",
            Self::PassphraseChange => "passphrase change",
            Self::Restore => "backup restore",
        }
    }
}

/// One committed (or intended) vault state: its generation, the SHA-256 of
/// the exact persisted encrypted envelope bytes, and its mutation audit MAC.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Checkpoint {
    pub(crate) generation: u64,
    pub(crate) envelope_sha256: String,
    pub(crate) mutation_audit_mac: String,
}

/// The irrevocable commit point of one transaction. It names the
/// target-keyed journal by digest and the state that completion produces.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PendingMarker {
    pub(crate) operation: TransactionKind,
    pub(crate) target_key: String,
    pub(crate) journal_sha256: String,
    pub(crate) next: Checkpoint,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WitnessRecord {
    pub(crate) schema: u32,
    pub(crate) vault_id: String,
    pub(crate) min_format: u32,
    pub(crate) committed: Option<Checkpoint>,
    pub(crate) pending: Option<PendingMarker>,
}

impl WitnessRecord {
    pub(crate) fn new(vault_id: &str) -> Self {
        Self {
            schema: RECORD_SCHEMA,
            vault_id: vault_id.to_owned(),
            min_format: WITNESSED_MIN_FORMAT,
            committed: None,
            pending: None,
        }
    }

    /// Validates a record read for `expected_vault_id`. Any inconsistency
    /// fails closed; a record is never reinterpreted as absent.
    pub(crate) fn validate(&self, expected_vault_id: &str) -> AnyResult<()> {
        if self.schema != RECORD_SCHEMA {
            bail!("unsupported vault witness record schema");
        }
        if self.vault_id != expected_vault_id {
            bail!("vault witness record does not belong to this vault");
        }
        if self.min_format != WITNESSED_MIN_FORMAT {
            bail!("vault witness record has an unsupported minimum format");
        }
        match (&self.committed, &self.pending) {
            (None, None) => {
                bail!("vault witness record has neither a checkpoint nor a pending transaction")
            }
            (Some(committed), Some(pending)) => {
                committed.validate()?;
                pending.validate()?;
                if pending.next.generation <= committed.generation {
                    bail!("vault witness pending generation does not advance its checkpoint");
                }
            }
            (Some(committed), None) => committed.validate()?,
            (None, Some(pending)) => pending.validate()?,
        }
        Ok(())
    }
}

impl Checkpoint {
    pub(crate) fn validate(&self) -> AnyResult<()> {
        if self.generation == 0 {
            bail!("vault witness checkpoint generation must be at least 1");
        }
        if !is_hex_digest(&self.envelope_sha256) || !is_hex_digest(&self.mutation_audit_mac) {
            bail!("vault witness checkpoint digest is malformed");
        }
        Ok(())
    }
}

impl PendingMarker {
    fn validate(&self) -> AnyResult<()> {
        self.next.validate()?;
        if !is_hex_digest(&self.target_key) || !is_hex_digest(&self.journal_sha256) {
            bail!("vault witness pending marker is malformed");
        }
        Ok(())
    }
}

/// Lowercase hex of a 32-byte digest or MAC.
pub(crate) fn is_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(crate) fn validate_vault_id(vault_id: &str) -> AnyResult<()> {
    if vault_id.is_empty()
        || vault_id.len() > MAX_VAULT_ID_BYTES
        || !vault_id.bytes().all(|byte| byte.is_ascii_graphic())
    {
        bail!("vault ID is outside the bounds the witness accepts");
    }
    Ok(())
}
