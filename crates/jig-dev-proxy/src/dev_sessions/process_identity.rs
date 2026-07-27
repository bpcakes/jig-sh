use crate::state::{DevProcessIdentity, pid_is_alive, process_start_token};

pub(super) fn process_identity_matches(identity: &DevProcessIdentity) -> bool {
    identity.start_token.as_deref().is_some_and(|token| {
        pid_is_alive(identity.pid) && process_start_token(identity.pid).as_deref() == Some(token)
    })
}

pub(super) fn process_identity_observed_alive(identity: &DevProcessIdentity) -> bool {
    if !pid_is_alive(identity.pid) {
        return false;
    }
    match (
        identity.start_token.as_deref(),
        process_start_token(identity.pid),
    ) {
        (Some(expected), Some(actual)) => actual == expected,
        // A live PID whose start identity cannot currently be read is
        // uncertain, not dead. Management must retain it and fail closed.
        (Some(_), None) | (None, _) => true,
    }
}

pub(super) fn capture_process_identity(pid: u32) -> DevProcessIdentity {
    DevProcessIdentity {
        pid,
        start_token: process_start_token(pid),
    }
}
