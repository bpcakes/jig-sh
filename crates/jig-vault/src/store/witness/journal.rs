//! Target-keyed transaction journals.
//!
//! A journal is written and synced before the ID's pending marker, so the
//! marker can name it by digest. It binds everything recovery must check:
//! the intended target, the exact predecessor and successor, and either the
//! candidate envelope plus the exact audit transition (in-place operations)
//! or an owned durable staging directory (absent-target restore). It never
//! contains plaintext secrets, audit keys, or passphrases.

use anyhow::{Result as AnyResult, bail};
use serde::{Deserialize, Serialize};

use super::record::{Checkpoint, TransactionKind, is_hex_digest, validate_vault_id};

pub(crate) const JOURNAL_SCHEMA: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Journal {
    pub(crate) schema: u32,
    pub(crate) operation: TransactionKind,
    pub(crate) vault_id: String,
    pub(crate) target: JournalTarget,
    /// The witness checkpoint the transaction advances, if any.
    pub(crate) previous: Option<Checkpoint>,
    /// SHA-256 of the exact live envelope bytes the transaction replaces;
    /// `None` when no envelope exists yet at the target.
    pub(crate) previous_envelope_sha256: Option<String>,
    pub(crate) next: Checkpoint,
    pub(crate) payload: JournalPayload,
}

/// The intended final target. For an existing home this is the home itself;
/// for restore it is the absent final path, never temporary staging.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JournalTarget {
    pub(crate) target_key: String,
    pub(crate) parent_device: u64,
    pub(crate) parent_inode: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum JournalPayload {
    InPlace(InPlacePayload),
    Restore(RestorePayload),
}

/// Exact successor of an in-place transaction.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InPlacePayload {
    pub(crate) candidate_envelope: String,
    pub(crate) audit: AuditTransition,
}

/// The exact audit change: the verified prefix to keep, any recoverable torn
/// suffix that preceded the transaction, and the exact bytes to append.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AuditTransition {
    pub(crate) prefix_len: u64,
    pub(crate) prefix_tip_mac: Option<String>,
    pub(crate) torn_suffix_len: u64,
    pub(crate) torn_suffix_sha256: Option<String>,
    pub(crate) append: String,
}

/// Owned durable staging holding the complete restored home.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RestorePayload {
    pub(crate) archive_sha256: String,
    pub(crate) staging_leaf: String,
    pub(crate) staging_device: u64,
    pub(crate) staging_inode: u64,
    pub(crate) audit_sha256: String,
}

impl Journal {
    pub(crate) fn validate(&self) -> AnyResult<()> {
        if self.schema != JOURNAL_SCHEMA {
            bail!("unsupported vault transaction journal schema");
        }
        validate_vault_id(&self.vault_id)?;
        if !is_hex_digest(&self.target.target_key) {
            bail!("vault transaction journal target is malformed");
        }
        self.next.validate()?;
        if let Some(previous) = &self.previous {
            previous.validate()?;
            if self.next.generation <= previous.generation {
                bail!("vault transaction journal does not advance its predecessor");
            }
        }
        if self
            .previous_envelope_sha256
            .as_deref()
            .is_some_and(|digest| !is_hex_digest(digest))
        {
            bail!("vault transaction journal predecessor digest is malformed");
        }
        match &self.payload {
            JournalPayload::InPlace(payload) => {
                if self.operation == TransactionKind::Restore {
                    bail!("vault restore journal must reference durable staging");
                }
                payload.audit.validate()
            }
            JournalPayload::Restore(payload) => {
                if self.operation != TransactionKind::Restore {
                    bail!("only a restore journal may reference staging");
                }
                payload.validate()
            }
        }
    }
}

impl AuditTransition {
    fn validate(&self) -> AnyResult<()> {
        let digests_valid = self.prefix_tip_mac.as_deref().is_none_or(is_hex_digest)
            && self.torn_suffix_sha256.as_deref().is_none_or(is_hex_digest);
        if !digests_valid
            || (self.torn_suffix_len == 0) != self.torn_suffix_sha256.is_none()
            || !self.append.ends_with('\n')
            || (self.prefix_len == 0) != self.prefix_tip_mac.is_none()
        {
            bail!("vault transaction journal audit transition is malformed");
        }
        Ok(())
    }
}

impl RestorePayload {
    fn validate(&self) -> AnyResult<()> {
        if !is_hex_digest(&self.archive_sha256)
            || !is_hex_digest(&self.audit_sha256)
            || !is_safe_leaf(&self.staging_leaf)
        {
            bail!("vault restore journal staging reference is malformed");
        }
        Ok(())
    }
}

/// A single owned staging directory name beside the target, never a path.
fn is_safe_leaf(leaf: &str) -> bool {
    !leaf.is_empty()
        && leaf.len() <= 255
        && leaf != "."
        && leaf != ".."
        && leaf
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}
