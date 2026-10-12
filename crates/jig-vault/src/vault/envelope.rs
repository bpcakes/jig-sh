use anyhow::Result as AnyResult;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use secrecy::SecretString;
use zeroize::Zeroizing;

use crate::crypto::{
    KEY_LEN, NONCE_LEN, SALT_LEN, decode_array, derive_audit_key, derive_wrap_key, open,
};
use crate::error::{VaultErrorKind, classified, classify_source};
use crate::format::{
    AeadRole, VaultFile, VaultState, decode_b64_array, payload_aad, validate_header,
};

mod seal;

pub(super) use seal::{
    MigratedVaultEnvelope, NewVaultMaterial, RekeyedVaultEnvelope, ResealedVaultEnvelope,
    RotatedVaultEnvelope,
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

    /// The unauthenticated public header, for lock and witness lookups only.
    pub(super) fn into_header(self) -> crate::format::VaultHeader {
        self.file.header
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
                    error,
                )
            })?;
        let audit_key = match &state.v3 {
            // The generation is authenticated twice: by the state AAD and
            // inside the encrypted state. Both must name the same value.
            Some(fields) if file.header.generation != Some(fields.generation) => {
                return Err(classified(
                    VaultErrorKind::Serialization,
                    "vault state generation does not match its authenticated header",
                ));
            }
            Some(fields) => Zeroizing::new(*fields.audit_root.as_bytes()),
            None => derive_audit_key(&dek).map_err(|error| {
                classify_source(
                    VaultErrorKind::Internal,
                    "failed to derive vault audit key",
                    error,
                )
            })?,
        };

        Ok(UnlockedVaultEnvelope {
            file,
            state,
            dek,
            audit_key,
        })
    }
}
