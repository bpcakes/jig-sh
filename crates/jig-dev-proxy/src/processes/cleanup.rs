use std::process::Child;
use std::sync::atomic::Ordering;

use crate::state::StateStore;

use super::child_lifecycle::terminate_and_reap;
use super::output::CapturedAppOutput;
use super::{CTRL_C_HANDLER, CTRL_C_REQUESTED};

pub(super) struct RunningChild {
    pub(super) name: String,
    pub(super) hostname: String,
    pub(super) proxied: bool,
    pub(super) store: StateStore,
    pub(super) child: Child,
    pub(super) output: CapturedAppOutput,
    pub(super) cleanup_armed: bool,
}

impl RunningChild {
    fn cleanup(&mut self) {
        if !self.cleanup_armed {
            return;
        }
        if self.proxied {
            if let Err(error) = self.store.remove_route(&self.hostname) {
                eprintln!(
                    "jig proxy could not remove route '{}' while cleaning up '{}': {error}",
                    self.hostname, self.name
                );
            }
        }
        match terminate_and_reap(&mut self.child) {
            Ok(()) => self.cleanup_armed = cleanup_remains_armed(&Ok(())),
            Err(error) => eprintln!(
                "jig proxy could not fully clean up child process {} for '{}': {error:#}; cleanup remains armed for a bounded retry",
                self.child.id(),
                self.name
            ),
        }
    }
}

fn cleanup_remains_armed(result: &anyhow::Result<()>) -> bool {
    result.is_err()
}
impl Drop for RunningChild {
    fn drop(&mut self) {
        self.cleanup()
    }
}
pub(super) fn cleanup_children(children: &mut [RunningChild]) {
    for running in children {
        running.cleanup();
    }
}

pub(super) fn start_ctrlc_cleanup_session() {
    CTRL_C_REQUESTED.store(false, Ordering::SeqCst);
    CTRL_C_HANDLER.get_or_init(|| {
        if let Err(error) = ctrlc::set_handler(|| {
            CTRL_C_REQUESTED.store(true, Ordering::SeqCst);
        }) {
            eprintln!("jig proxy could not install Ctrl-C cleanup handler: {error}");
        }
    });
}
pub(super) fn ctrl_c_requested() -> bool {
    CTRL_C_REQUESTED.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_disarms_only_after_confirmed_success() {
        assert!(!cleanup_remains_armed(&Ok(())));
        assert!(cleanup_remains_armed(&Err(anyhow::anyhow!(
            "still running"
        ))));
    }
}
