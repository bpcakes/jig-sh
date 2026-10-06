//! Envelope sealing for initialization, ordinary saves, migration, and
//! passphrase change.
//!
//! Key ownership: every DEK, wrapping key, and state plaintext produced here
//! lives in a `Zeroizing` owner returned inside the sealed envelope value,
//! which the caller keeps until the serialized envelope is persisted and
//! then drops on every success and error path. The caller's previous DEK
//! stays in its own zeroizing owner (the unlocked vault). Moving fixed-size
//! key arrays can leave transient copies that no owner wipes; this is a
//! source-reviewed best effort, not something a unit test can prove.

use anyhow::Result as AnyResult;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use secrecy::SecretString;
use zeroize::Zeroizing;

use crate::crypto::{
    KEY_LEN, KdfParams, NONCE_LEN, SALT_LEN, derive_audit_key, derive_wrap_key, random_array,
    random_key, seal,
};
use crate::error::{VaultErrorKind, classified, classify_source};
use crate::format::{
    AEAD_ALGORITHM, AeadRole, AuditRoot, LATEST_FORMAT_VERSION, MAGIC, V3_FORMAT_VERSION,
    V3StateFields, VaultFile, VaultHeader, VaultState, decode_b64_array, payload_aad,
    rotates_dek_on_rekey, supports_field_kinds, validate_header,
};

/// Fresh identity and key material for a new vault, created before its
/// initialization audit event so a version 3 state can carry that event's
/// MAC.
pub(in crate::vault) struct NewVaultMaterial {
    header: VaultHeader,
    salt: [u8; SALT_LEN],
    dek: Zeroizing<[u8; KEY_LEN]>,
    audit_key: Zeroizing<[u8; KEY_LEN]>,
    audit_root: Option<AuditRoot>,
}

pub(in crate::vault) struct NewVaultEnvelope {
    // Keep the serialized file, state plaintext, wrap key, and DEK alive
    // until the initialized vault file has been written.
    pub(in crate::vault) file_text: String,
    _state_plaintext: Zeroizing<Vec<u8>>,
    _wrap_key: Zeroizing<[u8; KEY_LEN]>,
    _dek: Zeroizing<[u8; KEY_LEN]>,
}

pub(in crate::vault) struct ResealedVaultEnvelope {
    file: VaultFile,
    // The former save_unlocked local lived through the atomic write. Retain
    // this zeroizing plaintext until the serialized envelope is written too.
    _state_plaintext: Zeroizing<Vec<u8>>,
}

pub(in crate::vault) struct MigratedVaultEnvelope {
    file: VaultFile,
    // The migration must seal the target state under target AAD before the
    // vault file is written. Keep both secret-bearing intermediate values
    // zeroizing through the atomic write just like ordinary resealing does.
    _state_plaintext: Zeroizing<Vec<u8>>,
    _wrap_key: Zeroizing<[u8; KEY_LEN]>,
}

/// A passphrase change. Formats that rotate the data-encryption key reseal
/// all state under a fresh DEK, salt, wrapping key, and nonces and wrap only
/// the new DEK, so a DEK recovered from any earlier envelope cannot decrypt
/// the successor. Format 2 keeps its frozen contract: the unchanged DEK is
/// rewrapped under the new passphrase and both payloads are resealed.
pub(in crate::vault) struct RekeyedVaultEnvelope {
    file: VaultFile,
    // Retain plaintext, the derived wrap key, and any fresh DEK in zeroizing
    // storage until the serialized envelope has been atomically written.
    _state_plaintext: Zeroizing<Vec<u8>>,
    _wrap_key: Zeroizing<[u8; KEY_LEN]>,
    _rotated_dek: Option<Zeroizing<[u8; KEY_LEN]>>,
}

struct SealedPayloads {
    wrapped_dek_nonce_b64: String,
    wrapped_dek_b64: String,
    state_nonce_b64: String,
    state_b64: String,
    state_plaintext: Zeroizing<Vec<u8>>,
}

impl SealedPayloads {
    fn into_file(self, header: VaultHeader) -> (VaultFile, Zeroizing<Vec<u8>>) {
        (
            VaultFile {
                header,
                wrapped_dek_nonce_b64: self.wrapped_dek_nonce_b64,
                wrapped_dek_b64: self.wrapped_dek_b64,
                state_nonce_b64: self.state_nonce_b64,
                state_b64: self.state_b64,
            },
            self.state_plaintext,
        )
    }
}

