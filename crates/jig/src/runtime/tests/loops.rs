use std::cell::Cell;
use std::fs;
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::process::Command;

use serde_json::json;
use tempfile::tempdir;

use crate::command::{
    LoopAcknowledgeOccurrenceRequest, LoopClearAttemptRequest, LoopCommand, LoopDispatchRequest,
    LoopRunRequest, LoopStatusRequest, LoopTickRequest,
};
#[cfg(unix)]
use crate::runtime::tests::common::write_codex_stub;
use crate::runtime::tests::common::write_fixture_repo;
use crate::state::now_ms;
#[cfg(unix)]
use crate::test_env::{EnvVarGuard, lock_env};
use crate::tool_defs::LOOP_TICK_TOOL;
#[cfg(unix)]
use crate::tool_defs::WORKER_RUN_TOOL;

use super::*;

struct CancelAfterEntryObserver {
    checks: Cell<usize>,
}

impl crate::execution::ExecutionObserver for CancelAfterEntryObserver {}

impl crate::execution::ExecutionCancellation for CancelAfterEntryObserver {
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

include!("loops/task_and_engine.rs");
include!("loops/checkout_regressions.rs");
include!("loops/scheduled_failures.rs");
include!("loops/attempt_lifecycle.rs");
include!("loops/status_and_pr_manager.rs");
include!("loops/pr_manager_conflict.rs");
include!("loops/occurrence_lifecycle.rs");
include!("loops/pr_manager_retries_and_helpers.rs");
include!("loops/scheduled_attention_regressions.rs");
include!("loops/manual_attention_regressions.rs");
