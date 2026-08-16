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

#[derive(Args, Clone, Debug)]
#[command(after_help = "\
For existing repositories, use:
  jig adopt .

Templates:
  Omit --template for the default jig-sh harness template.
  Release builds pin that template to this jig version's release tag.
  Unreleased local builds use templates embedded in the jig binary unless --vcs-ref is supplied.

Scaffold ownership:
  Presets create starter application code once. After creation, that app code is project-owned.
  `jig update` keeps the Jig harness current; it does not rewrite scaffolded app code.

Interaction modes:
  Interactive terminals prompt only for unresolved project-shape choices.
  --defaults uses rust-react, database none, and frontend web when those choices are omitted.
  --no-input and non-terminal execution require the project shape to be fully specified.

Examples:
  jig init /path/to/new-repo
  jig init /path/to/new-repo --preset harness-only --repo-name new-repo --sqlx-enabled false --no-input --no-vault
  jig init /path/to/new-repo --preset harness-only --no-input --no-vault
  jig init /path/to/new-repo --preset rust-react
  jig init /path/to/new-repo --preset rust-react --db postgres --frontends web,landing,admin
  jig presets
  jig init /path/to/new-repo --preset harness-only --template /path/to/jig-sh --template-mode committed --repo-name new-repo --sqlx-enabled false --no-input --no-vault")]
pub struct InitOpts {
    #[arg(help = "Destination directory for the new repository")]
    pub path: PathBuf,
    #[command(flatten)]
    pub scaffold: ScaffoldOpts,
    #[arg(
        long,
        help_heading = "Advanced Template Source",
        value_name = "PATH_OR_GIT_URL",
        help = "Template source to render; defaults to the official jig-sh template",
        long_help = "Template source to render. Release builds default to the official jig-sh template at https://github.com/bpcakes/jig-sh.git pinned to the release tag for this jig version; passing that canonical HTTPS URL explicitly, with or without .git, has the same pinned behavior unless --vcs-ref is also provided. Unreleased or dirty local builds use templates embedded in the jig binary for omitted --template, avoiding a stale release-tag lookup during local development. For checkout-driven template development, pass the path to your jig-sh checkout, for example /Users/you/src/jig-sh. For remote forks, SSH URLs, or private harnesses, pass a git URL. The source must contain templates/project."
    )]
    pub template: Option<String>,
    #[arg(
        long,
        value_enum,
        help_heading = "Advanced Template Source",
        help = "How to read a local git template checkout",
        long_help = "How to read a local git template checkout. The default for local git paths is committed, which renders from clean HEAD and refuses dirty template changes."
    )]
    pub template_mode: Option<TemplateMode>,
    #[arg(
        long,
        help_heading = "Advanced Template Source",
        help = "Git revision to render from the template source"
    )]
    pub vcs_ref: Option<String>,
    #[arg(
        long,
        help_heading = "Safety",
        help = "Allow init to write into a non-empty destination and overwrite existing scaffold files",
        long_help = "Allow init to write into a non-empty destination and overwrite existing scaffold files. Template-to-scaffold path collisions are still rejected because they indicate a preset/template ownership bug."
    )]
    pub force: bool,
    #[arg(
        long,
        help_heading = "Automation",
        help = "Skip the init wizard; omitted project shape defaults to rust-react, database none, and frontend web",
        long_help = "Skip the init wizard and resolve omitted project-shape choices to --preset rust-react, --db none, and --frontend web. Explicit scaffold flags are preserved, and effective frontend_apps from --answers-file prevent the default web scaffold from being added."
    )]
    pub defaults: bool,
    #[arg(
        long,
        help_heading = "Automation",
        help = "Skip the init wizard and require an explicit, complete project shape instead of prompting",
        long_help = "Skip the init wizard and require --preset. The rust-react preset also requires an explicit --db choice plus --frontend/--frontends or effective frontend_apps from --answers-file. The harness-only preset rejects database and scaffold frontend flags. Non-terminal execution without --defaults follows this strict behavior."
    )]
    pub no_input: bool,
    #[arg(
        long,
        help_heading = "Vault",
        help = "Skip initial passphrase setup; generated repo metadata still declares a vault scope"
    )]
    pub no_vault: bool,
    #[command(flatten)]
    pub answers: AnswerOpts,
}

#[derive(Args, Clone, Debug)]
#[command(after_help = "\
Templates:
  Release builds default to the official jig-sh harness template:
  https://github.com/bpcakes/jig-sh.git

  Release builds pin omitted --template to this jig version's release tag.
  Unreleased or dirty local builds use templates embedded in the jig binary unless --vcs-ref is supplied.

Adoption scans the existing repository before resolving answers. If SQLx is detected,
omitted SQLx answers resolve to migration defaults; if it is not detected, omitted SQLx
answers resolve to a tooling-only profile. Pass --sqlx-enabled true and --rust-migration-dir
<dir> to override.

Examples:
  jig adopt .
  jig adopt . --write
  jig adopt . --minimal --write
  jig adopt . --write --template /path/to/jig-sh --template-mode committed")]
