use anyhow::{Result, bail};

use crate::ports::{CapabilityProbe, ProxyCapabilities, jig_proxy_capabilities};
use crate::state::{LockOutcome, StateStore};
use crate::types::ProxySettings;

pub(super) fn runtime_generation_matches(
    store: &StateStore,
    health_pid: u32,
    health_token: &str,
    cancelled: &impl Fn() -> bool,
    transient_as_miss: bool,
) -> Result<LockOutcome<bool>> {
    let current_pid = match store.read_pid_interruptible(cancelled)? {
        LockOutcome::Acquired(pid) => pid,
        LockOutcome::Cancelled => return Ok(LockOutcome::Cancelled),
    };
    let current_token = match store.read_health_token_interruptible(cancelled)? {
        LockOutcome::Acquired(token) => token,
        LockOutcome::Cancelled => return Ok(LockOutcome::Cancelled),
    };
    if current_pid == Some(health_pid) && current_token.as_deref() == Some(health_token) {
        return Ok(LockOutcome::Acquired(true));
    }
    if transient_as_miss {
        return Ok(LockOutcome::Acquired(false));
    }
    bail!(
        "Jig proxy runtime generation changed during capability verification in state dir {}. Retry after the proxy stabilizes; no shared proxy was restarted.",
        store.root().display(),
    )
}

pub(super) fn capability_pid_matches(
    store: &StateStore,
    health_pid: u32,
    capability_pid: u32,
    transient_as_miss: bool,
) -> Result<bool> {
    if capability_pid == health_pid {
        return Ok(true);
    }
    if transient_as_miss {
        return Ok(false);
    }
    bail!(
        "Jig proxy generation changed during capability verification in state dir {} (health PID {health_pid}, capability PID {capability_pid}). Retry after the proxy stabilizes; no shared proxy was restarted.",
        store.root().display(),
    )
}

pub(super) fn checked_capabilities(
    store: &StateStore,
    settings: &ProxySettings,
    http_port: u16,
    health_token: &str,
    transient_as_miss: bool,
) -> Result<Option<ProxyCapabilities>> {
    match jig_proxy_capabilities("127.0.0.1", http_port, health_token) {
        CapabilityProbe::Available(capabilities) => Ok(Some(capabilities)),
        CapabilityProbe::Unsupported => {
            bail!(
                "The running Jig proxy in state dir {} cannot report authenticated LAN/HTTPS HTTP2 capabilities (requested LAN={}, HTTPS={}, HTTP2={}). It may be an older proxy. Keep existing sessions running; explicitly stop and restart that shared proxy with `scripts/jig proxy stop --state-dir PATH` and `scripts/jig proxy start --state-dir PATH` using this state directory and the desired listener flags, or use a compatible proxy. No proxy was restarted.",
                store.root().display(),
                settings.lan,
                settings.https,
                settings.http2,
            );
        }
        CapabilityProbe::Unavailable | CapabilityProbe::Invalid if transient_as_miss => Ok(None),
        CapabilityProbe::Unavailable => {
            bail!(
                "Jig proxy capability probe in state dir {} was temporarily unavailable after health succeeded. Retry when the proxy stabilizes; no shared proxy was restarted.",
                store.root().display(),
            );
        }
        CapabilityProbe::Invalid => {
            bail!(
                "Jig proxy in state dir {} returned invalid capability evidence. Inspect the proxy before reusing it; no shared proxy was restarted.",
                store.root().display(),
            );
        }
    }
}
