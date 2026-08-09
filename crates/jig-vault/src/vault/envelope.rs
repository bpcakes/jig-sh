use anyhow::Result as AnyResult;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use secrecy::SecretString;
use zeroize::Zeroizing;

use crate::crypto::{
    KEY_LEN, KdfParams, NONCE_LEN, SALT_LEN, decode_array, derive_audit_key, derive_wrap_key, open,
    random_array, seal,
};
use crate::error::{VaultErrorKind, classify_source};
use crate::format::{
    AEAD_ALGORITHM, AeadRole, FORMAT_VERSION, MAGIC, V1_FORMAT_VERSION, VaultFile, VaultHeader,
    VaultState, decode_b64_array, payload_aad, validate_header,
};

pub(super) struct ParsedVaultEnvelope {
    file: VaultFile,
}

pub(super) struct ValidatedVaultEnvelope {
    file: VaultFile,
    wrapped_dek_aad: Vec<u8>,
    state_aad: Vec<u8>,
}

pub(super) struct UnlockedVaultEnvelope {
    pub(super) file: VaultFile,
    pub(super) state: VaultState,
    pub(super) dek: Zeroizing<[u8; KEY_LEN]>,
    pub(super) audit_key: Zeroizing<[u8; KEY_LEN]>,
}

pub(super) struct NewVaultEnvelope {
    // Keep the sensitive values alive in the same order as the former
    // init_unlocked locals: audit key, serialized encrypted file, encrypted
    // file, state plaintext, wrap key, then DEK.
    pub(super) audit_key: Zeroizing<[u8; KEY_LEN]>,
    pub(super) file_text: String,
    pub(super) file: VaultFile,
    _state_plaintext: Zeroizing<Vec<u8>>,
    _wrap_key: Zeroizing<[u8; KEY_LEN]>,
    _dek: Zeroizing<[u8; KEY_LEN]>,
}

pub(super) struct ResealedVaultEnvelope {
    file: VaultFile,
    // The former save_unlocked local lived through the atomic write. Retain
    // this zeroizing plaintext until the serialized envelope is written too.
    _state_plaintext: Zeroizing<Vec<u8>>,
}

pub(super) struct MigratedVaultEnvelope {
    file: VaultFile,
    // The migration must seal v2 state under v2 AAD before the vault file is
    // written. Keep both secret-bearing intermediate values zeroizing through
    // the atomic write just like ordinary resealing does.
    _state_plaintext: Zeroizing<Vec<u8>>,
    _wrap_key: Zeroizing<[u8; KEY_LEN]>,
}

impl ParsedVaultEnvelope {
    pub(super) fn parse(text: &str) -> AnyResult<Self> {
        let file = serde_json::from_str(text).map_err(|error| {
            classify_source(
                VaultErrorKind::Serialization,
                "failed to parse vault file",
                error.into(),
            )
        })?;
        Ok(Self { file })
    }

    pub(super) fn validate(self) -> AnyResult<ValidatedVaultEnvelope> {
        validate_header(&self.file.header).map_err(|error| {
            classify_source(
                VaultErrorKind::Serialization,
                "vault header is invalid",
                error,
            )
        })?;
        let wrapped_dek_aad = payload_aad(&self.file.header, AeadRole::WrappedDek);
        let state_aad = payload_aad(&self.file.header, AeadRole::State);
        Ok(ValidatedVaultEnvelope {
            file: self.file,
            wrapped_dek_aad,
            state_aad,
        })
    }
}

