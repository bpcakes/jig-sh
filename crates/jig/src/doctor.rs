use std::collections::{HashSet, VecDeque};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
#[cfg(all(test, unix))]
use std::{sync::atomic::Ordering, time::Instant};

use anyhow::anyhow;
use anyhow::{Context, Result};
use jig_owned_process::{
    OwnedProcessTreeError, ProcessOutputLimits, run_owned_process_tree_with_output,
    run_owned_process_tree_with_output_limits,
};
use serde::Serialize;
use serde_json::{Value, json};

#[cfg(test)]
use crate::cli::format_doctor_summary_for_test as format_summary;
use crate::command::{VaultCommand, VaultStatusRequest};
#[cfg(test)]
use crate::context::{
    FALLBACK_RUNTIME_CACHE_BASE, GIT_RUNTIME_CACHE_BASE, RUNTIME_CACHE_PROFILE_SUFFIX,
};
use crate::context::{
    JIG_REPO_ROOT_ENV, RepoContext, find_repo_root_from, find_repo_root_from_or_env,
};
#[cfg(test)]
use crate::tool_defs::tool;

mod runtime;

#[cfg(test)]
use runtime::launcher_repair_staging_check_at;
use runtime::{
    contract_migration_check, launcher_repair_cache_check, launcher_repair_seed_stamp_is_present,
    launcher_repair_staging_check, legacy_version_cache_check, runtime_check,
};

const COMMAND: &str = "doctor";
const LAUNCHER_REPAIR_STAGING_DOCTOR_MIN_AGE: Duration = Duration::from_secs(5 * 60);
const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const VERSION_AUTHORITY_MAX_BYTES: u64 = 128;
const GO_MODULE_AUTHORITY_MAX_BYTES: u64 = 1024 * 1024;
const CARGO_MANIFEST_AUTHORITY_MAX_BYTES: u64 = 1024 * 1024;
const SQLX_DRIVER_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const CODEX_SUPPORT_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const PROXY_LIST_DIAGNOSTIC_TIMEOUT: Duration = Duration::from_secs(120);
const PROXY_LIST_STDOUT_LIMIT: usize = 8 * 1024 * 1024;

#[cfg(unix)]
mod signal_session;
#[cfg(unix)]
pub(crate) use signal_session::DoctorSignalSession;
#[cfg(unix)]
use signal_session::finish_doctor_signal_session;
#[cfg(all(test, unix))]
use signal_session::{
    DOCTOR_ACTIVE_GENERATION, DOCTOR_SIGNAL_GENERATION, DOCTOR_SIGNAL_SESSION,
    DoctorSignalFinishAction, DoctorSignals, SQLX_PROBE_TEST_HANDLER_PAUSED,
    SQLX_PROBE_TEST_HANDLER_PAUSED_AFTER_RECORD, SQLX_PROBE_TEST_HANDLER_PAUSED_BEFORE_CLAIM,
    SQLX_PROBE_TEST_PAUSE_HANDLER, SQLX_PROBE_TEST_PAUSE_HANDLER_AFTER_RECORD,
    SQLX_PROBE_TEST_PAUSE_HANDLER_BEFORE_CLAIM, SQLX_PROBE_TEST_PAUSE_QUIESCENCE_TIMEOUT,
    SQLX_PROBE_TEST_QUIESCENCE_TIMED_OUT, SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT,
    SQLX_PROBE_TEST_REDELIVERED_SIGNAL_ORDER, SQLX_PROBE_TEST_RELEASE_HANDLER,
    SQLX_PROBE_TEST_RELEASE_HANDLER_AFTER_RECORD, SQLX_PROBE_TEST_RELEASE_HANDLER_BEFORE_CLAIM,
    SQLX_PROBE_TEST_RELEASE_QUIESCENCE_TIMEOUT, doctor_signal_bit, doctor_signal_finish_action,
    install_default_doctor_signal_handler, record_doctor_signal, record_sqlx_probe_test_redelivery,
};

include!("doctor_parts/part_01.rs");
include!("doctor_parts/part_02.rs");
include!("doctor_parts/part_03.rs");
include!("doctor_parts/part_04.rs");
include!("doctor_parts/part_05.rs");
include!("doctor_parts/part_06.rs");
include!("doctor_parts/part_07.rs");
include!("doctor_parts/part_08.rs");
include!("doctor_parts/part_08_shell_token_state.rs");
include!("doctor_parts/part_09.rs");
include!("doctor_parts/part_10.rs");
include!("doctor_parts/part_11.rs");

pub(crate) fn go_version_selector(ctx: &RepoContext) -> Result<String> {
    let authority_paths = ctx
        .go_module_authority_paths()
        .context("Could not resolve Go module authority")?;
    let (_, requirement) = select_go_module_version_requirement(&authority_paths)
        .map_err(|error| anyhow!(error.reason))?
        .context("This repository does not declare a Go module authority")?;
    Ok(requirement.selector)
}

#[cfg(test)]
mod tests;
