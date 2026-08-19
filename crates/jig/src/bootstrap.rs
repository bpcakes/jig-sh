use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(test)]
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tempfile::{Builder as TempFileBuilder, TempDir};
use time::OffsetDateTime;
use toml::Table;
#[cfg(test)]
use toml::Value as TomlValue;
use ulid::Ulid;

use crate::context::RepoContext;
#[cfg(test)]
use crate::context::{RuntimeCacheProfile, runtime_cache_base, runtime_profile_cache_name};
use crate::frontend_metadata::resolve_frontend_metadata;
use crate::progress::CliProgress;
#[cfg(test)]
use crate::runtime_cache_lock::{RuntimeCacheLockPolicy, RuntimeCacheLocks};
use answers::{AnswerInput, RenderAnswers};
#[cfg(test)]
use file_copy::create_symlink;
#[cfg(test)]
use git::{git, git_stdout};
pub(crate) use git::{
    scrub_git_repository_environment_except, scrub_known_repository_git_environment,
};
use init_transaction::InitMutationTransaction;
#[cfg_attr(windows, allow(unused_imports))]
#[cfg(test)]
use init_transaction::{
    InitPathSnapshot, MAX_EXISTING_INIT_RETAINED_GENERATIONS, RETAINED_GENERATION_HANDLE_HEADROOM,
    process_soft_handle_limit, retained_generation_handle_requirement,
    retained_generation_handle_requirement_with_preimages,
    validate_existing_init_directory_after_create_error, validate_retained_generation_budget,
    validate_retained_generation_budget_with_preimages,
};
#[cfg(test)]
use initial_copy::seed_answers_toml;
use initial_copy::{BootstrapCopyRequest, render_and_copy_bootstrap_template};
#[cfg(test)]
use initial_template::{
    BuildTemplatePinPolicy, TEST_BUILD_TEMPLATE_PIN_POLICY, build_template_pin_policy_from_env,
    default_template_failure_context, is_official_template_source, official_template_ref,
    official_template_ref_for_version, resolve_initial_template_request_with_policy,
};
use initial_template::{prepare_initial_template_source, resolve_initial_template_request};
use path::{absolute_path_from, bootstrap_invocation_cwd, validate_repository_relative_ancestors};
#[cfg(test)]
use preview_seed::seed_preview_workspace;
use renderer::{RenderStageRequest, stage_render, stage_selected_render};
#[cfg(test)]
use sync::rendered_conflicts;
use sync::{ApplyRenderConflictPolicy, ApplyRenderOptions, apply_staged_render};
#[cfg(test)]
use template_source::PrivateAnswerOverrides;
use template_source::{
    EMBEDDED_TEMPLATE_SOURCE, prepare_template_source_from_base, prepare_update_template_source,
    read_stored_template_state,
};

mod adopt_infer;
mod answers;
mod crate_classification;
mod embedded_templates;
mod file_copy;
mod gate_preview;
mod git;
mod init;
mod init_transaction;
mod initial_copy;
mod initial_template;
mod launcher_repair_cache;
mod managed_paths;
mod opts;
pub(crate) mod path;
mod presets;
mod preview_seed;
mod renderer;
mod runtime_config;
mod scaffold;
mod staged_render;
mod sync;
mod template_source;
mod update;

pub(crate) use launcher_repair_cache::LAUNCHER_REPAIR_SEED_STAMP_HEADER;
use launcher_repair_cache::{
    FullRefreshRuntimePolicy, finish_full_refresh, seed_launcher_repair_runtime,
};
#[cfg(test)]
use launcher_repair_cache::{
    LAUNCHER_REPAIR_ENVIRONMENT_KEYS, LAUNCHER_REPAIR_RETIREMENT_RETRY_GUIDANCE,
    PublishedLauncherRepairCache, STALE_LAUNCHER_REPAIR_STAGING_AGE,
    TEST_FAIL_LAUNCHER_REPAIR_SEED_ENV, launcher_repair_retirement_warning,
    preserve_launcher_repair_staging, publish_launcher_repair_caches,
    publish_launcher_repair_caches_with_lock_policy, reap_stale_launcher_repair_staging,
    retire_launcher_repair_seeded_caches, rollback_published_repair_caches,
    sanitize_launcher_repair_environment,
};
#[cfg(all(test, unix))]
use launcher_repair_cache::{is_root_owned_nonwritable_path, root_owned_nonwritable_component};

pub use answers::HarnessFootprint;
pub(crate) use init::run_init;
pub use opts::AnswerOpts;
pub use presets::scaffold_presets_report;
pub use update::run_update;
pub(crate) use update::{
    launcher_only_repair_answers_are_valid, launcher_only_repair_scripts_are_recognizable,
};
#[cfg(test)]
use update::{
    legacy_launcher_only_paths, recognizable_contract_installer, recognizable_contract_launcher,
    recognizable_generated_installer, recognizable_generated_launcher,
};

const ANSWERS_FILE: &str = ".jig.toml";
pub(crate) const MANAGED_PATHS_MANIFEST_PATH: &str = managed_paths::MANIFEST_PATH;
const LAUNCHER_ONLY_MANAGED_PATHS: [&str; 2] = ["scripts/install-jig.sh", "scripts/jig"];
const ADOPT_RECEIPT_PATH: &str = ".agent/.cache/adopt/adopt-last.json";
const LEGACY_ADOPT_RECEIPT_PATH: &str = ".agent/state/adopt-last.json";
const ADOPT_RECEIPT_PATHS: [&str; 2] = [ADOPT_RECEIPT_PATH, LEGACY_ADOPT_RECEIPT_PATH];
pub(crate) const GIT_BIN_ENV: &str = "JIG_GIT_BIN";
const BUILD_TEMPLATE_PIN_RELEASED: &str = "released";
const BUILD_TEMPLATE_PIN_UNRELEASED: &str = "unreleased";
const OFFICIAL_TEMPLATE_SOURCE: &str = "https://github.com/bpcakes/jig-sh.git";
const REMOTE_TEMPLATE_MODE_ERROR: &str = "--template-mode only applies to local git template paths. Omit --template-mode for remote templates, or pass --template /path/to/jig-sh --template-mode committed.";
// Legacy conflict helpers keep these in sync with template task side effects.
#[cfg(test)]
const ALWAYS_TASK_MUTATED_PATHS: &[&str] = &[".jig.toml", "agent-map.md"];
const TEMPLATE_MODE_KEY: &str = "_template_mode";
const TEMPLATE_LOCAL_PATH_KEY: &str = "_template_local_path";
const GENERATED_NODE_VERSION: &str = "24.19.0";
const GENERATED_NODE_TYPES_VERSION: &str = "24.13.3";
pub(crate) const RUST_REACT_BACKEND_DEV_APP_NAME: &str = "api";
pub(crate) const RUST_REACT_ADMIN_BACKEND_DEV_APP_NAME: &str = "admin-api";

include!("bootstrap_parts/part_01.rs");
include!("bootstrap_parts/part_02.rs");
include!("bootstrap_parts/part_03.rs");

#[cfg(test)]
mod tests;
