#[cfg(target_os = "linux")]
use std::fmt;
#[cfg(target_os = "linux")]
use std::ops::Range;

use anyhow::{Context, Result as AnyResult, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use zeroize::Zeroizing;

use crate::VaultErrorKind;
use crate::crypto::{KEY_LEN, NONCE_LEN, SALT_LEN, validate_kdf_params};
use crate::error::{classified, classify_source};
use crate::format::{
    AEAD_ALGORITHM, FORMAT_VERSION, MAGIC, V1_FORMAT_VERSION, VaultHeader, decode_b64_array,
    validate_header,
};

use super::codec::{BackupKdfParams, validate_short_ascii};

pub(super) const MAX_BACKUP_PAYLOAD_BYTES: usize = 47 * 1024 * 1024;
const MAX_SOURCE_VAULT_ID_BYTES: usize = 128;
const BACKUP_PAYLOAD_MAGIC: &[u8] = b"jig-vault-backup-payload\n";
const BACKUP_PAYLOAD_VERSION: u32 = 1;

#[cfg(target_os = "linux")]
pub(super) struct DecodedBackupArchive {
    plaintext: Zeroizing<Vec<u8>>,
    vault_range: Range<usize>,
    audit_range: Range<usize>,
    pub(super) source_vault_id: String,
    pub(super) source_format_version: u32,
    pub(super) backup_created_at_ms: i128,
}

#[cfg(target_os = "linux")]
impl DecodedBackupArchive {
    pub(super) fn vault_bytes(&self) -> &[u8] {
        &self.plaintext[self.vault_range.clone()]
    }

    pub(super) fn audit_bytes(&self) -> &[u8] {
        &self.plaintext[self.audit_range.clone()]
    }
}

#[cfg(target_os = "linux")]
impl fmt::Debug for DecodedBackupArchive {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DecodedBackupArchive")
            .field("source_vault_id", &self.source_vault_id)
            .field("source_format_version", &self.source_format_version)
            .field("backup_created_at_ms", &self.backup_created_at_ms)
            .field("vault_len", &self.vault_range.len())
            .field("audit_len", &self.audit_range.len())
            .field("contents", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

pub(super) fn encode_backup_payload(
    source_vault_id: &str,
    source_format_version: u32,
    vault_bytes: &[u8],
    audit_bytes: &[u8],
) -> AnyResult<Zeroizing<Vec<u8>>> {
    validate_payload_metadata(
        source_vault_id,
        source_format_version,
        vault_bytes,
        audit_bytes,
    )?;
    let fixed_len = backup_payload_fixed_len(source_vault_id.len())?;
    let payload_len = fixed_len
        .checked_add(vault_bytes.len())
        .and_then(|len| len.checked_add(audit_bytes.len()))
        .ok_or_else(|| anyhow::anyhow!("backup payload length overflow"))?;
    if payload_len > MAX_BACKUP_PAYLOAD_BYTES {
        let max_audit = MAX_BACKUP_PAYLOAD_BYTES
            .saturating_sub(fixed_len)
            .saturating_sub(vault_bytes.len());
        bail!(
            "vault audit log is {} bytes; this one-shot backup supports at most {max_audit} audit bytes for the current vault size",
            audit_bytes.len()
        );
    }

    let mut payload = Zeroizing::new(Vec::with_capacity(payload_len));
    payload.extend_from_slice(BACKUP_PAYLOAD_MAGIC);
    push_u32(&mut payload, BACKUP_PAYLOAD_VERSION);
    push_len_prefixed_u32(&mut payload, source_vault_id.as_bytes())?;
    push_u32(&mut payload, source_format_version);
    push_len_prefixed_u64(&mut payload, vault_bytes)?;
    push_len_prefixed_u64(&mut payload, audit_bytes)?;
    debug_assert_eq!(payload.len(), payload_len);
    Ok(payload)
}

pub(crate) fn max_backup_audit_bytes(
    source_vault_id_len: usize,
    vault_len: usize,
) -> AnyResult<usize> {
    let fixed_len = backup_payload_fixed_len(source_vault_id_len)?;
    MAX_BACKUP_PAYLOAD_BYTES
        .checked_sub(fixed_len)
        .and_then(|remaining| remaining.checked_sub(vault_len))
        .context("vault state leaves no room in the bounded backup payload")
}

fn backup_payload_fixed_len(source_vault_id_len: usize) -> AnyResult<usize> {
    BACKUP_PAYLOAD_MAGIC
        .len()
        .checked_add(4 + 4 + source_vault_id_len + 4 + 8 + 8)
        .context("backup payload length overflow")
}

#[cfg(target_os = "linux")]
pub(super) fn decode_backup_payload(
    plaintext: Zeroizing<Vec<u8>>,
    backup_created_at_ms: i128,
) -> AnyResult<DecodedBackupArchive> {
    if plaintext.len() > MAX_BACKUP_PAYLOAD_BYTES {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            "decrypted backup payload exceeds the supported one-shot limit",
        ));
    }
    let mut reader = PayloadReader::new(&plaintext);
    if reader.take(BACKUP_PAYLOAD_MAGIC.len())? != BACKUP_PAYLOAD_MAGIC {
        return Err(classified(
            VaultErrorKind::Serialization,
            "backup payload magic is invalid",
        ));
    }
    let payload_version = reader.read_u32()?;
    if payload_version != BACKUP_PAYLOAD_VERSION {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            format!("unsupported backup payload version {payload_version}"),
        ));
    }
    let source_vault_id_bytes = reader.read_len_prefixed_u32(MAX_SOURCE_VAULT_ID_BYTES)?;
    let source_vault_id = std::str::from_utf8(source_vault_id_bytes)
        .context("backup source vault ID is not valid UTF-8")?
        .to_owned();
    let source_format_version = reader.read_u32()?;
    let vault_range = reader.read_range_u64(crate::store::VAULT_TEXT_READ_LIMIT as usize)?;
    let remaining_limit = MAX_BACKUP_PAYLOAD_BYTES.saturating_sub(reader.position());
    let audit_range = reader.read_range_u64(remaining_limit)?;
    reader.finish()?;

    validate_payload_metadata(
        &source_vault_id,
        source_format_version,
        &plaintext[vault_range.clone()],
        &plaintext[audit_range.clone()],
    )?;
    Ok(DecodedBackupArchive {
        plaintext,
        vault_range,
        audit_range,
        source_vault_id,
        source_format_version,
        backup_created_at_ms,
    })
}

