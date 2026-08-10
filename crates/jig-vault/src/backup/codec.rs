use std::fmt;

#[cfg(any(target_os = "linux", test))]
use anyhow::Context;
use anyhow::{Result as AnyResult, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use secrecy::SecretString;
use zeroize::Zeroizing;

use crate::SecretBytes;
#[cfg(any(target_os = "linux", test))]
use crate::VaultErrorKind;
#[cfg(target_os = "linux")]
use crate::crypto::open;
use crate::crypto::{
    KdfParams, NONCE_LEN, SALT_LEN, derive_wrap_key, random_array, seal, validate_kdf_params,
};
#[cfg(any(target_os = "linux", test))]
use crate::error::{classified, classify_source};
use crate::format::{AEAD_ALGORITHM, decode_b64_array};

#[cfg(target_os = "linux")]
use super::payload::{DecodedBackupArchive, decode_backup_payload};
use super::payload::{MAX_BACKUP_PAYLOAD_BYTES, encode_backup_payload};
use super::{BACKUP_FORMAT_VERSION, MAX_BACKUP_ARCHIVE_BYTES};

const MAX_BACKUP_CIPHERTEXT_BYTES: usize = MAX_BACKUP_PAYLOAD_BYTES + 16;
pub(super) const BACKUP_MAGIC: &str = "jig-vault-backup";
pub(super) const BACKUP_AAD_DOMAIN: &str = "jig-vault-backup-header-v1\n";

#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BackupHeader {
    pub(super) magic: String,
    pub(super) version: u32,
    pub(super) created_at_ms: i128,
    pub(super) kdf: BackupKdfParams,
    pub(super) salt_b64: String,
    pub(super) aead: String,
    pub(super) nonce_b64: String,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BackupKdfParams {
    pub(super) algorithm: String,
    pub(super) memory_kib: u32,
    pub(super) iterations: u32,
    pub(super) parallelism: u32,
    pub(super) output_len: u32,
}

impl BackupKdfParams {
    pub(super) fn as_vault_params(&self) -> KdfParams {
        KdfParams {
            algorithm: self.algorithm.clone(),
            memory_kib: self.memory_kib,
            iterations: self.iterations,
            parallelism: self.parallelism,
            output_len: self.output_len,
        }
    }
}

impl From<KdfParams> for BackupKdfParams {
    fn from(params: KdfParams) -> Self {
        Self {
            algorithm: params.algorithm,
            memory_kib: params.memory_kib,
            iterations: params.iterations,
            parallelism: params.parallelism,
            output_len: params.output_len,
        }
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BackupEnvelope {
    pub(super) header: BackupHeader,
    pub(super) ciphertext_b64: String,
}

#[cfg(any(target_os = "linux", test))]
pub(super) struct ParsedBackupArchive {
    #[cfg(target_os = "linux")]
    header: BackupHeader,
    #[cfg(target_os = "linux")]
    salt: [u8; SALT_LEN],
    #[cfg(target_os = "linux")]
    nonce: [u8; NONCE_LEN],
    #[cfg(target_os = "linux")]
    ciphertext: Zeroizing<Vec<u8>>,
    #[cfg(target_os = "linux")]
    pub(super) serialized_len: usize,
}

pub(super) struct SealedBackupArchive {
    pub(super) bytes: SecretBytes,
    pub(super) created_at_ms: i128,
}

impl fmt::Debug for BackupHeader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BackupHeader")
            .field("magic", &self.magic)
            .field("version", &self.version)
            .field("created_at_ms", &self.created_at_ms)
            .field("kdf", &self.kdf)
            .field("salt_b64", &"[REDACTED]")
            .field("aead", &self.aead)
            .field("nonce_b64", &"[REDACTED]")
            .finish()
    }
}

#[cfg(any(target_os = "linux", test))]
impl fmt::Debug for ParsedBackupArchive {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("ParsedBackupArchive");
        #[cfg(target_os = "linux")]
        debug
            .field("header", &self.header)
            .field("serialized_len", &self.serialized_len)
            .field("ciphertext", &"[REDACTED]");
        debug.finish_non_exhaustive()
    }
}