/// Wraps the DEK and seals state with fresh nonces under `header`.
fn seal_payloads(
    header: &VaultHeader,
    wrap_key: &[u8; KEY_LEN],
    dek: &[u8; KEY_LEN],
    state: &VaultState,
) -> AnyResult<SealedPayloads> {
    let wrapped_dek_nonce = random_array::<NONCE_LEN>()?;
    let wrapped_dek = seal(
        wrap_key,
        &wrapped_dek_nonce,
        &payload_aad(header, AeadRole::WrappedDek),
        dek,
    )?;
    let (state_nonce_b64, state_b64, state_plaintext) = seal_state(header, dek, state)?;
    Ok(SealedPayloads {
        wrapped_dek_nonce_b64: B64.encode(wrapped_dek_nonce),
        wrapped_dek_b64: B64.encode(wrapped_dek),
        state_nonce_b64,
        state_b64,
        state_plaintext,
    })
}

fn seal_state(
    header: &VaultHeader,
    dek: &[u8; KEY_LEN],
    state: &VaultState,
) -> AnyResult<(String, String, Zeroizing<Vec<u8>>)> {
    let state_nonce = random_array::<NONCE_LEN>()?;
    let state_plaintext = Zeroizing::new(state.serialize_for_version(header.version)?);
    let encrypted_state = seal(
        dek,
        &state_nonce,
        &payload_aad(header, AeadRole::State),
        &state_plaintext,
    )?;
    Ok((
        B64.encode(state_nonce),
        B64.encode(encrypted_state),
        state_plaintext,
    ))
}

/// Header for sealing `state`: version 3 carries the state's generation,
/// earlier versions never carry one.
fn header_for_state(
    mut header: VaultHeader,
    state: &VaultState,
    label: &str,
) -> AnyResult<VaultHeader> {
    header.generation = state.v3.as_ref().map(|fields| fields.generation);
    validate_header(&header).map_err(|error| {
        classify_source(
            VaultErrorKind::Internal,
            format!("constructed {label} vault header is invalid"),
            error,
        )
    })?;
    Ok(header)
}

/// An envelope sealed under a fresh DEK, salt, wrapping key, and nonces.
/// Every secret-bearing intermediate stays in its zeroizing owner until the
/// caller has persisted the serialized envelope.
struct FreshKeySealed {
    file: VaultFile,
    state_plaintext: Zeroizing<Vec<u8>>,
    wrap_key: Zeroizing<[u8; KEY_LEN]>,
    dek: Zeroizing<[u8; KEY_LEN]>,
}

/// Seals `state` under fresh key material for `header`, adopting the current
/// KDF policy while keeping the header's identity and creation time.
fn seal_under_fresh_key(
    mut header: VaultHeader,
    passphrase: &SecretString,
    state: &VaultState,
    kdf: KdfParams,
    label: &str,
) -> AnyResult<FreshKeySealed> {
    if state.v3.is_none() {
        anyhow::bail!("only format 3 state can be resealed under a fresh vault key");
    }
    let salt = random_array::<SALT_LEN>()?;
    let dek = random_key()?;
    header.kdf = kdf;
    header.salt_b64 = B64.encode(salt);
    let header = header_for_state(header, state, label)?;
    let wrap_key = derive_wrap_key(passphrase, &salt, &header.kdf).map_err(|error| {
        classify_source(
            VaultErrorKind::InvalidInput,
            "vault passphrase could not be derived safely",
            error,
        )
    })?;
    let (file, state_plaintext) = seal_payloads(&header, &wrap_key, &dek, state)?.into_file(header);
    Ok(FreshKeySealed {
        file,
        state_plaintext,
        wrap_key,
        dek,
    })
}

fn derive_header_wrap_key(
    passphrase: &SecretString,
    header: &VaultHeader,
) -> AnyResult<Zeroizing<[u8; KEY_LEN]>> {
    let salt = decode_b64_array::<SALT_LEN>("vault salt", &header.salt_b64).map_err(|error| {
        classify_source(
            VaultErrorKind::Serialization,
            "vault salt is invalid",
            error,
        )
    })?;
    derive_wrap_key(passphrase, &salt, &header.kdf).map_err(|error| {
        classify_source(
            VaultErrorKind::Serialization,
            "vault KDF parameters are invalid",
            error,
        )
    })
}

