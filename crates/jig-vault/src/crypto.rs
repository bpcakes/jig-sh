use anyhow::{Context, Result, anyhow};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use hkdf::Hkdf;
use secrecy::{ExposeSecret, SecretString};
use sha2::Sha256;
use zeroize::Zeroizing;

pub(crate) const KEY_LEN: usize = 32;
pub(crate) const NONCE_LEN: usize = 24;
pub(crate) const SALT_LEN: usize = 16;
const MIN_ARGON2_MEMORY_KIB: u32 = 19_456;
const MAX_ARGON2_MEMORY_KIB: u32 = 524_288;
const MAX_ARGON2_ITERATIONS: u32 = 10;
const MAX_ARGON2_PARALLELISM: u32 = 16;

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub(crate) struct KdfParams {
    pub(crate) algorithm: String,
    pub(crate) memory_kib: u32,
    pub(crate) iterations: u32,
    pub(crate) parallelism: u32,
    pub(crate) output_len: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self::production()
    }
}

impl KdfParams {
    pub(crate) fn production() -> Self {
        Self {
            algorithm: "argon2id".into(),
            memory_kib: 131_072,
            iterations: 3,
            parallelism: 4,
            output_len: KEY_LEN as u32,
        }
    }

    #[cfg(any(test, feature = "test-utils"))]
    pub(crate) fn for_tests() -> Self {
        Self {
            algorithm: "argon2id".into(),
            memory_kib: MIN_ARGON2_MEMORY_KIB,
            iterations: 1,
            parallelism: 1,
            output_len: KEY_LEN as u32,
        }
    }
}

pub(crate) fn random_array<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0_u8; N];
    getrandom::fill(&mut bytes).context("secure random generation failed")?;
    Ok(bytes)
}

pub(crate) fn derive_wrap_key(
    passphrase: &SecretString,
    salt: &[u8],
    params: &KdfParams,
) -> Result<Zeroizing<[u8; KEY_LEN]>> {
    if params.algorithm != "argon2id" {
        return Err(anyhow!("unsupported vault KDF '{}'", params.algorithm));
    }
    if params.output_len != KEY_LEN as u32 {
        return Err(anyhow!(
            "unsupported vault KDF output length {}; expected {KEY_LEN}",
            params.output_len
        ));
    }
    if !(MIN_ARGON2_MEMORY_KIB..=MAX_ARGON2_MEMORY_KIB).contains(&params.memory_kib) {
        return Err(anyhow!(
            "unsupported vault Argon2id memory cost {}; expected {MIN_ARGON2_MEMORY_KIB}..={MAX_ARGON2_MEMORY_KIB} KiB",
            params.memory_kib
        ));
    }
    if params.iterations == 0 || params.iterations > MAX_ARGON2_ITERATIONS {
        return Err(anyhow!(
            "unsupported vault Argon2id iterations {}; expected 1..={MAX_ARGON2_ITERATIONS}",
            params.iterations
        ));
    }
    if params.parallelism == 0 || params.parallelism > MAX_ARGON2_PARALLELISM {
        return Err(anyhow!(
            "unsupported vault Argon2id parallelism {}; expected 1..={MAX_ARGON2_PARALLELISM}",
            params.parallelism
        ));
    }
    let argon_params = Params::new(
        params.memory_kib,
        params.iterations,
        params.parallelism,
        Some(KEY_LEN),
    )
    .map_err(|error| anyhow!("invalid Argon2id vault parameters: {error}"))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params);
    let mut key = Zeroizing::new([0_u8; KEY_LEN]);
    argon2
        .hash_password_into(passphrase.expose_secret().as_bytes(), salt, key.as_mut())
        .map_err(|error| anyhow!("failed to derive vault wrap key: {error}"))?;
    Ok(key)
}

pub(crate) fn seal(
    key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new_from_slice(key).context("invalid vault key length")?;
    cipher
        .encrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        // The AEAD backend intentionally exposes only opaque failures here.
        .map_err(|_| anyhow!("vault encryption failed"))
}

pub(crate) fn open(
    key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Zeroizing<Vec<u8>>> {
    let cipher = XChaCha20Poly1305::new_from_slice(key).context("invalid vault key length")?;
    let plaintext = cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        // Authentication failures must stay coarse so passphrase and tamper
        // failures are not distinguishable through lower-level diagnostics.
        .map_err(|_| anyhow!("vault authentication failed"))?;
    Ok(Zeroizing::new(plaintext))
}

pub(crate) fn derive_audit_key(dek: &[u8; KEY_LEN]) -> Result<Zeroizing<[u8; KEY_LEN]>> {
    let hkdf = Hkdf::<Sha256>::new(None, dek);
    let mut key = Zeroizing::new([0_u8; KEY_LEN]);
    hkdf.expand(b"jig-vault audit v1", key.as_mut())
        .map_err(|error| anyhow!("failed to derive vault audit key: {error}"))?;
    Ok(key)
}

pub(crate) fn decode_array<const N: usize>(label: &str, bytes: &[u8]) -> Result<[u8; N]> {
    bytes.try_into().map_err(|error| {
        anyhow!(
            "{label} has invalid length {}; expected {N}: {error}",
            bytes.len()
        )
    })
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;

    use super::*;

    #[test]
    fn production_and_test_kdf_parameters_are_explicit_and_valid() {
        let production = KdfParams::default();
        assert_eq!(production, KdfParams::production());
        assert_eq!(production.memory_kib, 131_072);
        assert_eq!(production.iterations, 3);
        assert_eq!(production.parallelism, 4);

        let test = KdfParams::for_tests();
        assert_eq!(test.memory_kib, MIN_ARGON2_MEMORY_KIB);
        assert_eq!(test.iterations, 1);
        assert_eq!(test.parallelism, 1);
        Params::new(
            test.memory_kib,
            test.iterations,
            test.parallelism,
            Some(test.output_len as usize),
        )
        .unwrap();
    }

    #[test]
    fn rejects_absurd_kdf_memory_before_argon2_allocation() {
        let params = KdfParams {
            memory_kib: MAX_ARGON2_MEMORY_KIB + 1,
            ..KdfParams::default()
        };
        let error = derive_wrap_key(
            &SecretString::from("passphrase".to_string()),
            &[1_u8; 16],
            &params,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("memory cost"));
    }

    #[test]
    fn rejects_zero_kdf_iterations_before_argon2() {
        let params = KdfParams {
            iterations: 0,
            ..KdfParams::default()
        };
        let error = derive_wrap_key(
            &SecretString::from("passphrase".to_string()),
            &[1_u8; 16],
            &params,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("iterations"));
    }

    #[test]
    fn rejects_too_small_kdf_memory_before_argon2() {
        let params = KdfParams {
            memory_kib: MIN_ARGON2_MEMORY_KIB - 1,
            ..KdfParams::default()
        };
        let error = derive_wrap_key(
            &SecretString::from("passphrase".to_string()),
            &[1_u8; 16],
            &params,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("memory cost"));
    }
}
