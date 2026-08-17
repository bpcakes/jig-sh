use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

use crate::frontend_metadata::resolve_frontend_metadata;
use crate::progress::CliProgress;
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
use renderer::{RenderStageRequest, stage_render};
#[cfg(test)]
use sync::rendered_conflicts;
use sync::{ApplyRenderOptions, apply_staged_render};
#[cfg(test)]
use template_source::EMBEDDED_TEMPLATE_SOURCE;
#[cfg(test)]
use template_source::PrivateAnswerOverrides;
use template_source::{prepare_update_template_source, read_stored_template_state};

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

pub use answers::HarnessFootprint;
pub(crate) use init::run_init;
pub use opts::AnswerOpts;
pub use presets::scaffold_presets_report;

const ANSWERS_FILE: &str = ".jig.toml";
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
