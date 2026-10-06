use std::fmt;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::aad::push_length_prefixed_field;
use crate::crypto::{KdfParams, decode_array};
use crate::types::FieldKind;

mod v3;

pub(crate) use v3::{AuditRoot, V3StateFields};

pub(crate) const MAGIC: &str = "jig-vault";
/// Legacy envelope with concealed-only entries. Readable, never created.
pub(crate) const V1_FORMAT_VERSION: u32 = 1;
/// Envelope that adds encrypted field kinds. Retained for compatibility,
/// including its legacy passphrase rewrap that keeps the DEK.
pub(crate) const V2_FORMAT_VERSION: u32 = 2;
/// Envelope that adds an independent audit root, a monotonic state
/// generation, and the MAC of the state's mutation audit event.
pub(crate) const V3_FORMAT_VERSION: u32 = 3;
/// Format created by initialization and offered as the newest explicit
/// migration target. Legacy-specific behavior must name its own version.
pub(crate) const LATEST_FORMAT_VERSION: u32 = V3_FORMAT_VERSION;
pub(crate) const AEAD_ALGORITHM: &str = "xchacha20poly1305";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AeadRole {
    State,
    WrappedDek,
}

impl AeadRole {
    const fn as_str(self) -> &'static str {
        match self {
            Self::State => "state",
            Self::WrappedDek => "wrapped_dek",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct VaultHeader {
    pub(crate) magic: String,
    pub(crate) version: u32,
    pub(crate) vault_id: String,
    pub(crate) created_at_ms: i128,
    pub(crate) kdf: KdfParams,
    pub(crate) salt_b64: String,
    pub(crate) aead: String,
    /// Committed state generation. Required by version 3 and absent from
    /// earlier envelopes, whose wire shape must stay unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) generation: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct VaultFile {
    pub(crate) header: VaultHeader,
    pub(crate) wrapped_dek_nonce_b64: String,
    pub(crate) wrapped_dek_b64: String,
    pub(crate) state_nonce_b64: String,
    pub(crate) state_b64: String,
}

/// Decrypted vault state. Version 3 additionally carries its security
/// fields; earlier versions never do.
#[derive(Default)]
pub(crate) struct VaultState {
    pub(crate) secrets: std::collections::BTreeMap<String, SecretEntry>,
    pub(crate) v3: Option<V3StateFields>,
}

impl VaultState {
    /// Serializes state using the schema authenticated by the enclosing
    /// envelope version. Each version has an explicit schema so a newer
    /// field can never leak into an envelope that an old binary would
    /// silently accept while ignoring it.
    pub(crate) fn serialize_for_version(&self, version: u32) -> Result<Vec<u8>> {
        if (version == V3_FORMAT_VERSION) != self.v3.is_some() {
            bail!("vault state security fields do not match vault format {version}");
        }
        match version {
            V1_FORMAT_VERSION => {
                let secrets = self
                    .secrets
                    .iter()
                    .map(|(name, entry)| {
                        (
                            name.as_str(),
                            SecretEntryV1Serialized {
                                value_b64: &entry.value_b64,
                                value_len: entry.value_len,
                                created_at_ms: entry.created_at_ms,
                                updated_at_ms: entry.updated_at_ms,
                            },
                        )
                    })
                    .collect();
                Ok(serde_json::to_vec(&VaultStateV1Serialized { secrets })?)
            }
            V2_FORMAT_VERSION => Ok(serde_json::to_vec(&VaultStateV2Serialized {
                secrets: &self.secrets,
            })?),
            V3_FORMAT_VERSION => {
                let fields = self.v3.as_ref().expect("checked above");
                v3::serialize_state(&self.secrets, fields)
            }
            version => bail!("unsupported vault version {version}"),
        }
    }

    pub(crate) fn deserialize_for_version(version: u32, bytes: &[u8]) -> Result<Self> {
        match version {
            V1_FORMAT_VERSION => Ok(serde_json::from_slice::<VaultStateV1Deserialized>(bytes)
                .context("failed to parse version 1 vault state")?
                .into()),
            V2_FORMAT_VERSION => {
                let state = serde_json::from_slice::<VaultStateV2Deserialized>(bytes)
                    .context("failed to parse version 2 vault state")?;
                Ok(Self {
                    secrets: state.secrets,
                    v3: None,
                })
            }
            V3_FORMAT_VERSION => {
                let (secrets, fields) = v3::deserialize_state(bytes)?;
                Ok(Self {
                    secrets,
                    v3: Some(fields),
                })
            }
            _ => unreachable!("vault headers are validated before state deserialization"),
        }
    }

    /// Digest of the logical secret map, used to recognize a no-op edit
    /// without keeping a plaintext copy of the previous state.
    pub(crate) fn secrets_fingerprint(&self) -> Result<[u8; 32]> {
        v3::secrets_fingerprint(&self.secrets)
    }
}

#[derive(Serialize)]
struct VaultStateV1Serialized<'a> {
    secrets: std::collections::BTreeMap<&'a str, SecretEntryV1Serialized<'a>>,
}

#[derive(Serialize)]
struct SecretEntryV1Serialized<'a> {
    value_b64: &'a str,
    value_len: usize,
    created_at_ms: i128,
    updated_at_ms: i128,
}