pub struct AdoptOpts {
    #[arg(default_value = ".", help = "Existing repository directory to adopt")]
    pub path: PathBuf,
    #[arg(
        long,
        value_name = "PATH_OR_GIT_URL",
        help = "Template source to render; defaults to the official jig-sh template",
        long_help = "Template source to render. Release builds default to the official jig-sh template at https://github.com/bpcakes/jig-sh.git pinned to the release tag for this jig version; passing that canonical HTTPS URL explicitly, with or without .git, has the same pinned behavior unless --vcs-ref is also provided. Unreleased or dirty local builds use templates embedded in the jig binary for omitted --template, avoiding a stale release-tag lookup during local development. For checkout-driven template development, pass the path to your jig-sh checkout, for example /Users/you/src/jig-sh. For remote forks, SSH URLs, or private harnesses, pass a git URL. The source must contain templates/project."
    )]
    pub template: Option<String>,
    #[arg(
        long,
        value_enum,
        help = "How to read a local git template checkout",
        long_help = "How to read a local git template checkout. The default for local git paths is committed, which renders from clean HEAD and refuses dirty template changes."
    )]
    pub template_mode: Option<TemplateMode>,
    #[arg(long, help = "Git revision to render from the template source")]
    pub vcs_ref: Option<String>,
    #[arg(long, help = "Overwrite conflicting template-managed paths")]
    pub force: bool,
    #[arg(long, help = "Write rendered managed files; omit to preview only")]
    pub write: bool,
    #[arg(
        long,
        help = "Render only .jig.toml and .agent/ scaffolding (no scripts, workflows, or agent context files)",
        long_help = "Render a loop-ready minimal footprint: .jig.toml, .agent/jig-contract.json, and .agent/ scaffolding, plus block-managed .gitignore/.gitattributes. Omits scripts/, .github/workflows/, AGENTS.md, agent-map.md, and .mcp.json. Stores harness_footprint = \"minimal\" so jig update keeps the same footprint until you re-adopt without --minimal."
    )]
    pub minimal: bool,
    #[arg(
        long,
        help = "Use default answers for omitted configuration prompts and adopt write confirmation; vault setup captures credentials before rendering"
    )]
    pub defaults: bool,
    #[arg(
        long,
        help = "Fail instead of prompting for missing answers and skip adopt write confirmation; vault setup requires JIG_VAULT_PASSPHRASE or --no-vault"
    )]
    pub no_input: bool,
    #[arg(
        long,
        help = "Skip initial passphrase setup when --write is supplied; generated repo metadata still declares a vault scope"
    )]
    pub no_vault: bool,
    #[command(flatten)]
    pub answers: AnswerOpts,
}

#[derive(Args, Clone, Debug)]
#[command(after_help = "\
Update modes:
  jig update advances to the resolved template source.
  jig update --recopy re-renders from the stored .jig.toml commit.
  Add --force only when changed template-managed files should be replaced.

Examples:
  jig update
  jig update --recopy
  jig update --template /path/to/jig-sh --template-mode committed --force")]
pub struct UpdateOpts {
    #[arg(default_value = ".", help = "Adopted repository directory to update")]
    pub path: PathBuf,
    #[arg(long, help = "Template source to render from for this update")]
    pub template: Option<String>,
    #[arg(long, value_enum, help = "How to read a local git template checkout")]
    pub template_mode: Option<TemplateMode>,
    #[arg(
        long,
        help = "Re-render from the stored .jig.toml commit instead of advancing"
    )]
    pub recopy: bool,
    #[arg(long, help = "Overwrite changed template-managed files")]
    pub force: bool,
    #[arg(long, help = "Git revision to render from the template source")]
    pub vcs_ref: Option<String>,
    #[arg(long, help = "Use default answers for omitted configuration prompts")]
    pub defaults: bool,
    #[arg(long, help = "Fail instead of prompting for missing answers")]
    pub no_input: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FrontendApp {
    pub name: String,
    pub dir: String,
    pub coverage_threshold: u32,
    pub kind: String,
    pub role: String,
}

impl<'de> Deserialize<'de> for FrontendApp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct FrontendAppFields {
            name: String,
            dir: String,
            coverage_threshold: u32,
            #[serde(default)]
            kind: Option<String>,
            #[serde(default)]
            role: Option<String>,
        }

        let fields = FrontendAppFields::deserialize(deserializer)?;
        let metadata = resolve_frontend_metadata(
            &fields.name,
            fields.kind.as_deref(),
            fields.role.as_deref(),
            None,
        );
        Ok(Self {
            name: fields.name,
            dir: fields.dir,
            coverage_threshold: fields.coverage_threshold,
            kind: metadata.kind.into(),
            role: metadata.role.into(),
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DevApp {
    pub name: String,
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(default = "default_dev_app_kind")]
    pub kind: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub argv: Vec<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default = "default_true")]
    pub proxy: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum TemplateMode {
    Committed,
}

