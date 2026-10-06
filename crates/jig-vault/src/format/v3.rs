//! Version 3 encrypted-state security fields.

use std::collections::BTreeMap;
use std::fmt;
use std::io::Write;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

use crate::crypto::{KEY_LEN, random_array};

use super::SecretEntry;

/// Lowercase hex length of an HMAC-SHA-256 audit event MAC.
const AUDIT_MAC_HEX_LEN: usize = 64;

/// Independent audit authentication root of a version 3 vault.
///
/// The root is the audit HMAC key. It stays stable across migration and
/// passphrase rotation so historical and in-flight audit records remain
/// verifiable. It never appears in `Debug` output.
pub(crate) struct AuditRoot(Zeroizing<[u8; KEY_LEN]>);

impl AuditRoot {
    /// A fresh root for a newly initialized vault.
    pub(crate) fn random() -> Result<Self> {
        Ok(Self(Zeroizing::new(random_array::<KEY_LEN>()?)))
    }

    /// Seeds a migrated vault's root with its legacy derived audit key so
    /// every existing audit MAC still verifies.
    pub(crate) fn from_legacy_audit_key(audit_key: &[u8; KEY_LEN]) -> Self {
        Self(Zeroizing::new(*audit_key))
    }

    pub(crate) fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }

    fn decode_b64(encoded: &str) -> Result<Self> {
        let mut decoded = Zeroizing::new([0_u8; KEY_LEN + 3]);
        let len = B64
            .decode_slice(encoded.as_bytes(), decoded.as_mut())
            .map_err(|_| anyhow::anyhow!("vault audit root encoding is invalid"))?;
        if len != KEY_LEN {
            bail!("vault audit root must decode to exactly {KEY_LEN} bytes");
        }
        let mut root = Zeroizing::new([0_u8; KEY_LEN]);
        root.copy_from_slice(&decoded[..KEY_LEN]);
        Ok(Self(root))
    }

    fn encode_b64(&self) -> Zeroizing<String> {
        let mut encoded = Zeroizing::new(String::with_capacity(KEY_LEN.div_ceil(3) * 4));
        B64.encode_string(self.0.as_ref(), &mut encoded);
        encoded
    }
}

impl fmt::Debug for AuditRoot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuditRoot([REDACTED])")
    }
}

/// Security fields that every version 3 state must carry.
pub(crate) struct V3StateFields {
    pub(crate) audit_root: AuditRoot,
    pub(crate) generation: u64,
    pub(crate) mutation_audit_mac: String,
}

impl fmt::Debug for V3StateFields {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("V3StateFields")
            .field("audit_root", &self.audit_root)
            .field("generation", &self.generation)
            .field("mutation_audit_mac", &self.mutation_audit_mac)
            .finish()
    }
}

impl V3StateFields {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.generation == 0 {
            bail!("vault state generation must be at least 1");
        }
        if !is_audit_mac(&self.mutation_audit_mac) {
            bail!("vault state mutation audit MAC is malformed");
        }
        Ok(())
    }
}

/// Whether `value` has the exact shape of an audit event MAC.
fn is_audit_mac(value: &str) -> bool {
    value.len() == AUDIT_MAC_HEX_LEN
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Serialize)]
struct VaultStateV3Serialized<'a> {
    secrets: &'a BTreeMap<String, SecretEntry>,
    audit_root_b64: &'a str,
    generation: u64,
    mutation_audit_mac: &'a str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VaultStateV3Deserialized {
    secrets: BTreeMap<String, SecretEntry>,
    audit_root_b64: EncodedAuditRoot,
    generation: u64,
    mutation_audit_mac: String,
}

/// Encoded audit root that wipes itself when dropped.
///
/// Derived deserialization keeps already-parsed fields in locals until the
/// whole struct exists, so a later missing or invalid field would drop a
/// plain `String` without running any container destructor. Owning the wipe
/// in the field type covers that early-return path too.
#[derive(Deserialize)]
#[serde(transparent)]
struct EncodedAuditRoot(String);

impl Drop for EncodedAuditRoot {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

pub(super) fn serialize_state(
    secrets: &BTreeMap<String, SecretEntry>,
    fields: &V3StateFields,
) -> Result<Vec<u8>> {
    fields.validate()?;
    let audit_root_b64 = fields.audit_root.encode_b64();
    Ok(serde_json::to_vec(&VaultStateV3Serialized {
        secrets,
        audit_root_b64: &audit_root_b64,
        generation: fields.generation,
        mutation_audit_mac: &fields.mutation_audit_mac,
    })?)
}

/// Decodes version 3 state. Every security field is required; nothing is
/// defaulted, and an inconsistent value fails closed.
pub(super) fn deserialize_state(
    bytes: &[u8],
) -> Result<(BTreeMap<String, SecretEntry>, V3StateFields)> {
    let VaultStateV3Deserialized {
        secrets,
        audit_root_b64,
        generation,
        mutation_audit_mac,
    } = serde_json::from_slice(bytes).context("failed to parse version 3 vault state")?;
    let fields = V3StateFields {
        audit_root: AuditRoot::decode_b64(&audit_root_b64.0)?,
        generation,
        mutation_audit_mac,
    };
    fields.validate()?;
    Ok((secrets, fields))
}

pub(super) fn secrets_fingerprint(secrets: &BTreeMap<String, SecretEntry>) -> Result<[u8; 32]> {
    let mut writer = DigestWriter(Sha256::new());
    serde_json::to_writer(&mut writer, secrets).context("failed to fingerprint vault state")?;
    Ok(writer.0.finalize().into())
}

struct DigestWriter(Sha256);

impl Write for DigestWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