pub(super) fn seal_archive(
    passphrase: &SecretString,
    source_vault_id: &str,
    source_format_version: u32,
    vault_bytes: &[u8],
    audit_bytes: &[u8],
    created_at_ms: i128,
) -> AnyResult<SealedBackupArchive> {
    let payload = encode_backup_payload(
        source_vault_id,
        source_format_version,
        vault_bytes,
        audit_bytes,
    )?;
    let salt = random_array::<SALT_LEN>()?;
    let nonce = random_array::<NONCE_LEN>()?;
    let header = BackupHeader {
        magic: BACKUP_MAGIC.into(),
        version: BACKUP_FORMAT_VERSION,
        created_at_ms,
        kdf: KdfParams::default().into(),
        salt_b64: B64.encode(salt),
        aead: AEAD_ALGORITHM.into(),
        nonce_b64: B64.encode(nonce),
    };
    validate_backup_header(&header)?;
    let key = derive_wrap_key(passphrase, &salt, &header.kdf.as_vault_params())?;
    let ciphertext = seal(&key, &nonce, &backup_aad(&header), &payload)?;
    if ciphertext.len() > MAX_BACKUP_CIPHERTEXT_BYTES {
        bail!("encrypted backup payload exceeds the supported one-shot archive limit");
    }
    let envelope = BackupEnvelope {
        header,
        ciphertext_b64: B64.encode(ciphertext),
    };
    let serialized = Zeroizing::new(serde_json::to_vec_pretty(&envelope)?);
    if serialized.len() > MAX_BACKUP_ARCHIVE_BYTES {
        bail!(
            "backup archive is {} bytes, exceeding the {MAX_BACKUP_ARCHIVE_BYTES} byte one-shot archive limit",
            serialized.len()
        );
    }
    Ok(SealedBackupArchive {
        bytes: SecretBytes::new(serialized.to_vec()),
        created_at_ms,
    })
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn parse_archive_bytes(bytes: Zeroizing<Vec<u8>>) -> AnyResult<ParsedBackupArchive> {
    if bytes.len() > MAX_BACKUP_ARCHIVE_BYTES {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            format!(
                "backup archive is {} bytes, exceeding the {MAX_BACKUP_ARCHIVE_BYTES} byte read limit",
                bytes.len()
            ),
        ));
    }
    let envelope: BackupEnvelope = serde_json::from_slice(&bytes).map_err(|error| {
        classify_source(
            VaultErrorKind::Serialization,
            "failed to parse backup archive",
            error.into(),
        )
    })?;
    let validated_header = validate_backup_header(&envelope.header).map_err(|error| {
        classify_source(
            VaultErrorKind::Serialization,
            "backup public header is invalid",
            error,
        )
    })?;
    #[cfg(target_os = "linux")]
    let (salt, nonce) = validated_header;
    #[cfg(not(target_os = "linux"))]
    let _ = validated_header;
    let max_encoded = padded_base64_len(MAX_BACKUP_CIPHERTEXT_BYTES)?;
    if envelope.ciphertext_b64.len() > max_encoded {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            "backup ciphertext exceeds the supported one-shot archive limit",
        ));
    }
    let ciphertext = Zeroizing::new(B64.decode(&envelope.ciphertext_b64).map_err(|error| {
        classify_source(
            VaultErrorKind::Serialization,
            "backup ciphertext is not valid base64",
            error.into(),
        )
    })?);
    if ciphertext.len() < 16 || ciphertext.len() > MAX_BACKUP_CIPHERTEXT_BYTES {
        return Err(classified(
            VaultErrorKind::InvalidInput,
            "backup ciphertext length is outside supported bounds",
        ));
    }
    Ok(ParsedBackupArchive {
        #[cfg(target_os = "linux")]
        header: envelope.header,
        #[cfg(target_os = "linux")]
        salt,
        #[cfg(target_os = "linux")]
        nonce,
        #[cfg(target_os = "linux")]
        ciphertext,
        #[cfg(target_os = "linux")]
        serialized_len: bytes.len(),
    })
}