impl ValidatedVaultEnvelope {
    pub(super) fn unlock(self, passphrase: &SecretString) -> AnyResult<UnlockedVaultEnvelope> {
        let Self {
            file,
            wrapped_dek_aad,
            state_aad,
        } = self;
        let salt =
            decode_b64_array::<SALT_LEN>("vault salt", &file.header.salt_b64).map_err(|error| {
                classify_source(
                    VaultErrorKind::Serialization,
                    "vault salt is invalid",
                    error,
                )
            })?;
        let wrap_key = derive_wrap_key(passphrase, &salt, &file.header.kdf).map_err(|error| {
            classify_source(
                VaultErrorKind::Serialization,
                "vault KDF parameters are invalid",
                error,
            )
        })?;
        let wrapped_dek_nonce =
            decode_b64_array::<NONCE_LEN>("wrapped vault key nonce", &file.wrapped_dek_nonce_b64)
                .map_err(|error| {
                classify_source(
                    VaultErrorKind::Serialization,
                    "wrapped vault key nonce is invalid",
                    error,
                )
            })?;
        let wrapped_dek = B64.decode(&file.wrapped_dek_b64).map_err(|error| {
            classify_source(
                VaultErrorKind::Serialization,
                "wrapped vault key is not valid base64",
                error.into(),
            )
        })?;
        let dek_plaintext = open(
            &wrap_key,
            &wrapped_dek_nonce,
            &wrapped_dek_aad,
            &wrapped_dek,
        )
        .map_err(|error| {
            classify_source(
                VaultErrorKind::Authentication,
                "failed to unlock vault key",
                error,
            )
        })?;
        let dek = Zeroizing::new(
            decode_array::<KEY_LEN>("vault key", &dek_plaintext).map_err(|error| {
                classify_source(
                    VaultErrorKind::Serialization,
                    "vault key has invalid length",
                    error,
                )
            })?,
        );
        let state_nonce = decode_b64_array::<NONCE_LEN>("vault state nonce", &file.state_nonce_b64)
            .map_err(|error| {
                classify_source(
                    VaultErrorKind::Serialization,
                    "vault state nonce is invalid",
                    error,
                )
            })?;
        let state_ciphertext = B64.decode(&file.state_b64).map_err(|error| {
            classify_source(
                VaultErrorKind::Serialization,
                "vault state is not valid base64",
                error.into(),
            )
        })?;
        let state_plaintext =
            open(&dek, &state_nonce, &state_aad, &state_ciphertext).map_err(|error| {
                classify_source(
                    VaultErrorKind::Authentication,
                    "failed to decrypt vault state",
                    error,
                )
            })?;
        let state = VaultState::deserialize_for_version(file.header.version, &state_plaintext)
            .map_err(|error| {
                classify_source(
                    VaultErrorKind::Serialization,
                    "failed to parse vault state",
                    error.into(),
                )
            })?;
        let audit_key = derive_audit_key(&dek).map_err(|error| {
            classify_source(
                VaultErrorKind::Internal,
                "failed to derive vault audit key",
                error,
            )
        })?;

        Ok(UnlockedVaultEnvelope {
            file,
            state,
            dek,
            audit_key,
        })
    }
}

impl NewVaultEnvelope {
    pub(super) fn seal(passphrase: &SecretString, created_at_ms: i128) -> AnyResult<Self> {
        Self::seal_for_version(passphrase, created_at_ms, FORMAT_VERSION)
    }

    #[cfg(test)]
    pub(super) fn seal_v1(passphrase: &SecretString, created_at_ms: i128) -> AnyResult<Self> {
        Self::seal_for_version(passphrase, created_at_ms, V1_FORMAT_VERSION)
    }

    fn seal_for_version(
        passphrase: &SecretString,
        created_at_ms: i128,
        version: u32,
    ) -> AnyResult<Self> {
        let salt = random_array::<SALT_LEN>()?;
        let dek = Zeroizing::new(random_array::<KEY_LEN>()?);
        let header = VaultHeader {
            magic: MAGIC.into(),
            version,
            vault_id: ulid::Ulid::new().to_string(),
            created_at_ms,
            kdf: KdfParams::default(),
            salt_b64: B64.encode(salt),
            aead: AEAD_ALGORITHM.into(),
        };
        validate_header(&header).map_err(|error| {
            classify_source(
                VaultErrorKind::Internal,
                "constructed vault header is invalid",
                error,
            )
        })?;
        let wrapped_dek_aad = payload_aad(&header, AeadRole::WrappedDek);
        let state_aad = payload_aad(&header, AeadRole::State);
        let wrap_key = derive_wrap_key(passphrase, &salt, &header.kdf)?;
        let wrapped_dek_nonce = random_array::<NONCE_LEN>()?;
        let wrapped_dek = seal(
            &wrap_key,
            &wrapped_dek_nonce,
            &wrapped_dek_aad,
            dek.as_ref(),
        )?;
        let state_nonce = random_array::<NONCE_LEN>()?;
        let state_plaintext = Zeroizing::new(VaultState::default().serialize_for_version(version)?);
        let state = seal(&dek, &state_nonce, &state_aad, &state_plaintext)?;
        let file = VaultFile {
            header,
            wrapped_dek_nonce_b64: B64.encode(wrapped_dek_nonce),
            wrapped_dek_b64: B64.encode(wrapped_dek),
            state_nonce_b64: B64.encode(state_nonce),
            state_b64: B64.encode(state),
        };
        let file_text = serde_json::to_string_pretty(&file)?;
        let audit_key = derive_audit_key(&dek)?;
        Ok(Self {
            audit_key,
            file_text,
            file,
            _state_plaintext: state_plaintext,
            _wrap_key: wrap_key,
            _dek: dek,
        })
    }
}

