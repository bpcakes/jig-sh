use std::fmt;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::crypto::{KdfParams, decode_array};

pub(crate) const MAGIC: &str = "jig-vault";
pub(crate) const FORMAT_VERSION: u32 = 1;
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
}

impl fmt::Debug for SecretEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretEntry")
            .field("value_b64", &"[REDACTED]")
            .field("value_len", &self.value_len)
            .field("created_at_ms", &self.created_at_ms)
            .field("updated_at_ms", &self.updated_at_ms)
            .finish()
    }
}

impl Drop for SecretEntry {
    fn drop(&mut self) {
        self.value_b64.zeroize();
    }
}

pub(crate) fn validate_header(header: &VaultHeader) -> Result<()> {
    if header.magic != MAGIC {
        bail!("unsupported vault magic '{}'", header.magic);
    }
    if header.version != FORMAT_VERSION {
        bail!("unsupported vault version {}", header.version);
    }
    if header.aead != AEAD_ALGORITHM {
        bail!("unsupported vault AEAD '{}'", header.aead);
    }
    Ok(())
}

pub(crate) fn payload_aad(header: &VaultHeader, role: AeadRole) -> Vec<u8> {
    let mut aad = header_aad_string(header);
    push_aad_field(&mut aad, "payload_role", role.as_str());
    aad.into_bytes()
}

fn header_aad_string(header: &VaultHeader) -> String {
    let mut aad = String::from("jig-vault-header-v1\n");
    push_aad_field(&mut aad, "magic", &header.magic);
    push_aad_field(&mut aad, "version", &header.version.to_string());
    push_aad_field(&mut aad, "vault_id", &header.vault_id);
    push_aad_field(&mut aad, "created_at_ms", &header.created_at_ms.to_string());
    push_aad_field(&mut aad, "kdf.algorithm", &header.kdf.algorithm);
    push_aad_field(
        &mut aad,
        "kdf.memory_kib",
        &header.kdf.memory_kib.to_string(),
    );
    push_aad_field(
        &mut aad,
        "kdf.iterations",
        &header.kdf.iterations.to_string(),
    );
    push_aad_field(
        &mut aad,
        "kdf.parallelism",
        &header.kdf.parallelism.to_string(),
    );
    push_aad_field(
        &mut aad,
        "kdf.output_len",
        &header.kdf.output_len.to_string(),
    );
    push_aad_field(&mut aad, "salt_b64", &header.salt_b64);
    push_aad_field(&mut aad, "aead", &header.aead);
    aad
}

fn push_aad_field(output: &mut String, name: &str, value: &str) {
    use std::fmt::Write;

    // Lengths are UTF-8 byte counts, not character counts, for a stable AAD
    // byte string.
    writeln!(output, "{name}:{}:{value}", value.len()).expect("writing to String cannot fail");
}

pub(crate) fn decode_b64_array<const N: usize>(label: &str, value: &str) -> Result<[u8; N]> {
    let bytes = B64
        .decode(value)
        .with_context(|| format!("{label} is not valid base64"))?;
    decode_array(label, &bytes)
}