impl NewVaultMaterial {
    pub(in crate::vault) fn generate(
        version: u32,
        created_at_ms: i128,
        kdf: KdfParams,
    ) -> AnyResult<Self> {
        let salt = random_array::<SALT_LEN>()?;
        let dek = Zeroizing::new(random_array::<KEY_LEN>()?);
        let (audit_key, audit_root) = if version == V3_FORMAT_VERSION {
            // A new version 3 vault never derives its audit root from the
            // DEK, so later DEK rotation leaves audit verification intact.
            let root = AuditRoot::random()?;
            (Zeroizing::new(*root.as_bytes()), Some(root))
        } else {
            (derive_audit_key(&dek)?, None)
        };
        let header = VaultHeader {
            magic: MAGIC.into(),
            version,
            vault_id: ulid::Ulid::new().to_string(),
            created_at_ms,
            kdf,
            salt_b64: B64.encode(salt),
            aead: AEAD_ALGORITHM.into(),
            generation: audit_root.as_ref().map(|_| 1),
        };
        validate_header(&header).map_err(|error| {
            classify_source(
                VaultErrorKind::Internal,
                "constructed vault header is invalid",
                error,
            )
        })?;
        Ok(Self {
            header,
            salt,
            dek,
            audit_key,
            audit_root,
        })
    }

    pub(in crate::vault) fn vault_id(&self) -> &str {
        &self.header.vault_id
    }

    pub(in crate::vault) fn audit_key(&self) -> &[u8; KEY_LEN] {
        &self.audit_key
    }

    pub(in crate::vault) const fn is_v3(&self) -> bool {
        self.audit_root.is_some()
    }

    /// Seals an empty vault. A version 3 vault requires the MAC of its
    /// already prepared initialization event; earlier versions reject one.
    pub(in crate::vault) fn seal(
        self,
        passphrase: &SecretString,
        mutation_audit_mac: Option<String>,
    ) -> AnyResult<NewVaultEnvelope> {
        let Self {
            header,
            salt,
            dek,
            audit_key: _,
            audit_root,
        } = self;
        let v3 = match (audit_root, mutation_audit_mac) {
            (Some(audit_root), Some(mutation_audit_mac)) => Some(V3StateFields {
                audit_root,
                generation: 1,
                mutation_audit_mac,
            }),
            (None, None) => None,
            _ => {
                return Err(classified(
                    VaultErrorKind::Internal,
                    "new vault mutation anchor does not match its format",
                ));
            }
        };
        let state = VaultState {
            secrets: Default::default(),
            v3,
        };
        let wrap_key = derive_wrap_key(passphrase, &salt, &header.kdf)?;
        let (file, state_plaintext) =
            seal_payloads(&header, &wrap_key, &dek, &state)?.into_file(header);
        let file_text = serde_json::to_string_pretty(&file)?;
        Ok(NewVaultEnvelope {
            file_text,
            _state_plaintext: state_plaintext,
            _wrap_key: wrap_key,
            _dek: dek,
        })
    }
}

impl ResealedVaultEnvelope {
    pub(in crate::vault) fn seal(
        previous: &VaultFile,
        dek: &[u8; KEY_LEN],
        state: &VaultState,
    ) -> AnyResult<Self> {
        // Keep the immutable header fields that were validated at open/init
        // time. A version 3 save adopts the state's new generation, which
        // only the state AAD authenticates, so the wrapped DEK is retained.
        let header = header_for_state(previous.header.clone(), state, "resealed")?;
        if header.version != previous.header.version {
            return Err(classified(
                VaultErrorKind::Internal,
                "an ordinary vault save cannot change the format version",
            ));
        }
        let (state_nonce_b64, state_b64, state_plaintext) = seal_state(&header, dek, state)?;
        let file = VaultFile {
            header,
            wrapped_dek_nonce_b64: previous.wrapped_dek_nonce_b64.clone(),
            wrapped_dek_b64: previous.wrapped_dek_b64.clone(),
            state_nonce_b64,
            state_b64,
        };
        Ok(Self {
            file,
            _state_plaintext: state_plaintext,
        })
    }

    pub(in crate::vault) fn serialize_pretty(&self) -> AnyResult<String> {
        Ok(serde_json::to_string_pretty(&self.file)?)
    }
}

