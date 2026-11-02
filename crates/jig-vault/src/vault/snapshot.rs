use std::{collections::BTreeMap, fmt};

use secrecy::SecretString;
use zeroize::Zeroizing;

use super::{Vault, VaultSnapshot};
use crate::error::{ClassifiedVaultError, classify_source, vault_error_from_anyhow};
use crate::store::VaultStore;
use crate::{FieldKind, Result, VaultErrorKind, VaultReference};

#[derive(Clone, Default, Eq, PartialEq)]
pub(super) struct TextValues(BTreeMap<VaultReference, Zeroizing<String>>);

impl fmt::Debug for TextValues {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[TEXT VALUES REDACTED]")
    }
}

impl VaultSnapshot {
    /// Bounded, terminal-escaped text for interactive display. Concealed and
    /// legacy values are never included, nor are values in metadata snapshots.
    pub fn text_value(&self, reference: &VaultReference) -> Option<&str> {
        self.text_values.0.get(reference).map(|text| text.as_str())
    }
}

impl Vault {
    /// Authenticates metadata and non-secret text previews under one lock.
    /// Values remain encrypted on disk and absent from Debug output.
    pub fn display_snapshot(&self, passphrase: &SecretString) -> Result<VaultSnapshot> {
        self.store.snapshot_with_text(passphrase, true)
    }
}

impl VaultStore {
    pub(crate) fn snapshot(&self, passphrase: &SecretString) -> Result<VaultSnapshot> {
        self.snapshot_with_text(passphrase, false)
    }

    fn snapshot_with_text(
        &self,
        passphrase: &SecretString,
        include_text: bool,
    ) -> Result<VaultSnapshot> {
        self.with_lock(|| {
            let vault = self.open_unlocked(passphrase)?;
            let audit = vault.verify_audit_unlocked(self).map_err(|error| {
                classify_source(
                    VaultErrorKind::AuditTampered,
                    "vault audit chain verification failed",
                    error,
                )
            })?;
            let mut snapshot = vault.snapshot(audit);
            if include_text {
                for field in &snapshot.fields {
                    if field.kind != FieldKind::Text {
                        continue;
                    }
                    let bytes = vault.secret_value(&field.reference.to_secret_name())?;
                    // Bound display allocation and escape arbitrary bytes before rendering.
                    let mut text = String::new();
                    for character in
                        String::from_utf8_lossy(&bytes.as_slice()[..bytes.len().min(4096)]).chars()
                    {
                        if character.is_control() || character == '\\' {
                            text.extend(character.escape_default());
                        } else {
                            text.push(character);
                        }
                    }
                    if bytes.len() > 4096 {
                        text.push_str("… [truncated]");
                    }
                    snapshot
                        .text_values
                        .0
                        .insert(field.reference.clone(), Zeroizing::new(text));
                }
            }
            Ok(snapshot)
        })
        .map_err(|error| {
            if error.is::<ClassifiedVaultError>() {
                vault_error_from_anyhow(VaultErrorKind::Internal, error)
            } else {
                self.map_open_error(error)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SecretBytes;

    #[test]
    fn display_snapshots_include_only_current_text_and_escape_and_bound_it() {
        for version in [2, 3] {
            let temp = tempfile::tempdir().unwrap();
            let vault = Vault::resolve_for_test(Some(temp.path().join("ExampleVault"))).unwrap();
            let passphrase = SecretString::from("correct horse battery staple".to_owned());
            vault.init_format_for_test(&passphrase, version).unwrap();
            let text: VaultReference = "jig://ExampleProject/URL".parse().unwrap();
            let secret: VaultReference = "jig://ExampleProject/TOKEN".parse().unwrap();
            vault
                .set_field(
                    &passphrase,
                    text.clone(),
                    FieldKind::Text,
                    SecretBytes::new(b"visible\x1b[2J\ntext\\n".to_vec()),
                )
                .unwrap();
            vault
                .set_field(
                    &passphrase,
                    secret.clone(),
                    FieldKind::Concealed,
                    SecretBytes::new(b"concealed-sentinel".to_vec()),
                )
                .unwrap();
            assert_eq!(vault.snapshot(&passphrase).unwrap().text_value(&text), None);
            let snapshot = vault.display_snapshot(&passphrase).unwrap();
            assert_eq!(
                snapshot.text_value(&text),
                Some("visible\\u{1b}[2J\\ntext\\\\n")
            );
            assert_eq!(snapshot.text_value(&secret), None);
            assert!(!format!("{snapshot:?}").contains("visible"));
            assert!(!format!("{snapshot:?}").contains("concealed-sentinel"));
            vault
                .set_field(
                    &passphrase,
                    text.clone(),
                    FieldKind::Concealed,
                    SecretBytes::new(b"now-secret".to_vec()),
                )
                .unwrap();
            assert_eq!(
                vault
                    .display_snapshot(&passphrase)
                    .unwrap()
                    .text_value(&text),
                None
            );
            vault
                .set_field(
                    &passphrase,
                    text.clone(),
                    FieldKind::Text,
                    SecretBytes::new(vec![b'x'; 5000]),
                )
                .unwrap();
            let snapshot = vault.display_snapshot(&passphrase).unwrap();
            assert_eq!(
                snapshot.text_value(&text).unwrap(),
                format!("{}… [truncated]", "x".repeat(4096))
            );
            vault
                .set_field(
                    &passphrase,
                    text.clone(),
                    FieldKind::Text,
                    SecretBytes::new(Vec::new()),
                )
                .unwrap();
            assert_eq!(
                vault
                    .display_snapshot(&passphrase)
                    .unwrap()
                    .text_value(&text),
                Some("")
            );
        }
    }
}