#[derive(Deserialize)]
struct VaultStateV1Deserialized {
    secrets: std::collections::BTreeMap<String, SecretEntryV1Deserialized>,
}

#[derive(Deserialize)]
struct SecretEntryV1Deserialized {
    value_b64: String,
    value_len: usize,
    created_at_ms: i128,
    updated_at_ms: i128,
}

#[derive(Serialize)]
struct VaultStateV2Serialized<'a> {
    secrets: &'a std::collections::BTreeMap<String, SecretEntry>,
}

#[derive(Deserialize)]
struct VaultStateV2Deserialized {
    secrets: std::collections::BTreeMap<String, SecretEntry>,
}

impl Drop for SecretEntryV1Deserialized {
    fn drop(&mut self) {
        self.value_b64.zeroize();
    }
}

impl From<VaultStateV1Deserialized> for VaultState {
    fn from(state: VaultStateV1Deserialized) -> Self {
        let secrets = state
            .secrets
            .into_iter()
            .map(|(name, mut entry)| {
                let value_b64 = std::mem::take(&mut entry.value_b64);
                (
                    name,
                    SecretEntry {
                        value_b64,
                        value_len: entry.value_len,
                        created_at_ms: entry.created_at_ms,
                        updated_at_ms: entry.updated_at_ms,
                        kind: FieldKind::Concealed,
                    },
                )
            })
            .collect();
        Self { secrets, v3: None }
    }
}

impl fmt::Debug for VaultState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VaultState")
            .field("secret_count", &self.secrets.len())
            .field("v3", &self.v3)
            .finish()
    }
}

#[derive(Deserialize, Serialize)]
pub(crate) struct SecretEntry {
    pub(crate) value_b64: String,
    pub(crate) value_len: usize,
    pub(crate) created_at_ms: i128,
    pub(crate) updated_at_ms: i128,
    #[serde(default)]
    pub(crate) kind: FieldKind,
}

impl fmt::Debug for SecretEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretEntry")
            .field("value_b64", &"[REDACTED]")
            .field("value_len", &self.value_len)
            .field("created_at_ms", &self.created_at_ms)
            .field("updated_at_ms", &self.updated_at_ms)
            .field("kind", &self.kind)
            .finish()
    }
}

impl Drop for SecretEntry {
    fn drop(&mut self) {
        self.value_b64.zeroize();
    }
}

pub(crate) fn validate_header(header: &VaultHeader) -> Result<()> {
    validate_header_with_supported_version(header, is_supported_format_version)
}

fn validate_header_with_supported_version(
    header: &VaultHeader,
    supports_version: impl FnOnce(u32) -> bool,
) -> Result<()> {
    if header.magic != MAGIC {
        bail!("unsupported vault magic '{}'", header.magic);
    }
    if !supports_version(header.version) {
        bail!("unsupported vault version {}", header.version);
    }
    if header.aead != AEAD_ALGORITHM {
        bail!("unsupported vault AEAD '{}'", header.aead);
    }
    match (header.version, header.generation) {
        (V3_FORMAT_VERSION, Some(generation)) if generation >= 1 => {}
        (V3_FORMAT_VERSION, Some(_)) => bail!("vault format 3 generation must be at least 1"),
        (V3_FORMAT_VERSION, None) => bail!("vault format 3 header is missing its generation"),
        (_, Some(_)) => bail!(
            "vault format {} header must not contain a generation",
            header.version
        ),
        (_, None) => {}
    }
    Ok(())
}

