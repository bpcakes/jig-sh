//! Jig's init, adopt, and update flows: answers, adoption inference, native
//! template rendering from embedded or explicit sources, and the staged,
//! transactional writes that publish them.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(test)]
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use jig_context::RepoContext;
#[cfg(test)]
use jig_context::{RuntimeCacheProfile, runtime_cache_base, runtime_profile_cache_name};
use jig_execution::progress::CliProgress;
use jig_repository::path::{
    self, absolute_path_from, bootstrap_invocation_cwd, validate_repository_relative_ancestors,
};
use tempfile::{Builder as TempFileBuilder, TempDir};
use toml::Table;
#[cfg(test)]
use toml::Value as TomlValue;

#[cfg(test)]
use crate::runtime_cache_lock::{RuntimeCacheLockPolicy, RuntimeCacheLocks};
use answers::RenderAnswers;
#[cfg(test)]
use file_copy::create_symlink;
#[cfg(test)]
use git::{git, git_stdout};
use init_transaction::InitMutationTransaction;
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
pub use initial_template::record_build_template_pin_policy;
#[cfg(test)]
use initial_template::{
    BuildTemplatePinPolicy, TEST_BUILD_TEMPLATE_PIN_POLICY, build_template_pin_policy_from_env,
    default_template_failure_context, is_official_template_source, official_template_ref,
    official_template_ref_for_version, resolve_initial_template_request_with_policy,
};
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

#[cfg(test)]
use adopt::{ADOPT_RECEIPT_PATH, ADOPT_RECEIPT_PATHS, LEGACY_ADOPT_RECEIPT_PATH};
#[cfg(test)]
use apps::parse_frontend_app;
#[cfg(test)]
use initial_template::resolve_initial_template_request;
#[cfg(test)]
use serde_json::{Value, json};
mod adopt;
mod apps;
mod destination;
mod initial_report;
mod scaffold_opts;

pub use adopt::run_adopt;
pub use apps::{DevApp, FrontendApp};
pub use destination::preflight_init_destination;
use destination::{
    ensure_init_destination_noreplace_supported, reject_newer_declared_contract,
    validate_init_destination, validate_update_destination,
};
pub use init::{prepare_init_answers_for_interaction, should_default_init_sqlx_disabled};
#[cfg(test)]
use initial_report::initial_command_report;
pub use initial_report::{BootstrapVaultReport, InitReport};
use initial_report::{
    InitialCommand, initial_next_steps, initial_notes, initial_render_report,
    template_progress_label,
};
pub use opts::{AdoptOpts, InitOpts, TemplateMode, UpdateOpts};
use scaffold_opts::ScaffoldFrontendKind;
pub use scaffold_opts::{
    ScaffoldDb, ScaffoldFrontend, ScaffoldJobs, ScaffoldMetrics, ScaffoldOpts, ScaffoldPreset,
    parse_scaffold_frontend,
};
mod adopt_infer;
pub use adopt_infer::ComponentSelectionOpts;
mod adoption_file_budget;
mod answers;
#[cfg(test)]
pub use jig_context::backend::BackendLanguage;
pub mod clippy_policy;
mod crate_classification;
mod embedded_templates;
mod file_budget_lifecycle;
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
mod presets;
mod preview_seed;
mod renderer;
mod repository_model;
mod retired_mcp;
mod runtime_config;
mod scaffold;
mod source_inputs;
pub use scaffold::{default_go_module, validate_go_module};
mod staged_render;
mod sync;
mod template_source;
mod update;
mod update_transaction;

pub use launcher_repair_cache::LAUNCHER_REPAIR_SEED_STAMP_HEADER;
use launcher_repair_cache::seed_launcher_repair_runtime;
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
pub use answers::PreparedInitAnswers;
#[cfg(any(test, feature = "test-support"))]
pub use init::run_init;
pub use init::run_prepared_init;
pub use opts::AnswerOpts;
pub use opts::DevSettingsAnswers;
pub use presets::scaffold_presets_report;
pub use update::run_update;
pub use update::{
    launcher_only_repair_answers_are_valid, launcher_only_repair_scripts_are_recognizable,
};
#[cfg(test)]
use update::{
    legacy_launcher_only_paths, recognizable_contract_installer, recognizable_contract_launcher,
    recognizable_generated_installer, recognizable_generated_launcher,
};

const ANSWERS_FILE: &str = ".jig.toml";
pub const MANAGED_PATHS_MANIFEST_PATH: &str = managed_paths::MANIFEST_PATH;
const LAUNCHER_ONLY_MANAGED_PATHS: [&str; 2] = ["scripts/install-jig.sh", "scripts/jig"];

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
pub const APPLICATION_BACKEND_DEV_APP_NAME: &str = "api";
pub const RUST_REACT_ADMIN_BACKEND_DEV_APP_NAME: &str = "admin-api";

fn generated_package_manager_spec(package_manager: &str) -> &'static str {
    match package_manager {
        "bun" => "bun@1.3.14",
        "npm" => "npm@12.0.2",
        "pnpm" => "pnpm@11.22.0",
        "yarn" => "yarn@4.18.0",
        _ => unreachable!("web package manager was already validated"),
    }
}

fn generated_package_manager_version(package_manager: &str) -> &'static str {
    generated_package_manager_spec(package_manager)
        .split_once('@')
        .expect("generated package manager specs contain @")
        .1
}

#[cfg(test)]
fn read_optional_answer_string(answers_path: &Path, key: &str) -> Result<Option<String>> {
    let answers = read_answers_toml(answers_path)?;
    Ok(answers
        .get(key)
        .and_then(TomlValue::as_str)
        .map(str::to_string)
        .filter(|value| !value.is_empty()))
}

fn read_answers_toml(path: &Path) -> Result<Table> {
    let text =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("Failed to parse {}", path.display()))
}

#[cfg(test)]
fn write_answers_toml(path: &Path, mapping: &Table) -> Result<()> {
    let toml = toml::to_string(mapping)
        .with_context(|| format!("Failed to serialize {}", path.display()))?;
    fs::write(path, toml).with_context(|| format!("Failed to write {}", path.display()))
}

#[cfg(test)]
mod tests;

pub mod runtime_artifacts;
pub mod runtime_cache_lock;

#[cfg(test)]
use jig_context::test_support as test_env;