#[derive(Args, Clone, Debug, Default)]
pub struct ScaffoldOpts {
    #[arg(
        long,
        value_enum,
        help_heading = "Project Shape",
        help = "Project scaffold to generate alongside the Jig harness; run `jig presets` to inspect available presets"
    )]
    pub preset: Option<ScaffoldPreset>,
    #[arg(
        long,
        value_enum,
        help_heading = "Project Shape",
        help = "Database scaffold for presets that support a Rust backend"
    )]
    pub db: Option<ScaffoldDb>,
    #[arg(
        long = "frontend",
        help_heading = "Project Shape",
        value_parser = parse_scaffold_frontend,
        help = "Frontend scaffold as name[:kind], e.g. web:spa, landing:astro, admin-panel:admin; may be repeated. Bare web, landing, and admin use preset shorthands. Rust-react reserves api and admin-api for backend dev apps."
    )]
    pub frontends: Vec<ScaffoldFrontend>,
    #[arg(
        long = "frontends",
        help_heading = "Project Shape",
        value_delimiter = ',',
        value_parser = parse_scaffold_frontend,
        help = "Comma-separated frontend scaffolds, e.g. web,landing,admin. Bare web, landing, and admin use preset shorthands. Rust-react reserves api and admin-api for backend dev apps."
    )]
    pub frontend_list: Vec<ScaffoldFrontend>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ScaffoldPreset {
    RustReact,
    HarnessOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ScaffoldDb {
    None,
    Postgres,
    Sqlite,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScaffoldFrontend {
    name: String,
    kind: ScaffoldFrontendKind,
    custom_default_name: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScaffoldFrontendKind {
    Spa,
    Admin,
    Astro,
}

impl TemplateMode {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Committed => "committed",
        }
    }
}

pub(crate) fn merge_init_answer_file_for_interaction(answers: &mut AnswerOpts) -> Result<()> {
    let invocation_cwd = bootstrap_invocation_cwd()?;
    let input = AnswerInput::from_opts_at(answers, &invocation_cwd)?;
    *answers = input.effective_opts(answers)?;
    Ok(())
}

pub(crate) fn should_default_init_sqlx_disabled(answers: &AnswerOpts) -> bool {
    answers::should_default_init_sqlx_disabled(answers)
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct InitReport {
    ok: bool,
    command: String,
    render_mode: String,
    template: String,
    destination: String,
    answers_file: String,
    git_initialized: bool,
    scaffold: Option<Value>,
    render_report: Value,
    next_steps: Vec<String>,
    notes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vault: Option<BootstrapVaultReport>,
    // Keep legacy JSON-style bootstrap assertions working without carrying a
    // second representation in production reports.
    #[cfg(test)]
    #[serde(skip)]
    serialized: std::sync::OnceLock<Value>,
}

impl InitReport {
    pub(crate) fn destination(&self) -> &str {
        &self.destination
    }

    pub(crate) fn template(&self) -> &str {
        &self.template
    }

    pub(crate) const fn git_initialized(&self) -> bool {
        self.git_initialized
    }

    pub(crate) fn scaffold(&self) -> Option<&Value> {
        self.scaffold.as_ref()
    }

    pub(crate) const fn render_report(&self) -> &Value {
        &self.render_report
    }

    pub(crate) fn next_steps(&self) -> &[String] {
        &self.next_steps
    }

    pub(crate) fn notes(&self) -> &[String] {
        &self.notes
    }

    pub(crate) fn vault(&self) -> Option<&BootstrapVaultReport> {
        self.vault.as_ref()
    }

    pub(crate) fn attach_vault(&mut self, vault: BootstrapVaultReport) -> Result<()> {
        if self.vault.is_some() {
            bail!("bootstrap::run_init output unexpectedly included a vault field");
        }
        self.vault = Some(vault);
        #[cfg(test)]
        {
            self.serialized = std::sync::OnceLock::new();
        }
        Ok(())
    }
}

#[cfg(test)]
impl std::ops::Deref for InitReport {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        self.serialized.get_or_init(|| {
            serde_json::to_value(self).expect("typed init report should serialize for legacy tests")
        })
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct BootstrapVaultReport {
    requested: bool,
    initialized: bool,
    created: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    skipped_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vault_home: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vault_scope: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vault_scope_id: Option<Value>,
}

impl BootstrapVaultReport {
    pub(crate) fn disabled() -> Self {
        Self::skipped(false, "disabled")
    }

    pub(crate) fn missing_scope() -> Self {
        Self::skipped(true, "repo has no [vault] scope")
    }

    fn skipped(requested: bool, reason: &str) -> Self {
        Self {
            requested,
            initialized: false,
            created: false,
            skipped_reason: Some(reason.to_string()),
            vault_home: None,
            vault_scope: None,
            vault_scope_id: None,
        }
    }

    pub(crate) fn initialized(created: bool, runtime_report: &Value) -> Self {
        Self {
            requested: true,
            initialized: true,
            created,
            skipped_reason: None,
            vault_home: Some(runtime_report["vault_home"].clone()),
            vault_scope: Some(runtime_report["vault_scope"].clone()),
            vault_scope_id: Some(runtime_report["vault_scope_id"].clone()),
        }
    }

    pub(crate) const fn requested(&self) -> bool {
        self.requested
    }

    pub(crate) const fn initialized_status(&self) -> bool {
        self.initialized
    }

    pub(crate) const fn created(&self) -> bool {
        self.created
    }

    pub(crate) fn skipped_reason(&self) -> Option<&str> {
        self.skipped_reason.as_deref()
    }

    pub(crate) fn vault_scope(&self) -> Option<&str> {
        self.vault_scope.as_ref().and_then(Value::as_str)
    }
}

pub(crate) fn preflight_init_destination(opts: &InitOpts) -> Result<()> {
    let invocation_cwd = bootstrap_invocation_cwd()?;
    let destination = path::resolve_init_destination(&opts.path, &invocation_cwd)?;
    validate_init_destination(&destination, opts.force)?;
    ensure_init_destination_noreplace_supported(&destination)
}

fn ensure_init_destination_noreplace_supported(destination: &Path) -> Result<()> {
    let (existing_ancestor, _) = path::split_existing_ancestor(destination)?;
    path::ensure_atomic_noreplace_publication_supported(&existing_ancestor)
}

pub fn run_adopt(opts: AdoptOpts) -> Result<Value> {
    let invocation_cwd = bootstrap_invocation_cwd()?;
    let destination = absolute_path_from(&opts.path, &invocation_cwd)?;
    let progress = CliProgress::new("adopt");
    progress.header_for_path("render harness into existing repo", &destination);
    progress.step("validate destination", "existing repository directory");
    progress.log_blocked_on_err(validate_adopt_destination(&destination))?;
    let prior_managed_paths =
        progress.log_blocked_on_err(managed_paths::load_manifest(&destination))?;
    progress.step(
        "resolve template",
        template_progress_label(opts.template.as_deref()),
    );
    let template_request = progress.log_blocked_on_err(resolve_initial_template_request(
        opts.template.as_deref(),
        &opts.vcs_ref,
    ))?;
    let template = progress.log_blocked_on_err(prepare_initial_template_source(
        &template_request,
        opts.template_mode,
        &invocation_cwd,
    ))?;
    progress.step("infer answers", "scan existing repository");
    let inference = adopt_infer::infer_adopt_answers(&destination);
    let prior_answers = recognized_prior_answers(&destination);
    let requested_harness_footprint = if opts.minimal {
        HarnessFootprint::Minimal
    } else {
        HarnessFootprint::Full
    };
    let expands_minimal_harness = prior_answers.as_ref().is_some_and(|prior| {
        prior.harness_footprint() == HarnessFootprint::Minimal
            && requested_harness_footprint == HarnessFootprint::Full
    });
    let changes_harness_footprint = prior_answers
        .as_ref()
        .is_some_and(|prior| prior.harness_footprint() != requested_harness_footprint);
    let establishes_manifest = prior_managed_paths.is_none() && prior_answers.is_some();
    if prior_managed_paths.is_none()
        && prior_answers.as_ref().is_some_and(|prior| {
            prior.harness_footprint() == HarnessFootprint::Full
                && requested_harness_footprint == HarnessFootprint::Minimal
        })
    {
        bail!(
            "Cannot switch this adopted repository from the full harness to --minimal because {} is missing. First run `jig adopt . --write` without --minimal to establish exact managed-path ownership, then retry the minimal adoption.",
            managed_paths::MANIFEST_PATH
        );
    }
    let mut answers = opts.answers.clone();
    answers.harness_footprint = Some(requested_harness_footprint);
    let answer_input = progress.log_blocked_on_err(
        if (changes_harness_footprint || establishes_manifest) && answers.answers_file.is_none() {
            AnswerInput::from_file(&destination.join(ANSWERS_FILE))
        } else {
            AnswerInput::from_opts_at(&answers, &invocation_cwd)
        },
    )?;
    let answer_shape = answer_input.shape().clone();
    progress.info("detected", inference.summary());
    progress.info("detected stack", inference.detected_stack_label());
    if opts.minimal {
        progress.info(
            "footprint",
            "minimal (.jig.toml + .agent/ scaffolding; no scripts/workflows/context files)",
        );
    }
    for warning in inference.warnings() {
        progress.info("warning", warning);
    }
    inference.apply_to_answers(&mut answers, &answer_shape);
    let review = inference.adoption_review(&answers, &opts.answers, &answer_shape);
    for item in &review.items {
        progress.info("review", item);
    }
    if opts.write {
        confirm_adopt_write(&opts)?;
    } else {
        progress.info(
            "mode",
            "preview only; re-run with --write to apply managed files",
        );
    }
    let backup_root = opts.write.then(|| adopt_backup_root(&destination));
    if opts.write {
        progress.log_blocked_on_err(validate_adopt_output_ancestors(
            &destination,
            backup_root.as_deref(),
        ))?;
    }

    let copy_result = render_and_copy_bootstrap_template(BootstrapCopyRequest {
        destination: &destination,
        template: &template,
        answers: &answers,
        answer_input: Some(answer_input),
        use_defaults: opts.defaults,
        force: opts.force,
        dry_run: !opts.write,
        backup_root: backup_root.clone(),
        seed_repo_path: Some(&destination),
        prior_harness_footprint: prior_answers.as_ref().map(RenderAnswers::harness_footprint),
        prior_managed_paths: prior_managed_paths.as_ref(),
        reconcile_runtime_config: prior_answers.is_some(),
        allow_answers_overwrite: expands_minimal_harness || establishes_manifest,
        allow_contract_overwrite: expands_minimal_harness,
        reserved_output_paths: Vec::new(),
        init_transaction: None,
        progress,
    })?;
    if opts.write {
        if let Err(error) =
            write_adopt_last_receipt(&destination, backup_root.as_deref(), &copy_result)
        {
            progress.info(
                "warning",
                format!("adopt write completed but undo receipt could not be recorded: {error:#}"),
            );
        }
        progress.done("adopt complete");
    } else {
        progress.done("adopt preview complete");
    }

    Ok(json!({
        "ok": true,
        "command": "adopt",
        "render_mode": if opts.write { "copy" } else { "preview" },
        "harness_footprint": if copy_result.minimal_footprint {
            "minimal"
        } else {
            "full"
        },
        "template": template.source(),
        "destination": destination.display().to_string(),
        "answers_file": ANSWERS_FILE,
        "git_initialized": false,
        "write": opts.write,
        "detection_report": inference.report(),
        "adoption_profile": inference.adoption_profile_report(
            &copy_result.render_preview.generated_gates,
            &copy_result.render_preview.managed_files,
            &copy_result.render_preview.retired_managed_files,
            &opts.answers,
            &answer_shape,
        ),
        "adoption_review": review.items,
        "render_report": initial_render_report(&copy_result),
        "next_steps": initial_next_steps(
            InitialCommand::Adopt,
            &destination,
            &copy_result,
            false,
        ),
        "notes": initial_notes(
            copy_result.notes,
            copy_result.frontend_apps_configured,
            None,
            copy_result.minimal_footprint,
        ),
    }))
}

pub fn run_update(opts: UpdateOpts) -> Result<Value> {
    let invocation_cwd = bootstrap_invocation_cwd()?;
    let destination = absolute_path_from(&opts.path, &invocation_cwd)?;
    let progress = CliProgress::new("update");
    let mode = if opts.recopy { "recopy" } else { "update" };
    progress.header_for_path(format!("refresh harness ({mode})"), &destination);
    progress.step("validate destination", "adopted repository directory");
    progress.log_blocked_on_err(validate_update_destination(&destination))?;
    let prior_managed_paths = progress
        .log_blocked_on_err(managed_paths::load_manifest(&destination))?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Cannot update this repository because {} is missing. Run `jig adopt . --write` with the current harness footprint to establish exact managed-path ownership, then retry `jig update`.",
                managed_paths::MANIFEST_PATH
            )
        })?;
    let answers_path = destination.join(ANSWERS_FILE);
    progress.step("read answers", answers_path.display());
    let stored = progress.log_blocked_on_err(read_stored_template_state(&answers_path))?;
    progress.step("resolve template", "stored source metadata");
    let update_template = progress.log_blocked_on_err(prepare_update_template_source(
        &opts,
        &stored,
        &invocation_cwd,
    ))?;
    let Some(update_template) = update_template else {
        progress.blocked("stored template source metadata is missing");
        bail!(
            "Missing template source metadata in {ANSWERS_FILE}. Re-adopt the repo before running jig update."
        );
    };
    let answers = progress.log_blocked_on_err(RenderAnswers::from_answers_file(&answers_path))?;
    let reconcile_runtime_config =
        crate::context::RepoContext::validate_config_file(&destination).is_ok();
    let staged = stage_render(RenderStageRequest {
        template: &update_template,
        answers: &answers,
        seed_repo_path: Some(&destination),
        prior_managed_paths: Some(&prior_managed_paths),
        reconcile_runtime_config,
        progress,
    })?;
    let render_report = apply_staged_render(
        &staged,
        &destination,
        ApplyRenderOptions {
            force: opts.force,
            dry_run: false,
            allow_answers_overwrite: true,
            allow_contract_overwrite: false,
            allow_manifest_overwrite: true,
            backup_root: None,
            conflict_message: "Update would overwrite or remove template-managed paths. No files were changed. Re-run with --force to accept the rendered output:",
            progress,
            init_transaction: None,
        },
    )?;
    progress.done("update complete");

    Ok(json!({
        "ok": true,
        "command": "update",
        "render_mode": mode,
        "destination": destination.display().to_string(),
        "answers_file": ANSWERS_FILE,
        "git_initialized": false,
        "render_report": render_report,
    }))
}

