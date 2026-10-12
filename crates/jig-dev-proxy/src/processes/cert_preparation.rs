use anyhow::{Context, Result, bail};

use crate::certs;
use crate::state::LockOutcome;
use crate::types::ProxySettings;

use super::{TerminationReason, lock_outcome_or_interruption, proxy_ready_interruptible};

pub(super) fn prepare_certs_for_hosts_interruptible(
    settings: &ProxySettings,
    hostnames: &[String],
    interrupt_requested: &impl Fn() -> Option<TerminationReason>,
) -> Result<()> {
    if !settings.https {
        return Ok(());
    }
    let cancelled = || interrupt_requested().is_some();
    // Run under the certificate lock: a delayed lock acquisition must not use
    // an earlier readiness observation as authority to alter shared TLS state.
    let outcome = certs::ensure_for_hosts_after_check_interruptible(settings, hostnames, &cancelled, |store| {
        match proxy_ready_interruptible(store, settings, &cancelled)? {
            LockOutcome::Acquired(true) => Ok(LockOutcome::Acquired(())),
            LockOutcome::Cancelled => Ok(LockOutcome::Cancelled),
            LockOutcome::Acquired(false) => bail!(
                "Jig proxy readiness in state dir {} is unconfirmed. Retry after the proxy stabilizes; shared certificates were preserved.",
                store.root().display(),
            ),
        }
    })
        .context("Failed to prepare app HTTPS certificates; inspect the readiness or certificate error before retrying")?;
    lock_outcome_or_interruption(outcome, interrupt_requested)?;
    Ok(())
}

#[cfg(all(test, unix))]
pub(super) fn prepare_certs_for_hosts(
    settings: &ProxySettings,
    hostnames: &[String],
) -> Result<()> {
    certs::ensure_for_hosts(settings, hostnames).map(|_| ())
}
