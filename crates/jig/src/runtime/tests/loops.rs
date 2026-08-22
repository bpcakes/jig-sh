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

include!("loops/task_and_engine.rs");
include!("loops/scheduled_failures.rs");
include!("loops/attempt_lifecycle.rs");
include!("loops/status_and_pr_manager.rs");
include!("loops/occurrence_lifecycle.rs");
include!("loops/pr_manager_retries_and_helpers.rs");