impl MigratedVaultEnvelope {
    /// Rewraps the unchanged DEK and reseals `state` under the target
    /// version's AAD. `state` must already carry the target's schema fields.
    pub(in crate::vault) fn migrate(
        previous: &VaultFile,
        passphrase: &SecretString,
        dek: &[u8; KEY_LEN],
        state: &VaultState,
        target_version: u32,
    ) -> AnyResult<Self> {
        if target_version <= previous.header.version || target_version > LATEST_FORMAT_VERSION {
            anyhow::bail!(
                "vault format {} cannot be migrated to {target_version}",
                previous.header.version
            );
        }
        let mut header = previous.header.clone();
        header.version = target_version;
        let header = header_for_state(header, state, "migrated")?;
        let wrap_key = derive_header_wrap_key(passphrase, &header)?;
        let (file, state_plaintext) =
            seal_payloads(&header, &wrap_key, dek, state)?.into_file(header);
        Ok(Self {
            file,
            _state_plaintext: state_plaintext,
            _wrap_key: wrap_key,
        })
    }

    pub(in crate::vault) fn serialize_pretty(&self) -> AnyResult<String> {
        Ok(serde_json::to_string_pretty(&self.file)?)
    }
}

impl RekeyedVaultEnvelope {
    /// `legacy_dek` is rewrapped only under the frozen format 2 contract; a
    /// format that rotates its DEK never reuses it.
    pub(in crate::vault) fn seal(
        previous: &VaultFile,
        new_passphrase: &SecretString,
        legacy_dek: &[u8; KEY_LEN],
        state: &VaultState,
        kdf: KdfParams,
    ) -> AnyResult<Self> {
        if !supports_field_kinds(previous.header.version) {
            anyhow::bail!(
                "vault format {} does not support passphrase change; run `jig vault migrate --to {LATEST_FORMAT_VERSION}` first",
                previous.header.version
            );
        }
        if rotates_dek_on_rekey(previous.header.version) {
            let rotated = seal_under_fresh_key(
                previous.header.clone(),
                new_passphrase,
                state,
                kdf,
                "rekeyed",
            )?;
            return Ok(Self {
                file: rotated.file,
                _state_plaintext: rotated.state_plaintext,
                _wrap_key: rotated.wrap_key,
                _rotated_dek: Some(rotated.dek),
            });
        }

        let salt = random_array::<SALT_LEN>()?;
        let mut header = previous.header.clone();
        // A passphrase change is the deliberate point where an older valid
        // envelope adopts the current KDF policy. Identity and creation time
        // stay stable; cost parameters do not remain pinned to legacy values.
        header.kdf = kdf;
        header.salt_b64 = B64.encode(salt);
        let header = header_for_state(header, state, "rekeyed")?;

        let wrap_key = derive_wrap_key(new_passphrase, &salt, &header.kdf).map_err(|error| {
            classify_source(
                VaultErrorKind::InvalidInput,
                "new vault passphrase could not be derived safely",
                error,
            )
        })?;
        let (file, state_plaintext) =
            seal_payloads(&header, &wrap_key, legacy_dek, state)?.into_file(header);
        Ok(Self {
            file,
            _state_plaintext: state_plaintext,
            _wrap_key: wrap_key,
            _rotated_dek: None,
        })
    }

    pub(in crate::vault) fn serialize_pretty(&self) -> AnyResult<String> {
        Ok(serde_json::to_string_pretty(&self.file)?)
    }
}

/// A format 3 envelope resealed under a fresh DEK, salt, wrapping key, and
/// nonces, keeping the header's vault ID and creation time. The audit root
/// travels in the state, so audit verification is unaffected; ciphertext
/// sealed under any earlier DEK stays decryptable by that DEK.
pub(in crate::vault) struct RotatedVaultEnvelope {
    file: VaultFile,
    // Keep secret-bearing intermediates zeroizing until the serialized
    // envelope has been persisted.
    _state_plaintext: Zeroizing<Vec<u8>>,
    _wrap_key: Zeroizing<[u8; KEY_LEN]>,
    _dek: Zeroizing<[u8; KEY_LEN]>,
}

impl RotatedVaultEnvelope {
    pub(in crate::vault) fn seal(
        previous: &VaultHeader,
        passphrase: &SecretString,
        state: &VaultState,
        kdf: KdfParams,
    ) -> AnyResult<Self> {
        let mut header = previous.clone();
        header.version = V3_FORMAT_VERSION;
        let rotated = seal_under_fresh_key(header, passphrase, state, kdf, "resealed format 3")?;
        Ok(Self {
            file: rotated.file,
            _state_plaintext: rotated.state_plaintext,
            _wrap_key: rotated.wrap_key,
            _dek: rotated.dek,
        })
    }

    pub(in crate::vault) fn serialize_pretty(&self) -> AnyResult<String> {
        Ok(serde_json::to_string_pretty(&self.file)?)
    }
}
