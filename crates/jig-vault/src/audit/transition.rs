//! Audit support for the witnessed transaction protocol: the exact audit
//! transition a pending transaction records, verified prefix checks during
//! recovery, and the lookup of a state's mutation anchor.

use anyhow::{Context, Result, bail};

use crate::store::VaultStore;
use crate::store::witness::AuditTransition;

use super::{AuditEvent, AuditVerification, PreparedAuditAppend, verify_chain_text};

impl PreparedAuditAppend {
    /// The exact audit change this append makes: keep the verified prefix,
    /// drop any recoverable torn suffix, and append the prepared line.
    pub(crate) fn transition(&self) -> AuditTransition {
        let mut append = String::with_capacity(self.line.len() + 2);
        if self.needs_separator {
            append.push('\n');
        }
        append.push_str(&self.line);
        append.push('\n');
        AuditTransition {
            prefix_len: self.valid_len as u64,
            prefix_tip_mac: self.event.previous_mac.clone(),
            torn_suffix_len: (self.audit_len - self.valid_len) as u64,
            torn_suffix_sha256: self.torn_suffix_sha256.clone(),
            append,
        }
    }
}

/// Verifies that `prefix` is a complete authenticated chain with no torn
/// tail and returns its latest MAC.
pub(crate) fn verify_exact_prefix(prefix: &[u8], audit_key: &[u8]) -> Result<Option<String>> {
    let text = std::str::from_utf8(prefix).context("vault audit prefix is not valid UTF-8")?;
    let verified = verify_chain_text(text, audit_key)?;
    if verified.valid_len != text.len() {
        bail!("vault audit prefix ends with an unverified partial event");
    }
    Ok(verified.verification.latest_mac)
}

/// Verifies the whole chain and returns the event whose MAC is `mac`, if any.
pub(crate) fn find_verified_event_unlocked(
    store: &VaultStore,
    audit_key: &[u8],
    mac: &str,
) -> Result<(AuditVerification, Option<AuditEvent>)> {
    let text = store
        .read_audit_text()?
        .context("vault audit log is missing; its mutation anchor cannot be verified")?;
    let verified = verify_chain_text(&text, audit_key)?;
    let mut anchor = None;
    for line in text[..verified.valid_len].lines() {
        let event: AuditEvent =
            serde_json::from_str(line).context("failed to re-read a verified vault audit event")?;
        if event.mac == mac {
            anchor = Some(event);
            break;
        }
    }
    Ok((verified.verification, anchor))
}