#[cfg(test)]
fn validate_v1_header_compat(header: &VaultHeader) -> Result<()> {
    validate_header_with_supported_version(header, |version| version == V1_FORMAT_VERSION)
}

/// Mirrors the version gate of readers released before format 3 existed.
#[cfg(test)]
fn validate_v2_reader_header_compat(header: &VaultHeader) -> Result<()> {
    validate_header_with_supported_version(header, |version| {
        matches!(version, V1_FORMAT_VERSION | V2_FORMAT_VERSION)
    })
}

pub(crate) const fn is_supported_format_version(version: u32) -> bool {
    matches!(
        version,
        V1_FORMAT_VERSION | V2_FORMAT_VERSION | V3_FORMAT_VERSION
    )
}

/// Formats with encrypted field kinds. They accept field mutation, import,
/// passphrase change, and backup; version 1 needs explicit migration first.
pub(crate) const fn supports_field_kinds(version: u32) -> bool {
    matches!(version, V2_FORMAT_VERSION | V3_FORMAT_VERSION)
}

pub(crate) fn payload_aad(header: &VaultHeader, role: AeadRole) -> Vec<u8> {
    let mut aad = header_aad_string(header);
    if header.version == V3_FORMAT_VERSION && role == AeadRole::State {
        // The generation changes with every committed state save, so only
        // the state ciphertext authenticates it. The wrapped DEK stays bound
        // to the immutable header fields and survives ordinary saves.
        let generation = header
            .generation
            .map(|generation| generation.to_string())
            .unwrap_or_default();
        push_length_prefixed_field(&mut aad, "generation", &generation);
    }
    push_length_prefixed_field(&mut aad, "payload_role", role.as_str());
    aad.into_bytes()
}

fn header_aad_string(header: &VaultHeader) -> String {
    let mut aad = String::from(match header.version {
        // Keep the v1 byte string exactly as it was before v2 existed so
        // legacy ciphertext continues to authenticate without migration.
        V1_FORMAT_VERSION => "jig-vault-header-v1\n",
        V2_FORMAT_VERSION => "jig-vault-header-v2\n",
        V3_FORMAT_VERSION => "jig-vault-header-v3\n",
        // Callers validate headers before attempting cryptography. Use an
        // impossible domain here rather than panicking if a future internal
        // caller asks for AAD before validation.
        _ => "jig-vault-header-unsupported\n",
    });
    push_length_prefixed_field(&mut aad, "magic", &header.magic);
    push_length_prefixed_field(&mut aad, "version", &header.version.to_string());
    push_length_prefixed_field(&mut aad, "vault_id", &header.vault_id);
    push_length_prefixed_field(&mut aad, "created_at_ms", &header.created_at_ms.to_string());
    push_length_prefixed_field(&mut aad, "kdf.algorithm", &header.kdf.algorithm);
    push_length_prefixed_field(
        &mut aad,
        "kdf.memory_kib",
        &header.kdf.memory_kib.to_string(),
    );
    push_length_prefixed_field(
        &mut aad,
        "kdf.iterations",
        &header.kdf.iterations.to_string(),
    );
    push_length_prefixed_field(
        &mut aad,
        "kdf.parallelism",
        &header.kdf.parallelism.to_string(),
    );
    push_length_prefixed_field(
        &mut aad,
        "kdf.output_len",
        &header.kdf.output_len.to_string(),
    );
    push_length_prefixed_field(&mut aad, "salt_b64", &header.salt_b64);
    push_length_prefixed_field(&mut aad, "aead", &header.aead);
    aad
}

pub(crate) fn decode_b64_array<const N: usize>(label: &str, value: &str) -> Result<[u8; N]> {
    let bytes = B64
        .decode(value)
        .with_context(|| format!("{label} is not valid base64"))?;
    decode_array(label, &bytes)
}

#[cfg(test)]
mod tests;