impl ResealedVaultEnvelope {
    pub(super) fn seal(
        previous: &VaultFile,
        dek: &[u8; KEY_LEN],
        state: &VaultState,
    ) -> AnyResult<Self> {
        // Keep the state AAD derived from the immutable, validated header that
        // was parsed at open/init time. Header-changing migrations must update
        // wrapped key and state encryption together.
        let aad = payload_aad(&previous.header, AeadRole::State);
        let state_nonce = random_array::<NONCE_LEN>()?;
        let state_plaintext = Zeroizing::new(state.serialize_for_version(previous.header.version)?);
        let encrypted_state = seal(dek, &state_nonce, &aad, &state_plaintext)?;
        let file = VaultFile {
            header: previous.header.clone(),
            wrapped_dek_nonce_b64: previous.wrapped_dek_nonce_b64.clone(),
            wrapped_dek_b64: previous.wrapped_dek_b64.clone(),
            state_nonce_b64: B64.encode(state_nonce),
            state_b64: B64.encode(encrypted_state),
        };
        Ok(Self {
            file,
            _state_plaintext: state_plaintext,
        })
    }

    pub(super) fn serialize_pretty(&self) -> AnyResult<String> {
        Ok(serde_json::to_string_pretty(&self.file)?)
    }
}

impl MigratedVaultEnvelope {
    pub(super) fn v1_to_v2(
        previous: &VaultFile,
        passphrase: &SecretString,
        dek: &[u8; KEY_LEN],
        state: &VaultState,
    ) -> AnyResult<Self> {
        if previous.header.version != V1_FORMAT_VERSION {
            anyhow::bail!(
                "vault format {} cannot be migrated as a version 1 envelope",
                previous.header.version
            );
        }

        let mut header = previous.header.clone();
        header.version = FORMAT_VERSION;
        validate_header(&header).map_err(|error| {
            classify_source(
                VaultErrorKind::Internal,
                "constructed version 2 vault header is invalid",
                error,
            )
        })?;
        let salt =
            decode_b64_array::<SALT_LEN>("vault salt", &header.salt_b64).map_err(|error| {
                classify_source(
                    VaultErrorKind::Serialization,
                    "vault salt is invalid",
                    error,
                )
            })?;
        let wrap_key = derive_wrap_key(passphrase, &salt, &header.kdf).map_err(|error| {
            classify_source(
                VaultErrorKind::Serialization,
                "vault KDF parameters are invalid",
                error,
            )
        })?;
        let wrapped_dek_nonce = random_array::<NONCE_LEN>()?;
        let wrapped_dek = seal(
            &wrap_key,
            &wrapped_dek_nonce,
            &payload_aad(&header, AeadRole::WrappedDek),
            dek,
        )?;
        let state_nonce = random_array::<NONCE_LEN>()?;
        let state_plaintext = Zeroizing::new(state.serialize_for_version(FORMAT_VERSION)?);
        let state = seal(
            dek,
            &state_nonce,
            &payload_aad(&header, AeadRole::State),
            &state_plaintext,
        )?;
        Ok(Self {
            file: VaultFile {
                header,
                wrapped_dek_nonce_b64: B64.encode(wrapped_dek_nonce),
                wrapped_dek_b64: B64.encode(wrapped_dek),
                state_nonce_b64: B64.encode(state_nonce),
                state_b64: B64.encode(state),
            },
            _state_plaintext: state_plaintext,
            _wrap_key: wrap_key,
        })
    }

    pub(super) fn serialize_pretty(&self) -> AnyResult<String> {
        Ok(serde_json::to_string_pretty(&self.file)?)
    }
}
