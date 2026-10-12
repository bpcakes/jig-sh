use std::cell::Cell;
use std::fs;
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::process::Command;

use jig_state::now_ms;
use serde_json::json;
use tempfile::tempdir;

use crate::command::{
    LoopAcknowledgeOccurrenceRequest, LoopClearAttemptRequest, LoopCommand, LoopDispatchRequest,
    LoopRunRequest, LoopStatusRequest, LoopTickRequest,
};
#[cfg(unix)]
use crate::runtime::tests::common::write_codex_stub;
use crate::runtime::tests::common::write_fixture_repo;
#[cfg(unix)]
use crate::test_env::{EnvVarGuard, lock_env};

use super::*;

struct CancelAfterEntryObserver {
    checks: Cell<usize>,
}

impl jig_execution::ExecutionObserver for CancelAfterEntryObserver {}

impl jig_execution::ExecutionCancellation for CancelAfterEntryObserver {
    fn cancelled(&self) -> bool {
        let checks = self.checks.get();
        self.checks.set(checks + 1);
        checks > 0
    }
}

impl CancelAfterEntryObserver {
    fn new() -> Self {
        Self {
            checks: Cell::new(0),
        }
    }
}

mod attempt_lifecycle;
mod checkout_regressions;
mod manual_attention_regressions;
mod occurrence_lifecycle;
mod pr_manager_conflict;
mod pr_manager_retries_and_helpers;
mod pr_manager_review_authority;
mod scheduled_attention_regressions;
mod scheduled_failures;
mod status_and_pr_manager;
mod task_and_engine;

use pr_manager_retries_and_helpers::*;
use scheduled_attention_regressions::*;
use scheduled_failures::*;
