use anyhow::{Context, Result};

use crate::certs;
use crate::types::ProxySettings;

use super::{TerminationReason, lock_outcome_or_interruption};

pub(super) fn prepare_certs_for_hosts_interruptible(
    settings: &ProxySettings,
    hostnames: &[String],
    interrupt_requested: &impl Fn() -> Option<TerminationReason>,
) -> Result<()> {
    if !settings.https {
        return Ok(());
    }
    let cancelled = || interrupt_requested().is_some();
    let outcome = certs::ensure_for_hosts_interruptible(settings, hostnames, &cancelled)
        .with_context(|| {
            "Failed to prepare HTTPS proxy certificates. Likely fix: run `scripts/jig proxy cert generate --force`, trust the CA with `scripts/jig proxy cert trust --accept-trust-scope`, or disable [dev].https for HTTP-only local development."
        })?;
    lock_outcome_or_interruption(outcome, interrupt_requested)?;
    Ok(())
}

#[cfg(all(test, unix))]
pub(super) fn prepare_certs_for_hosts(
    settings: &ProxySettings,
    hostnames: &[String],
) -> Result<()> {
    prepare_certs_for_hosts_interruptible(settings, hostnames, &|| None)
}
