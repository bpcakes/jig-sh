use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{Result, VaultError, VaultErrorKind};

const VAULT_REFERENCE_PREFIX: &str = "jig://";
const MAX_VAULT_REFERENCE_SEGMENT_LEN: usize = 64;

/// Handling policy for an encrypted vault field.
///
/// Both variants are encrypted at rest. `Concealed` values participate in
/// output redaction; `Text` values remain encrypted but do not become
/// redaction needles for ordinary command output.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    /// An encrypted value that must be considered secret when rendering output.
    #[default]
    Concealed,
    /// An encrypted contextual value that must not redact unrelated output.
    Text,
}

impl FieldKind {
    /// Stable metadata label used in audit and user-facing records.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Concealed => "concealed",
            Self::Text => "text",
        }
    }
}

/// Canonical project-local vault item selector.
///
/// This is the single-segment companion to [`VaultReference`], useful when a
/// caller needs to list all fields in one item without inventing a fake field
/// name.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct VaultItem(String);

impl VaultItem {
    /// Parses one canonical `jig://ITEM` item selector.
    ///
    /// # Errors
    ///
    /// Returns an error when the selector contains URI features outside the
    /// canonical local form or has an invalid item label.
    pub fn parse(selector: &str) -> Result<Self> {
        let Some(item) = selector.strip_prefix(VAULT_REFERENCE_PREFIX) else {
            return Err(VaultError::new(
                VaultErrorKind::InvalidInput,
                "vault item selector must use the canonical form jig://ITEM",
            ));
        };
        if item
            .bytes()
            .any(|byte| matches!(byte, b'/' | b'%' | b'?' | b'#' | b'@' | b':'))
        {
            return Err(VaultError::new(
                VaultErrorKind::InvalidInput,
                "vault item selector must contain exactly one item segment without URI encoding, queries, fragments, credentials, or ports",
            ));
        }
        validate_vault_reference_segment("item", item)?;
        Ok(Self(item.to_owned()))
    }

    /// Returns the non-secret item label.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for VaultItem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{VAULT_REFERENCE_PREFIX}{}", self.0)
    }
}

impl FromStr for VaultItem {
    type Err = VaultError;

    fn from_str(value: &str) -> Result<Self> {
        Self::parse(value)
    }
}

/// Canonical project-local vault field reference.
///
/// The selected vault supplies the project context, so references always use
/// the exact `jig://ITEM/FIELD` spelling and never include a project name.
/// Item and field labels are vault metadata, not filesystem paths.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct VaultReference {
    item: String,
    field: String,
}

impl VaultReference {
    /// Parses one canonical `jig://ITEM/FIELD` vault reference.
    ///
    /// # Errors
    ///
    /// Returns an error when the reference has an unsupported URI component,
    /// does not have exactly two nonempty segments, or has invalid metadata
    /// labels.
    pub fn parse(reference: &str) -> Result<Self> {
        let Some(path) = reference.strip_prefix(VAULT_REFERENCE_PREFIX) else {
            return Err(VaultError::new(
                VaultErrorKind::InvalidInput,
                "vault reference must use the canonical form jig://ITEM/FIELD",
            ));
        };

        if path
            .bytes()
            .any(|byte| matches!(byte, b'%' | b'?' | b'#' | b'@' | b':'))
        {
            return Err(VaultError::new(
                VaultErrorKind::InvalidInput,
                "vault reference must not contain percent encoding, queries, fragments, credentials, or ports",
            ));
        }

        let mut segments = path.split('/');
        let Some(item) = segments.next() else {
            unreachable!("split always yields at least one segment");
        };
        let Some(field) = segments.next() else {
            return Err(VaultError::new(
                VaultErrorKind::InvalidInput,
                "vault reference must have exactly an item and field segment",
            ));
        };
        if segments.next().is_some() {
            return Err(VaultError::new(
                VaultErrorKind::InvalidInput,
                "vault reference must have exactly an item and field segment",
            ));
        }

        validate_vault_reference_segment("item", item)?;
        validate_vault_reference_segment("field", field)?;
        let Some(secret_name_len) = item.len().checked_add(1 + field.len()) else {
            return Err(VaultError::new(
                VaultErrorKind::InvalidInput,
                "vault reference is too long",
            ));
        };
        if secret_name_len > 128 {
            return Err(VaultError::new(
                VaultErrorKind::InvalidInput,
                "vault reference is too long",
            ));
        }

        Ok(Self {
            item: item.to_owned(),
            field: field.to_owned(),
        })
    }

    /// Returns the non-secret item label.
    pub fn item(&self) -> &str {
        &self.item
    }

    /// Returns the non-secret field label.
    pub fn field(&self) -> &str {
        &self.field
    }

    /// Maps this reference to the compatible internal secret-map key.
    pub fn to_secret_name(&self) -> SecretName {
        // `parse` validates the combined length and alphabet, so this cannot
        // fail. Keep the assertion to protect this invariant if validation is
        // changed in the future.
        let name = format!("{}/{}", self.item, self.field);
        debug_assert!(SecretName::parse(&name).is_ok());
        SecretName(name)
    }