fn validate_payload_metadata(
    source_vault_id: &str,
    source_format_version: u32,
    vault_bytes: &[u8],
    audit_bytes: &[u8],
) -> AnyResult<()> {
    if source_vault_id.is_empty() || source_vault_id.len() > MAX_SOURCE_VAULT_ID_BYTES {
        return Err(classified(
            VaultErrorKind::Serialization,
            "backup source vault ID length is outside supported bounds",
        ));
    }
    if source_format_version == V1_FORMAT_VERSION {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            format!(
                "backup contains vault format {V1_FORMAT_VERSION}; migrate the source first with `jig vault migrate --to {FORMAT_VERSION}` and create a new backup"
            ),
        ));
    }
    if source_format_version != FORMAT_VERSION {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            format!("unsupported embedded vault format {source_format_version}"),
        ));
    }
    let (embedded_vault_id, embedded_format_version) = inspect_embedded_vault(vault_bytes)?;
    if embedded_format_version != source_format_version {
        return Err(classified(
            VaultErrorKind::Serialization,
            "backup source format does not match the embedded vault header",
        ));
    }
    if embedded_vault_id != source_vault_id {
        return Err(classified(
            VaultErrorKind::Serialization,
            "backup source vault ID does not match the embedded vault header",
        ));
    }
    std::str::from_utf8(audit_bytes).map_err(|error| {
        classify_source(
            VaultErrorKind::Serialization,
            "embedded vault audit log is not valid UTF-8",
            error.into(),
        )
    })?;
    Ok(())
}