fn recognized_prior_answers(destination: &Path) -> Option<RenderAnswers> {
    let answers = RenderAnswers::from_answers_file(&destination.join(ANSWERS_FILE)).ok()?;
    crate::context::RepoContext::validate_config_file(destination).ok()?;
    Some(answers)
}

fn template_progress_label(template: Option<&str>) -> String {
    template.unwrap_or("default jig-sh template").to_string()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InitialCommand {
    Init,
    Adopt,
}

fn initial_next_steps(
    command: InitialCommand,
    destination: &Path,
    result: &initial_copy::BootstrapCopyResult,
    database_config_required: bool,
) -> Vec<String> {
    let destination_for_cd = destination
        .canonicalize()
        .unwrap_or_else(|_| destination.to_path_buf());
    let mut steps = vec![format!(
        "cd {}",
        crate::shell::quote(&destination_for_cd.display().to_string())
    )];
    if command == InitialCommand::Adopt && result.apply_report.dry_run {
        steps.push("Review the adoption preview and managed-file diff.".into());
        if result.minimal_footprint {
            if result.full_to_minimal_transition {
                steps.push("jig adopt . --minimal --write --force".into());
            } else {
                steps.push(
                    "Re-run jig adopt . --minimal --write after reviewing the summary.".into(),
                );
            }
        } else {
            steps.push("Re-run jig adopt . --write after reviewing the summary.".into());
        }
        steps.push("No files were changed by this preview.".into());
        return steps;
    }
    if result.minimal_footprint {
        steps.push(
            "Add [[loop.workflows]] entries to .jig.toml, then run jig loop tick / jig loop run."
                .into(),
        );
        steps.push(
            "Re-run jig adopt . --write (without --minimal) when you want the full harness.".into(),
        );
        if command == InitialCommand::Adopt {
            steps.push("Commit the adoption diff after reviewing .jig.toml and .agent/.".into());
        }
        return steps;
    }
    if database_config_required {
        steps.push(
            "Export DATABASE_URL, or copy .env.example to .env and configure it before bootstrap."
                .into(),
        );
    }
    steps.push("scripts/jig setup".into());
    steps.push("scripts/jig check test".into());
    if result.dev_apps_configured {
        steps.push("scripts/jig dev".into());
    }
    if result.sqlx_enabled {
        steps.push(
            "Run scripts/jig check sqlx after database access is configured; doctor flags missing cargo-sqlx or a build that lacks the configured database driver."
                .into(),
        );
    }
    if result.schema_dump_enabled {
        steps.push("Provide scripts/dump-schema.sh, then run scripts/jig sqlx schema dump.".into());
    }
    if command == InitialCommand::Adopt {
        steps.push("Commit the adoption diff after generated checks pass.".into());
    }
    steps
}

fn initial_notes(
    extra_notes: Vec<String>,
    frontend_apps_configured: bool,
    scaffold_plan: Option<&scaffold::InitScaffoldPlan>,
    minimal_footprint: bool,
) -> Vec<String> {
    let mut notes = if minimal_footprint {
        vec![
            "Minimal adoption wrote .jig.toml and .agent/ scaffolding only; scripts/, workflows, AGENTS.md, agent-map.md, and .mcp.json were omitted.".into(),
            "harness_footprint = \"minimal\" is stored in .jig.toml so jig update keeps the same footprint until you re-adopt without --minimal.".into(),
            "Invoke the installed jig binary directly for loop commands; there is no scripts/jig launcher yet.".into(),
        ]
    } else {
        vec![
            "The first scripts/jig command may install or compile the pinned Jig runtime into this repo's local cache.".into(),
            "Review generated .jig.toml, AGENTS.md, agent-map.md, and check commands before relying on the harness.".into(),
            "Re-run scripts/jig doctor after setup changes to confirm readiness.".into(),
            "Full gates remain available through scripts/jig work gates or scripts/jig check <gate>.".into(),
        ]
    };
    if scaffold_plan.is_some() {
        notes.push(
            "Scaffolded application code is project-owned after creation. jig update keeps the Jig harness current and does not rewrite app code."
                .into(),
        );
    }
    if frontend_apps_configured && !minimal_footprint {
        notes.push(
            "Frontend checks expect package scripts for lint, typecheck, build:bundle, and test:coverage plus a package-manager lockfile; generated preset apps include them."
                .into(),
        );
        notes.push(
            "Frontend gates are available as scripts/jig check typescript-lint, typescript-typecheck, typescript-build, and typescript-coverage."
                .into(),
        );
    }
    if !minimal_footprint {
        notes.push(
            "Policy gates are available as scripts/jig check contract and scripts/jig check agent-guides when evidence is needed."
                .into(),
        );
    }
    if let Some(note) = scaffold_plan.and_then(scaffold::InitScaffoldPlan::sanitized_repo_name_note)
    {
        notes.push(note);
    }
    notes.extend(extra_notes);
    notes
}

fn adopt_backup_root(destination: &Path) -> PathBuf {
    destination
        .join(".agent/.cache/adopt/backups")
        .join(Ulid::new().to_string())
}

fn validate_adopt_output_ancestors(destination: &Path, backup_root: Option<&Path>) -> Result<()> {
    validate_adopt_receipt_paths(destination)?;
    if let Some(backup_root) = backup_root {
        let backup_relative = backup_root.strip_prefix(destination).with_context(|| {
            format!(
                "Backup destination {} must be contained by repository root {}",
                backup_root.display(),
                destination.display()
            )
        })?;
        validate_repository_relative_ancestors(destination, &backup_relative.join("preflight"))?;
    }
    Ok(())
}

fn validate_adopt_receipt_paths(destination: &Path) -> Result<()> {
    for relative in ADOPT_RECEIPT_PATHS.map(Path::new) {
        validate_repository_relative_ancestors(destination, relative)?;
        let receipt_path = destination.join(relative);
        match fs::symlink_metadata(&receipt_path) {
            Ok(metadata) if metadata.file_type().is_file() => {}
            Ok(_) => {
                bail!(
                    "Adopt receipt path must be missing or a regular file, not a symlink, directory, or other file type: {}",
                    receipt_path.display()
                );
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Failed to stat {}", receipt_path.display()));
            }
        }
    }
    Ok(())
}

fn confirm_adopt_write(opts: &AdoptOpts) -> Result<()> {
    if opts.defaults || opts.no_input {
        return Ok(());
    }
    let stdin = io::stdin();
    let mut stderr = io::stderr();
    if !stdin.is_terminal() || !stderr.is_terminal() {
        bail!(
            "Adopt write needs confirmation but stdin or stderr is not a terminal. Re-run interactively, or pass --defaults or --no-input for noninteractive execution."
        );
    }

    write!(stderr, "Proceed with adopt --write? [y/N] ")
        .context("Failed to write adopt confirmation prompt")?;
    stderr
        .flush()
        .context("Failed to flush adopt confirmation prompt")?;
    let mut answer = String::new();
    stdin
        .read_line(&mut answer)
        .context("Failed to read adopt confirmation")?;
    if matches!(answer.trim(), "y" | "Y" | "yes" | "YES" | "Yes") {
        return Ok(());
    }
    bail!("Adopt write cancelled; re-run with --defaults or --no-input to skip confirmation.");
}

fn write_adopt_last_receipt(
    destination: &Path,
    backup_root: Option<&Path>,
    result: &initial_copy::BootstrapCopyResult,
) -> Result<()> {
    validate_adopt_output_ancestors(destination, backup_root)?;
    let receipt = json!({
        "command": "adopt",
        "created_at_unix": OffsetDateTime::now_utc().unix_timestamp(),
        "destination": destination.display().to_string(),
        "backup_root": backup_root.map(|path| path.display().to_string()),
        "canonical_receipt_path": ADOPT_RECEIPT_PATH,
        "legacy_receipt_path": LEGACY_ADOPT_RECEIPT_PATH,
        "legacy_receipt_deprecated": true,
        "apply_report": &result.apply_report,
        "undo_hint": "Use apply_report.backups to restore modified or removed files, then delete paths listed in apply_report.files_created if you want to undo this adopt write. Delete backup_root when those backups are no longer needed.",
    });
    let text =
        serde_json::to_string_pretty(&receipt).context("Failed to serialize adopt receipt")?;
    let bytes = format!("{text}\n");
    write_adopt_receipt_atomic(destination, Path::new(ADOPT_RECEIPT_PATH), bytes.as_bytes())?;
    // TODO(jig-0.4): remove the legacy receipt copy after adopted repos have
    // had a release window to migrate readers to the canonical cache path.
    write_adopt_receipt_atomic(
        destination,
        Path::new(LEGACY_ADOPT_RECEIPT_PATH),
        bytes.as_bytes(),
    )?;
    Ok(())
}

fn write_adopt_receipt_atomic(destination: &Path, relative: &Path, bytes: &[u8]) -> Result<()> {
    validate_adopt_receipt_paths(destination)?;
    let receipt_path = destination.join(relative);
    let parent = receipt_path.parent().with_context(|| {
        format!(
            "Adopt receipt path has no parent: {}",
            receipt_path.display()
        )
    })?;
    fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
    validate_adopt_receipt_paths(destination)?;

    let existing_permissions = match fs::symlink_metadata(&receipt_path) {
        Ok(metadata) => Some(metadata.permissions()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Failed to stat {}", receipt_path.display()));
        }
    };
    #[cfg(unix)]
    let temp_builder = {
        use std::os::unix::fs::PermissionsExt;

        let mut builder = TempFileBuilder::new();
        if existing_permissions.is_none() {
            builder.permissions(fs::Permissions::from_mode(0o666));
        }
        builder
    };
    #[cfg(not(unix))]
    let temp_builder = TempFileBuilder::new();
    let mut temp = temp_builder.tempfile_in(parent).with_context(|| {
        format!(
            "Failed to create temporary adopt receipt in {}",
            parent.display()
        )
    })?;
    if let Some(permissions) = existing_permissions {
        temp.as_file()
            .set_permissions(permissions)
            .with_context(|| {
                format!(
                    "Failed to preserve permissions for {}",
                    receipt_path.display()
                )
            })?;
    }
    temp.write_all(bytes).with_context(|| {
        format!(
            "Failed to write temporary adopt receipt for {}",
            receipt_path.display()
        )
    })?;
    temp.as_file().sync_all().with_context(|| {
        format!(
            "Failed to sync temporary adopt receipt for {}",
            receipt_path.display()
        )
    })?;

    validate_adopt_receipt_paths(destination)?;
    temp.persist(&receipt_path)
        .map(|_| ())
        .map_err(|error| error.error)
        .with_context(|| format!("Failed to write {}", receipt_path.display()))
}

fn initial_render_report(result: &initial_copy::BootstrapCopyResult) -> Value {
    json!({
        "dry_run": result.apply_report.dry_run,
        "active_managed_paths": &result.apply_report.active_managed_paths,
        "retired_managed_paths": &result.apply_report.retired_managed_paths,
        "files_created": &result.apply_report.files_created,
        "files_modified": &result.apply_report.files_modified,
        "files_removed": &result.apply_report.files_removed,
        "files_unchanged": &result.apply_report.files_unchanged,
        "managed_blocks_inserted": &result.apply_report.managed_blocks_inserted,
        "managed_blocks_rendered": &result.apply_report.managed_blocks_rendered,
        "backups": &result.apply_report.backups,
        "conflicts": &result.apply_report.conflicts,
        "commands_detected_or_skipped": initial_command_report(result),
        "todos": initial_todos(result),
        "suggested_jig_toml_edits": initial_suggested_jig_toml_edits(result),
    })
}

fn initial_command_report(result: &initial_copy::BootstrapCopyResult) -> Vec<String> {
    let launcher = gate_preview::jig_launcher(result.minimal_footprint);
    let mut commands = Vec::new();
    if result.bootstrap_command_configured {
        commands.push(format!(
            "bootstrap_command configured; run {launcher} bootstrap before checks"
        ));
    } else {
        commands.push(format!(
            "bootstrap_command not configured; skip {launcher} bootstrap"
        ));
    }
    commands.push(format!(
        "contract check available through {launcher} check contract"
    ));
    if result.dev_apps_configured {
        commands.push(format!("[[dev.apps]] configured; run {launcher} dev"));
    } else {
        commands.push(format!(
            "no [[dev.apps]] configured; {launcher} dev has no app to launch"
        ));
    }
    if result.frontend_apps_configured && !result.minimal_footprint {
        commands.push(format!(
            "frontend app checks available through {launcher} check typescript-*"
        ));
    }
    commands
}

fn initial_todos(result: &initial_copy::BootstrapCopyResult) -> Vec<String> {
    let mut todos = vec![
        "Review generated command strings in .jig.toml against this repo's actual setup.".into(),
        "Add or update crate-level AGENTS.md files for repo-owned business rules.".into(),
    ];
    if result.sqlx_enabled {
        todos.push("Confirm SQLx database access and committed metadata workflow.".into());
    }
    if result.schema_dump_enabled {
        todos.push("Provide the project-owned scripts/dump-schema.sh implementation.".into());
    }
    if result.frontend_apps_configured && !result.minimal_footprint {
        todos.push(
            "Confirm each frontend app has package scripts and starts on the injected PORT/HOST."
                .into(),
        );
    }
    todos
}

fn initial_suggested_jig_toml_edits(result: &initial_copy::BootstrapCopyResult) -> Vec<String> {
    let mut edits = vec![
        "Replace generated fallback Cargo commands if this repo uses nested workspaces or non-Cargo checks.".into(),
    ];
    if result.dev_apps_configured {
        edits.push("Tune [dev] ports, tld, HTTPS, LAN, and each [[dev.apps]] kind/argv if defaults do not match local development.".into());
    }
    if result.sqlx_enabled {
        edits.push("Set rust_migration_dir, rust_sqlx_metadata_dir, and sqlx_check_command to the repo-owned SQLx layout.".into());
    }
    edits
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

fn parse_frontend_app(value: &str) -> Result<FrontendApp, String> {
    let parts = value.split(':').collect::<Vec<_>>();
    if !(3..=5).contains(&parts.len()) {
        return Err("expected <name>:<dir>:<coverage_threshold>[:kind[:role]]".into());
    }

    let coverage_threshold = parts[2]
        .parse::<u32>()
        .map_err(|error| format!("coverage_threshold must be a non-negative integer: {error}"))?;

    let metadata =
        resolve_frontend_metadata(parts[0], parts.get(3).copied(), parts.get(4).copied(), None);
    let app = FrontendApp {
        name: parts[0].to_string(),
        dir: parts[1].to_string(),
        coverage_threshold,
        kind: metadata.kind.to_string(),
        role: metadata.role.to_string(),
    };
    answers::validate_frontend_apps(std::slice::from_ref(&app))
        .map_err(|error| error.to_string())?;
    Ok(app)
}

pub(crate) fn parse_scaffold_frontend(value: &str) -> Result<ScaffoldFrontend, String> {
    let (raw_name, explicit_kind) = value
        .split_once(':')
        .map_or((value, None), |(name, kind)| (name, Some(kind)));
    let name = match raw_name {
        "admin" => "admin-panel",
        other => other,
    };
    // Generated JS and HTML interpolate frontend titles directly, so these
    // rules must stay narrow unless the scaffold templates add escaping.
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err("frontend name must use ASCII letters, numbers, '-' or '_'".into());
    }
    if !name.chars().any(|ch| ch.is_ascii_alphanumeric()) {
        return Err("frontend name must include at least one ASCII letter or number".into());
    }
    let kind = match explicit_kind {
        Some(kind) => parse_scaffold_frontend_kind(kind)?,
        None => match raw_name {
            "admin" | "admin-panel" => ScaffoldFrontendKind::Admin,
            "landing" | "marketing" | "astro" => ScaffoldFrontendKind::Astro,
            _ => ScaffoldFrontendKind::Spa,
        },
    };
    Ok(ScaffoldFrontend {
        name: name.to_string(),
        kind,
        custom_default_name: explicit_kind.is_none()
            && !matches!(
                raw_name,
                "web" | "admin" | "admin-panel" | "landing" | "marketing" | "astro"
            ),
    })
}

impl ScaffoldFrontend {
    pub(crate) fn custom_default_name_notice(&self) -> Option<String> {
        self.custom_default_name.then(|| {
            format!(
                "'{}' isn't a preset shorthand — scaffolding a {} in {}/.",
                self.name,
                self.kind.custom_scaffold_label(),
                self.name
            )
        })
    }
}

impl ScaffoldFrontendKind {
    const fn custom_scaffold_label(self) -> &'static str {
        match self {
            Self::Spa => "custom Vite SPA",
            Self::Admin => "custom Vite admin app",
            Self::Astro => "custom Astro site",
        }
    }
}