#[cfg(target_os = "linux")]
pub(super) fn decrypt_archive(
    passphrase: &SecretString,
    archive: ParsedBackupArchive,
) -> AnyResult<DecodedBackupArchive> {
    let ParsedBackupArchive {
        header,
        salt,
        nonce,
        ciphertext,
        ..
    } = archive;
    let key =
        derive_wrap_key(passphrase, &salt, &header.kdf.as_vault_params()).map_err(|error| {
            classify_source(
                VaultErrorKind::Serialization,
                "backup KDF parameters are invalid",
                error,
            )
        })?;
    let plaintext = open(&key, &nonce, &backup_aad(&header), &ciphertext).map_err(|error| {
        classify_source(
            VaultErrorKind::Authentication,
            "failed to authenticate backup archive with the supplied passphrase",
            error,
        )
    })?;
    decode_backup_payload(plaintext, header.created_at_ms)
}

fn validate_backup_header(header: &BackupHeader) -> AnyResult<([u8; SALT_LEN], [u8; NONCE_LEN])> {
    validate_short_ascii("backup magic", &header.magic, 32)?;
    if header.magic != BACKUP_MAGIC {
        bail!("unsupported backup magic");
    }
    if header.version != BACKUP_FORMAT_VERSION {
        bail!("unsupported backup version {}", header.version);
    }
    if header.created_at_ms < 0 {
        bail!("backup creation timestamp must not be negative");
    }
    validate_short_ascii("backup AEAD", &header.aead, 32)?;
    if header.aead != AEAD_ALGORITHM {
        bail!("unsupported backup AEAD");
    }
    validate_short_ascii("backup KDF algorithm", &header.kdf.algorithm, 16)?;
    if header.kdf.algorithm != "argon2id" {
        bail!("unsupported backup KDF algorithm");
    }
    validate_kdf_params(&header.kdf.as_vault_params())
        .map_err(|_| anyhow::anyhow!("unsupported backup KDF parameters"))?;
    if header.salt_b64.len() > 32 || header.nonce_b64.len() > 48 {
        bail!("backup salt or nonce encoding is outside supported bounds");
    }
    let salt = decode_b64_array::<SALT_LEN>("backup salt", &header.salt_b64)?;
    let nonce = decode_b64_array::<NONCE_LEN>("backup nonce", &header.nonce_b64)?;
    Ok((salt, nonce))
}

pub(super) fn validate_short_ascii(label: &str, value: &str, max_len: usize) -> AnyResult<()> {
    if value.is_empty() || value.len() > max_len || !value.is_ascii() {
        bail!("{label} is outside supported bounds");
    }
    Ok(())
}

pub(super) fn backup_aad(header: &BackupHeader) -> Vec<u8> {
    let mut aad = String::from(BACKUP_AAD_DOMAIN);
    push_aad_field(&mut aad, "magic", &header.magic);
    push_aad_field(&mut aad, "version", &header.version.to_string());
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
    push_aad_field(&mut aad, "nonce_b64", &header.nonce_b64);
    push_aad_field(&mut aad, "payload_role", "backup_payload");
    aad.into_bytes()
}

fn push_aad_field(output: &mut String, name: &str, value: &str) {
    use std::fmt::Write;
    writeln!(output, "{name}:{}:{value}", value.len()).expect("writing to String cannot fail");
}

#[cfg(any(target_os = "linux", test))]
fn padded_base64_len(len: usize) -> AnyResult<usize> {
    len.checked_add(2)
        .and_then(|len| len.checked_div(3))
        .and_then(|len| len.checked_mul(4))
        .context("backup base64 length overflow")
}
