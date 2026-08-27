use std::fmt;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::aad::push_length_prefixed_field;
use crate::crypto::{KdfParams, decode_array};
use crate::types::FieldKind;

pub(crate) const MAGIC: &str = "jig-vault";
pub(crate) const V1_FORMAT_VERSION: u32 = 1;
pub(crate) const FORMAT_VERSION: u32 = 2;
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
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct VaultFile {
    pub(crate) header: VaultHeader,
    pub(crate) wrapped_dek_nonce_b64: String,
    pub(crate) wrapped_dek_b64: String,
    pub(crate) state_nonce_b64: String,
    pub(crate) state_b64: String,
}

#[derive(Default, Deserialize, Serialize)]
pub(crate) struct VaultState {
    pub(crate) secrets: std::collections::BTreeMap<String, SecretEntry>,
}

impl VaultState {
    /// Serializes state using the schema authenticated by the enclosing
    /// envelope version. Version 1 had no field kind, so writing a legacy
    /// envelope must not add one that an old binary would silently ignore.
    pub(crate) fn serialize_for_version(&self, version: u32) -> Result<Vec<u8>> {
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
            FORMAT_VERSION => Ok(serde_json::to_vec(self)?),
            version => bail!("unsupported vault version {version}"),
        }
    }

    pub(crate) fn deserialize_for_version(version: u32, bytes: &[u8]) -> serde_json::Result<Self> {
        match version {
            V1_FORMAT_VERSION => {
                serde_json::from_slice::<VaultStateV1Deserialized>(bytes).map(Into::into)
            }
            FORMAT_VERSION => serde_json::from_slice(bytes),
            _ => unreachable!("vault headers are validated before state deserialization"),
        }
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
        Self { secrets }
    }
}

impl fmt::Debug for VaultState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VaultState")
            .field("secret_count", &self.secrets.len())
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
    Ok(())
}

#[cfg(test)]
fn validate_v1_header_compat(header: &VaultHeader) -> Result<()> {
    validate_header_with_supported_version(header, |version| version == V1_FORMAT_VERSION)
}

pub(crate) const fn is_supported_format_version(version: u32) -> bool {
    matches!(version, V1_FORMAT_VERSION | FORMAT_VERSION)
}

pub(crate) fn payload_aad(header: &VaultHeader, role: AeadRole) -> Vec<u8> {
    let mut aad = header_aad_string(header);
    push_length_prefixed_field(&mut aad, "payload_role", role.as_str());
    aad.into_bytes()
}

fn header_aad_string(header: &VaultHeader) -> String {
    let mut aad = String::from(match header.version {
        // Keep the v1 byte string exactly as it was before v2 existed so
        // legacy ciphertext continues to authenticate without migration.
        V1_FORMAT_VERSION => "jig-vault-header-v1\n",
        FORMAT_VERSION => "jig-vault-header-v2\n",
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
mod tests {
    use super::{
        AEAD_ALGORITHM, AeadRole, FORMAT_VERSION, MAGIC, V1_FORMAT_VERSION, VaultHeader,
        VaultState, payload_aad, validate_header, validate_v1_header_compat,
    };
    use crate::crypto::KdfParams;
    use crate::types::FieldKind;

    fn fixture_header(version: u32) -> VaultHeader {
        VaultHeader {
            magic: MAGIC.into(),
            version,
            vault_id: "fixture-vault".into(),
            created_at_ms: 7,
            kdf: KdfParams::default(),
            salt_b64: "c2FsdA==".into(),
            aead: AEAD_ALGORITHM.into(),
        }
    }

    #[test]
    fn v1_payload_aad_remains_byte_for_byte_stable() {
        let header = fixture_header(V1_FORMAT_VERSION);
        let aad = payload_aad(&header, AeadRole::WrappedDek);
        assert_eq!(
            aad,
            b"jig-vault-header-v1\n\
magic:9:jig-vault\n\
version:1:1\n\
vault_id:13:fixture-vault\n\
created_at_ms:1:7\n\
kdf.algorithm:8:argon2id\n\
kdf.memory_kib:6:131072\n\
kdf.iterations:1:3\n\
kdf.parallelism:1:4\n\
kdf.output_len:2:32\n\
salt_b64:8:c2FsdA==\n\
aead:17:xchacha20poly1305\n\
payload_role:11:wrapped_dek\n"
        );
    }

    #[test]
    fn version_two_has_a_distinct_aad_domain_and_old_validator_rejects_it() {
        let header = fixture_header(FORMAT_VERSION);
        validate_header(&header).unwrap();
        assert!(payload_aad(&header, AeadRole::State).starts_with(b"jig-vault-header-v2\n"));
        let error = validate_v1_header_compat(&header).unwrap_err().to_string();
        assert_eq!(error, "unsupported vault version 2");
    }

    #[test]
    fn missing_v2_field_kind_defensively_defaults_to_concealed() {
        let state: VaultState = serde_json::from_str(
            r#"{
                "secrets": {
                    "Production/RESTIC_PASSWORD": {
                        "value_b64": "c2VjcmV0",
                        "value_len": 6,
                        "created_at_ms": 1,
                        "updated_at_ms": 1
                    }
                }
            }"#,
        )
        .unwrap();

        assert_eq!(
            state.secrets["Production/RESTIC_PASSWORD"].kind,
            FieldKind::Concealed
        );
    }
}