    /// Converts a compatible legacy map key into a canonical reference.
    ///
    /// Legacy secret names may have more or fewer path segments; those names
    /// deliberately return `None` instead of becoming invented field
    /// references.
    pub(crate) fn from_secret_name(name: &SecretName) -> Option<Self> {
        Self::parse(&format!("{VAULT_REFERENCE_PREFIX}{name}")).ok()
    }
}

impl fmt::Display for VaultReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{VAULT_REFERENCE_PREFIX}{}/{}",
            self.item, self.field
        )
    }
}

impl FromStr for VaultReference {
    type Err = VaultError;

    fn from_str(value: &str) -> Result<Self> {
        Self::parse(value)
    }
}

fn validate_vault_reference_segment(label: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(VaultError::new(
            VaultErrorKind::InvalidInput,
            format!("vault reference {label} segment must not be empty"),
        ));
    }
    if value.len() > MAX_VAULT_REFERENCE_SEGMENT_LEN {
        return Err(VaultError::new(
            VaultErrorKind::InvalidInput,
            format!("vault reference {label} segment is too long"),
        ));
    }
    if matches!(value, "." | "..") {
        return Err(VaultError::new(
            VaultErrorKind::InvalidInput,
            format!("vault reference {label} segment must not be '.' or '..'"),
        ));
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(VaultError::new(
            VaultErrorKind::InvalidInput,
            format!(
                "vault reference {label} segment contains unsupported characters; use letters, digits, '_', '-', or '.'"
            ),
        ));
    }
    Ok(())
}

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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{FieldKind, SecretName, VaultItem, VaultReference};

    #[test]
    fn vault_reference_accepts_and_canonicalizes_project_local_form() {
        let reference = VaultReference::from_str("jig://Production/RESTIC_PASSWORD").unwrap();

        assert_eq!(reference.item(), "Production");
        assert_eq!(reference.field(), "RESTIC_PASSWORD");
        assert_eq!(reference.to_string(), "jig://Production/RESTIC_PASSWORD");
        assert_eq!(
            reference.to_secret_name().as_str(),
            "Production/RESTIC_PASSWORD"
        );
        assert_eq!(
            VaultReference::from_secret_name(
                &SecretName::parse("Production/RESTIC_PASSWORD").unwrap()
            )
            .unwrap(),
            reference
        );
    }

    #[test]
    fn vault_reference_rejects_ambiguous_or_unsupported_forms() {
        for reference in [
            "",
            "jig:/Production/RESTIC_PASSWORD",
            "JIG://Production/RESTIC_PASSWORD",
            "jig://",
            "jig://Production",
            "jig:///RESTIC_PASSWORD",
            "jig://Production/",
            "jig://Production/RESTIC_PASSWORD/extra",
            "jig://Production//RESTIC_PASSWORD",
            "jig://Production/RESTIC_PASSWORD?query",
            "jig://Production/RESTIC_PASSWORD#fragment",
            "jig://Production/RESTIC%5FPASSWORD",
            "jig://user@Production/RESTIC_PASSWORD",
            "jig://Production:443/RESTIC_PASSWORD",
            "jig://./RESTIC_PASSWORD",
            "jig://../RESTIC_PASSWORD",
            "jig://Production/..",
            "jig://Production/has space",
            "jig://Pröd/RESTIC_PASSWORD",
        ] {
            assert!(
                VaultReference::from_str(reference).is_err(),
                "expected {reference:?} to be rejected"
            );
        }
    }

    #[test]
    fn vault_reference_enforces_segment_and_compatible_key_lengths() {
        let long_segment = "a".repeat(65);
        assert!(VaultReference::from_str(&format!("jig://{long_segment}/FIELD")).is_err());
        assert!(VaultReference::from_str(&format!("jig://ITEM/{long_segment}")).is_err());

        let item = "i".repeat(64);
        let field = "f".repeat(64);
        assert!(VaultReference::from_str(&format!("jig://{item}/{field}")).is_err());
    }

    #[test]
    fn vault_item_is_the_single_segment_companion_to_references() {
        let item = VaultItem::from_str("jig://Production").unwrap();
        assert_eq!(item.as_str(), "Production");
        assert_eq!(item.to_string(), "jig://Production");

        for selector in [
            "Production",
            "jig://",
            "jig://Production/RESTIC_PASSWORD",
            "jig://Production?query",
            "jig://Production%20One",
            "jig://.",
        ] {
            assert!(
                VaultItem::from_str(selector).is_err(),
                "expected {selector:?} to be rejected"
            );
        }
    }

    #[test]
    fn field_kind_labels_are_stable_and_non_persistent_plaintext_is_not_implied() {
        assert_eq!(FieldKind::Concealed.as_str(), "concealed");
        assert_eq!(FieldKind::Text.as_str(), "text");
    }
}