impl ScaffoldOpts {
    pub(crate) fn normalize_minimal_harness_shape(&mut self, answers: &AnswerOpts) {
        if answers.harness_footprint == Some(HarnessFootprint::Minimal) && self.preset.is_none() {
            self.preset = Some(ScaffoldPreset::HarnessOnly);
        }
    }

    pub(crate) fn has_frontends(&self) -> bool {
        !self.frontends.is_empty() || !self.frontend_list.is_empty()
    }

    pub(crate) fn custom_frontend_notices(&self) -> Vec<String> {
        self.frontends
            .iter()
            .chain(self.frontend_list.iter())
            .filter_map(ScaffoldFrontend::custom_default_name_notice)
            .collect()
    }

    pub(crate) fn validate_init_invariants(&self, answers: &AnswerOpts) -> Result<()> {
        if answers.harness_footprint == Some(HarnessFootprint::Minimal)
            && (self.preset == Some(ScaffoldPreset::RustReact)
                || self.db.is_some()
                || self.has_frontends())
        {
            bail!(
                "Init cannot combine harness_footprint = \"minimal\" with a Rust React scaffold; remove --preset rust-react and its database/frontend options, or use harness_footprint = \"full\""
            );
        }
        if self.preset == Some(ScaffoldPreset::HarnessOnly)
            && (self.db.is_some() || self.has_frontends())
        {
            bail!(
                "--preset harness-only cannot be combined with --db, --frontend, or --frontends; remove the scaffold flags or use --preset rust-react"
            );
        }
        if self.preset == Some(ScaffoldPreset::RustReact) {
            let reserved_backends = [
                RUST_REACT_BACKEND_DEV_APP_NAME,
                RUST_REACT_ADMIN_BACKEND_DEV_APP_NAME,
            ];
            for frontend_name in self
                .frontends
                .iter()
                .chain(self.frontend_list.iter())
                .map(|frontend| frontend.name.as_str())
                .chain(
                    answers
                        .frontend_apps
                        .iter()
                        .map(|frontend| frontend.name.as_str()),
                )
            {
                for backend_name in reserved_backends {
                    let backend_prefix = jig_core::dev_app_env_prefix(backend_name);
                    if jig_core::dev_app_env_prefix(frontend_name) == backend_prefix {
                        bail!(
                            "Rust React frontend app name '{frontend_name}' conflicts with the reserved backend dev app '{backend_name}' because both derive dev environment prefix {backend_prefix}; choose another frontend name"
                        );
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn apply_init_answer_defaults(&self, answers: &mut AnswerOpts) {
        if self.preset == Some(ScaffoldPreset::HarnessOnly)
            && should_default_init_sqlx_disabled(answers)
        {
            answers.sqlx_enabled = Some(false);
        }
    }
}

fn parse_scaffold_frontend_kind(value: &str) -> Result<ScaffoldFrontendKind, String> {
    Ok(match value {
        "web" | "spa" => ScaffoldFrontendKind::Spa,
        "admin" | "admin-panel" => ScaffoldFrontendKind::Admin,
        "landing" | "marketing" | "astro" => ScaffoldFrontendKind::Astro,
        other => {
            return Err(format!(
                "unsupported frontend kind '{other}'. Expected spa, admin, or astro"
            ));
        }
    })
}
fn default_dev_app_kind() -> String {
    "env-port".into()
}

const fn default_true() -> bool {
    true
}

fn validate_init_destination(path: &Path, force: bool) -> Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Failed to inspect init destination {}", path.display()));
        }
    };
    if !metadata.file_type().is_dir() {
        bail!(
            "Init destination is not a real directory: {}",
            path.display()
        );
    }

    let first_entry = fs::read_dir(path)?
        .next()
        .transpose()
        .with_context(|| format!("Failed to enumerate {}", path.display()))?;
    if first_entry.is_none() || force {
        return Ok(());
    }

    bail!(
        "Init destination is not empty: {}. Re-run with --force to overwrite.",
        path.display()
    );
}

fn validate_adopt_destination(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("Adopt destination does not exist: {}", path.display());
    }
    if !path.is_dir() {
        bail!("Adopt destination is not a directory: {}", path.display());
    }
    Ok(())
}

fn validate_update_destination(path: &Path) -> Result<()> {
    validate_adopt_destination(path)?;
    let answers_path = path.join(ANSWERS_FILE);
    if !answers_path.exists() {
        bail!(
            "Update destination does not contain {}: {}",
            ANSWERS_FILE,
            path.display()
        );
    }
    Ok(())
}

pub(crate) fn external_program(env_key: &str, fallback: &str) -> String {
    env::var(env_key).unwrap_or_else(|_| fallback.to_string())
}

#[cfg(test)]
mod tests;
