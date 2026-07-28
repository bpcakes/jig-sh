use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{Result, VaultError, VaultErrorKind};

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
/// Validated vault secret name.
///
/// Names may be path-shaped labels containing `/`, `.`, and even `..`, but
/// they are only valid as vault map keys and audit metadata. Do not join a
/// `SecretName` into a filesystem path without defining a separate path-safe
/// encoding first.
pub struct SecretName(String);

impl SecretName {
    /// Parse a secret name for vault lookup and audit metadata.
    ///
    /// This permits path-like labels for operator organization. The returned
    /// value is not a filesystem-safe path component.
    ///
    /// # Errors
    ///
    /// Returns an error when the name is empty, exceeds 128 bytes, or contains
    /// characters outside the supported metadata alphabet.
    pub fn parse(name: &str) -> Result<Self> {
        if name.is_empty() {
            return Err(VaultError::new(
                VaultErrorKind::InvalidInput,
                "secret name must not be empty",
            ));
        }
        if name.len() > 128 {
            return Err(VaultError::new(
                VaultErrorKind::InvalidInput,
                format!("secret name '{name}' is too long"),
            ));
        }
        // Path-shaped labels are allowed because secret names are used only as
        // map keys and audit metadata, never as filesystem paths.
        if !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/'))
        {
            return Err(VaultError::new(
                VaultErrorKind::InvalidInput,
                format!(
                    "secret name '{name}' contains unsupported characters; use letters, digits, '_', '-', '.', or '/'"
                ),
            ));
        }
        Ok(Self(name.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SecretName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl TryFrom<&str> for SecretName {
    type Error = VaultError;

    fn try_from(value: &str) -> Result<Self> {
        Self::parse(value)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct EnvVarName(String);

impl EnvVarName {
    /// Parses a portable environment variable name.
    ///
    /// # Errors
    ///
    /// Returns an error when the name is empty, does not start with a letter or
    /// underscore, or contains characters other than letters, digits, and
    /// underscores.
    pub fn parse(name: &str) -> Result<Self> {
        if name.is_empty() {
            return Err(VaultError::new(
                VaultErrorKind::InvalidInput,
                "vault env mapping has an empty environment variable name",
            ));
        }
        let mut bytes = name.bytes();
        let Some(first) = bytes.next() else {
            return Err(VaultError::new(
                VaultErrorKind::InvalidInput,
                "vault env mapping has an empty environment variable name",
            ));
        };
        if !(first.is_ascii_alphabetic() || first == b'_') {
            return Err(VaultError::new(
                VaultErrorKind::InvalidInput,
                format!("environment variable '{name}' must start with a letter or underscore"),
            ));
        }
        if !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') {
            return Err(VaultError::new(
                VaultErrorKind::InvalidInput,
                format!(
                    "environment variable '{name}' may only contain letters, digits, and underscore"
                ),
            ));
        }
        Ok(Self(name.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EnvVarName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl TryFrom<&str> for EnvVarName {
    type Error = VaultError;

    fn try_from(value: &str) -> Result<Self> {
        Self::parse(value)
    }
}
