use std::time::Instant;

use super::*;
use crate::runtime::loops::renewal::{RenewalAttemptError, RenewalOwnershipLost};

impl LeaseStore {
    pub(super) fn renew(
        &mut self,
        key: &str,
        owner: &str,
        ttl_seconds: u64,
    ) -> Result<LeaseRecord> {
        self.renew_with_lock_deadline(key, owner, ttl_seconds, loop_state_lock_deadline(), now_ms)
    }

    pub(super) fn renew_for_guard(
        &mut self,
        key: &str,
        owner: &str,
        ttl_seconds: u64,
        deadline: Instant,
    ) -> std::result::Result<LeaseRecord, RenewalAttemptError> {
        self.renew_with_lock_deadline(key, owner, ttl_seconds, deadline, now_ms)
            .map_err(|error| {
                if error.downcast_ref::<RenewalOwnershipLost>().is_some() {
                    RenewalAttemptError::Terminal(error)
                } else {
                    RenewalAttemptError::Retryable(error)
                }
            })
    }

    #[cfg(test)]
    pub(super) fn renew_at(
        &mut self,
        key: &str,
        owner: &str,
        ttl_seconds: u64,
        now: u64,
    ) -> Result<LeaseRecord> {
        self.renew_with_lock_deadline(key, owner, ttl_seconds, loop_state_lock_deadline(), || now)
    }

    fn renew_with_lock_deadline(
        &mut self,
        key: &str,
        owner: &str,
        ttl_seconds: u64,
        deadline: Instant,
        now: impl FnOnce() -> u64,
    ) -> Result<LeaseRecord> {
        self.persistence
            .with_locked_until(deadline, |store: &mut LeaseFile| {
                let now = now();
                let lease = store.leases.get_mut(key).ok_or_else(|| {
                    RenewalOwnershipLost::new(format!("Loop lease is no longer held: {key}"))
                })?;
                if lease.owner != owner {
                    return Err(RenewalOwnershipLost::new(format!(
                        "Loop lease '{key}' is owned by another worker"
                    ))
                    .into());
                }
                if lease.expires_at_ms <= now {
                    return Err(RenewalOwnershipLost::new(format!(
                        "Loop lease expired before renewal: {key}"
                    ))
                    .into());
                }
                lease.expires_at_ms = now.saturating_add(ttl_seconds.saturating_mul(1_000));
                Ok(lease.clone())
            })
    }
}
