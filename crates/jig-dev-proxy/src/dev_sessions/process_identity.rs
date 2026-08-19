use crate::state::{DevProcessIdentity, PidObservation, observe_pid, process_start_token};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ProcessIdentityObservation {
    Alive,
    Absent,
    Uncertain,
}

pub(super) fn process_identity_may_be_alive(identity: &DevProcessIdentity) -> bool {
    observe_process_identity(identity) != ProcessIdentityObservation::Absent
}

pub(super) fn observe_process_identity(
    identity: &DevProcessIdentity,
) -> ProcessIdentityObservation {
    match observe_pid(identity.pid) {
        PidObservation::Absent => ProcessIdentityObservation::Absent,
        PidObservation::Alive | PidObservation::Uncertain => {
            match (
                identity.start_token.as_deref(),
                process_start_token(identity.pid),
            ) {
                (Some(expected), Some(actual)) if actual == expected => {
                    ProcessIdentityObservation::Alive
                }
                (Some(_), Some(_)) => ProcessIdentityObservation::Absent,
                (Some(_), None) | (None, _) => ProcessIdentityObservation::Uncertain,
            }
        }
    }
}

pub(super) fn capture_process_identity(pid: u32) -> DevProcessIdentity {
    DevProcessIdentity {
        pid,
        start_token: process_start_token(pid),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_live_pid_without_a_start_token_is_uncertain_not_verified() {
        let identity = DevProcessIdentity {
            pid: std::process::id(),
            start_token: None,
        };

        assert_eq!(
            observe_process_identity(&identity),
            ProcessIdentityObservation::Uncertain
        );
        assert!(process_identity_may_be_alive(&identity));
    }
}