pub(crate) fn inspect_embedded_vault(vault_bytes: &[u8]) -> AnyResult<(String, u32)> {
    if vault_bytes.len() > crate::store::VAULT_TEXT_READ_LIMIT as usize {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            "embedded vault state exceeds the persistent vault size limit",
        ));
    }
    let vault_text = std::str::from_utf8(vault_bytes).map_err(|error| {
        classify_source(
            VaultErrorKind::Serialization,
            "embedded vault state is not valid UTF-8",
            error.into(),
        )
    })?;
    let embedded: StrictEmbeddedVaultFile = serde_json::from_str(vault_text).map_err(|error| {
        classify_source(
            VaultErrorKind::Serialization,
            "failed to parse complete embedded vault envelope",
            error.into(),
        )
    })?;
    let embedded_header = embedded.header.to_vault_header();
    validate_short_ascii("embedded vault magic", &embedded_header.magic, 32)?;
    if embedded_header.magic != MAGIC {
        return Err(classified(
            VaultErrorKind::Serialization,
            "unsupported embedded vault magic",
        ));
    }
    if embedded_header.version == V1_FORMAT_VERSION {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            format!(
                "backup contains vault format {V1_FORMAT_VERSION}; migrate the source first with `jig vault migrate --to {FORMAT_VERSION}` and create a new backup"
            ),
        ));
    }
    if embedded_header.version != FORMAT_VERSION {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            "unsupported embedded vault format",
        ));
    }
    validate_short_ascii("embedded vault AEAD", &embedded_header.aead, 32)?;
    if embedded_header.aead != AEAD_ALGORITHM {
        return Err(classified(
            VaultErrorKind::Serialization,
            "unsupported embedded vault AEAD",
        ));
    }
    validate_short_ascii(
        "embedded vault KDF algorithm",
        &embedded_header.kdf.algorithm,
        16,
    )?;
    if embedded_header.kdf.algorithm != "argon2id" {
        return Err(classified(
            VaultErrorKind::Serialization,
            "unsupported embedded vault KDF algorithm",
        ));
    }
    validate_kdf_params(&embedded_header.kdf).map_err(|_| {
        classified(
            VaultErrorKind::Serialization,
            "unsupported embedded vault KDF parameters",
        )
    })?;
    if embedded_header.salt_b64.len() > 32 {
        return Err(classified(
            VaultErrorKind::Serialization,
            "embedded vault salt encoding is outside supported bounds",
        ));
    }
    decode_b64_array::<SALT_LEN>("embedded vault salt", &embedded_header.salt_b64)?;
    validate_header(&embedded_header).map_err(|error| {
        classify_source(
            VaultErrorKind::Serialization,
            "embedded vault header is invalid",
            error,
        )
    })?;
    decode_b64_array::<NONCE_LEN>(
        "embedded wrapped vault key nonce",
        &embedded.wrapped_dek_nonce_b64,
    )?;
    let wrapped_dek = B64.decode(&embedded.wrapped_dek_b64).map_err(|error| {
        classify_source(
            VaultErrorKind::Serialization,
            "embedded wrapped vault key is not valid base64",
            error.into(),
        )
    })?;
    if wrapped_dek.len() != KEY_LEN + 16 {
        return Err(classified(
            VaultErrorKind::Serialization,
            "embedded wrapped vault key length is invalid",
        ));
    }
    decode_b64_array::<NONCE_LEN>("embedded vault state nonce", &embedded.state_nonce_b64)?;
    let state = B64.decode(&embedded.state_b64).map_err(|error| {
        classify_source(
            VaultErrorKind::Serialization,
            "embedded vault state is not valid base64",
            error.into(),
        )
    })?;
    if state.len() < 16 {
        return Err(classified(
            VaultErrorKind::Serialization,
            "embedded vault state ciphertext is truncated",
        ));
    }
    Ok((embedded_header.vault_id, embedded_header.version))
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictEmbeddedVaultFile {
    header: StrictEmbeddedVaultHeader,
    wrapped_dek_nonce_b64: String,
    wrapped_dek_b64: String,
    state_nonce_b64: String,
    state_b64: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictEmbeddedVaultHeader {
    magic: String,
    version: u32,
    vault_id: String,
    created_at_ms: i128,
    kdf: BackupKdfParams,
    salt_b64: String,
    aead: String,
}

impl StrictEmbeddedVaultHeader {
    fn to_vault_header(&self) -> VaultHeader {
        VaultHeader {
            magic: self.magic.clone(),
            version: self.version,
            vault_id: self.vault_id.clone(),
            created_at_ms: self.created_at_ms,
            kdf: self.kdf.as_vault_params(),
            salt_b64: self.salt_b64.clone(),
            aead: self.aead.clone(),
        }
    }
}

fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn push_len_prefixed_u32(output: &mut Vec<u8>, bytes: &[u8]) -> AnyResult<()> {
    let len = u32::try_from(bytes.len()).context("backup field is too large")?;
    push_u32(output, len);
    output.extend_from_slice(bytes);
    Ok(())
}

fn push_len_prefixed_u64(output: &mut Vec<u8>, bytes: &[u8]) -> AnyResult<()> {
    let len = u64::try_from(bytes.len()).context("backup blob is too large")?;
    output.extend_from_slice(&len.to_be_bytes());
    output.extend_from_slice(bytes);
    Ok(())
}

#[cfg(target_os = "linux")]
struct PayloadReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

#[cfg(target_os = "linux")]
impl<'a> PayloadReader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    const fn position(&self) -> usize {
        self.offset
    }

    fn take(&mut self, len: usize) -> AnyResult<&'a [u8]> {
        let end = self
            .offset
            .checked_add(len)
            .context("backup payload offset overflow")?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .context("backup payload is truncated")?;
        self.offset = end;
        Ok(bytes)
    }

    fn read_u32(&mut self) -> AnyResult<u32> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four-byte slice"),
        ))
    }

    fn read_u64(&mut self) -> AnyResult<u64> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight-byte slice"),
        ))
    }

    fn read_len_prefixed_u32(&mut self, max_len: usize) -> AnyResult<&'a [u8]> {
        let len = usize::try_from(self.read_u32()?).context("backup field length overflow")?;
        if len > max_len {
            bail!("backup field length {len} exceeds the {max_len} byte limit");
        }
        self.take(len)
    }

    fn read_range_u64(&mut self, max_len: usize) -> AnyResult<Range<usize>> {
        let len = usize::try_from(self.read_u64()?).context("backup blob length overflow")?;
        if len > max_len {
            bail!("backup blob length {len} exceeds the {max_len} byte limit");
        }
        let start = self.offset;
        self.take(len)?;
        Ok(start..self.offset)
    }

    fn finish(self) -> AnyResult<()> {
        if self.offset != self.bytes.len() {
            bail!("backup payload contains trailing bytes");
        }
        Ok(())
    }
}
