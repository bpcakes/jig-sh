use super::*;

pub(crate) fn ensure_for_hosts(settings: &ProxySettings, hostnames: &[String]) -> Result<Value> {
    ensure_certificate_generation_supported()?;
    let store = StateStore::resolve(settings.state_dir.clone())?;
    store.with_cert_lock(|| ensure_for_hosts_locked(&store, settings, hostnames))
}

pub(crate) fn ensure_for_hosts_after_check_interruptible(
    settings: &ProxySettings,
    hostnames: &[String],
    cancelled: &impl Fn() -> bool,
    check: impl FnOnce(&StateStore) -> Result<LockOutcome<()>>,
) -> Result<LockOutcome<Value>> {
    ensure_certificate_generation_supported()?;
    let store = match StateStore::resolve_interruptible(settings.state_dir.clone(), cancelled)? {
        LockOutcome::Acquired(store) => store,
        LockOutcome::Cancelled => return Ok(LockOutcome::Cancelled),
    };
    match store.with_cert_lock_interruptible(cancelled, || {
        match check(&store)? {
            LockOutcome::Acquired(()) => {}
            LockOutcome::Cancelled => return Ok(LockOutcome::Cancelled),
        }
        ensure_for_hosts_locked_interruptible(&store, settings, hostnames, cancelled)
    })? {
        LockOutcome::Acquired(outcome) => Ok(outcome),
        LockOutcome::Cancelled => Ok(LockOutcome::Cancelled),
    }
}

fn ensure_for_hosts_locked(
    store: &StateStore,
    settings: &ProxySettings,
    hostnames: &[String],
) -> Result<Value> {
    match ensure_for_hosts_locked_interruptible(store, settings, hostnames, &|| false)? {
        LockOutcome::Acquired(value) => Ok(value),
        LockOutcome::Cancelled => bail!("uncancelled certificate preparation was cancelled"),
    }
}
