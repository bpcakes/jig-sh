use std::borrow::Cow;
use std::cell::Cell;
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
use git::init_git_repo_with_validation;
#[cfg(test)]
use git::{git, git_stdout};
#[cfg(test)]
use initial_copy::seed_answers_toml;
use initial_copy::{BootstrapCopyRequest, render_and_copy_bootstrap_template};
use path::{absolute_path_from, bootstrap_invocation_cwd, validate_repository_relative_ancestors};
#[cfg(test)]
use preview_seed::seed_preview_workspace;
use renderer::{RenderStageRequest, stage_render};
#[cfg(test)]
use sync::rendered_conflicts;
use sync::{ApplyRenderOptions, apply_staged_render};
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
mod initial_copy;
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
pub use opts::AnswerOpts;
pub use presets::scaffold_presets_report;

const ANSWERS_FILE: &str = ".jig.toml";
const ADOPT_RECEIPT_PATH: &str = ".agent/.cache/adopt/adopt-last.json";
const LEGACY_ADOPT_RECEIPT_PATH: &str = ".agent/state/adopt-last.json";
const ADOPT_RECEIPT_PATHS: [&str; 2] = [ADOPT_RECEIPT_PATH, LEGACY_ADOPT_RECEIPT_PATH];
const GIT_BIN_ENV: &str = "JIG_GIT_BIN";
const BUILD_TEMPLATE_PIN_RELEASED: &str = "released";
const BUILD_TEMPLATE_PIN_UNRELEASED: &str = "unreleased";
const OFFICIAL_TEMPLATE_SOURCE: &str = "https://github.com/bpcakes/jig-sh.git";
const REMOTE_TEMPLATE_MODE_ERROR: &str = "--template-mode only applies to local git template paths. Omit --template-mode for remote templates, or pass --template /path/to/jig-sh --template-mode committed.";
// Legacy conflict helpers keep these in sync with template task side effects.
#[cfg(test)]
const ALWAYS_TASK_MUTATED_PATHS: &[&str] = &[".jig.toml", "agent-map.md"];
const TEMPLATE_MODE_KEY: &str = "_template_mode";
const TEMPLATE_LOCAL_PATH_KEY: &str = "_template_local_path";
const GENERATED_NODE_VERSION: &str = "22.22.2";
const GENERATED_NODE_TYPES_VERSION: &str = "22.20.1";
pub(crate) const RUST_REACT_BACKEND_DEV_APP_NAME: &str = "api";

fn generated_package_manager_spec(package_manager: &str) -> &'static str {
    match package_manager {
        "bun" => "bun@1.3.14",
        "npm" => "npm@12.0.1",
        "pnpm" => "pnpm@11.13.0",
        "yarn" => "yarn@4.17.1",
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
        help = "Frontend scaffold as name[:kind], e.g. web:spa, landing:astro, admin-panel:admin; may be repeated. Bare web, landing, and admin use preset shorthands. Rust-react reserves api for its backend dev app."
    )]
    pub frontends: Vec<ScaffoldFrontend>,
    #[arg(
        long = "frontends",
        help_heading = "Project Shape",
        value_delimiter = ',',
        value_parser = parse_scaffold_frontend,
        help = "Comma-separated frontend scaffolds, e.g. web,landing,admin. Bare web, landing, and admin use preset shorthands. Rust-react reserves api for its backend dev app."
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
    pub(super) fn as_str(self) -> &'static str {
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

#[derive(Clone, Debug)]
enum InitPathSnapshot {
    Missing,
    Regular(path::RepositoryFileCommit),
    Symlink {
        identity: path::RepositoryEntryIdentity,
        target: PathBuf,
        target_is_directory: bool,
        _handle: Arc<fs::File>,
    },
}

#[derive(Clone, Debug)]
struct InitFileMutation {
    before: InitPathSnapshot,
    expected_jig_states: Vec<InitPathSnapshot>,
    original_quarantine: Option<PathBuf>,
}

struct InitMutationTransaction {
    final_destination: PathBuf,
    destination: PathBuf,
    destination_identity: path::RepositoryDirectoryCommit,
    staged_publication: Option<StagedInitPublication>,
    write_staging: BTreeMap<PathBuf, InitWriteStagingDirectory>,
    next_snapshot: Cell<u64>,
    files: BTreeMap<PathBuf, InitFileMutation>,
    directory_identities: BTreeMap<PathBuf, path::RepositoryDirectoryCommit>,
    owned_directories: BTreeMap<PathBuf, path::RepositoryDirectoryCommit>,
    existing_generation_budget_sealed: bool,
    armed: bool,
}

const MAX_EXISTING_INIT_RETAINED_GENERATIONS: usize = 256;
const RETAINED_GENERATION_HANDLES_PER_PATH: usize = 2;
const RETAINED_GENERATION_HANDLE_HEADROOM: usize = 32;

fn retained_generation_handle_requirement(
    planned: &BTreeSet<PathBuf>,
    repeated_generation_count: usize,
) -> usize {
    let mut directory_prefixes = BTreeSet::new();
    let mut target_parents = BTreeSet::new();
    for relative in planned {
        let parent = relative.parent().unwrap_or(Path::new(""));
        target_parents.insert(parent.to_path_buf());
        let mut prefix = PathBuf::new();
        for component in parent.components() {
            prefix.push(component.as_os_str());
            directory_prefixes.insert(prefix.clone());
        }
    }
    planned
        .len()
        .saturating_mul(RETAINED_GENERATION_HANDLES_PER_PATH)
        .saturating_add(repeated_generation_count)
        .saturating_add(directory_prefixes.len())
        .saturating_add(target_parents.len())
        .saturating_add(RETAINED_GENERATION_HANDLE_HEADROOM)
}

#[cfg(unix)]
fn process_soft_handle_limit() -> Option<usize> {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `limit` is valid writable storage for `getrlimit`.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0
        || limit.rlim_cur == libc::RLIM_INFINITY
    {
        return None;
    }
    usize::try_from(limit.rlim_cur).ok()
}

#[cfg(not(unix))]
fn process_soft_handle_limit() -> Option<usize> {
    None
}

#[cfg(unix)]
fn current_open_handle_count() -> usize {
    [Path::new("/proc/self/fd"), Path::new("/dev/fd")]
        .into_iter()
        .find_map(|directory| fs::read_dir(directory).ok())
        .map(|entries| entries.filter_map(std::result::Result::ok).count())
        .unwrap_or(RETAINED_GENERATION_HANDLE_HEADROOM)
}

#[cfg(not(unix))]
fn current_open_handle_count() -> usize {
    RETAINED_GENERATION_HANDLE_HEADROOM
}

fn validate_retained_generation_budget(
    planned: &BTreeSet<PathBuf>,
    repeated_generation_count: usize,
    soft_limit: Option<usize>,
    open_handles: usize,
) -> Result<()> {
    let planned_generation_count = planned.len().saturating_add(repeated_generation_count);
    if planned_generation_count > MAX_EXISTING_INIT_RETAINED_GENERATIONS {
        bail!(
            "Existing-destination init plans {} generated file generations, exceeding the safe retained-generation limit of {}. Use a wholly missing destination so Jig can publish one privately staged tree, or reduce the explicit template/scaffold output set.",
            planned_generation_count,
            MAX_EXISTING_INIT_RETAINED_GENERATIONS
        );
    }
    // Every planned leaf may acquire a preimage before Jig retains its first
    // snapshot, so reserve both that preimage and the first Jig generation
    // without relying on the leaf's current existence. Additional publications
    // are counted explicitly. Current/quarantine/disposal snapshots are
    // processed one leaf at a time and fit within the fixed transient headroom.
    let required = retained_generation_handle_requirement(planned, repeated_generation_count);
    if soft_limit.is_some_and(|limit| open_handles.saturating_add(required) > limit) {
        bail!(
            "Existing-destination init needs capacity for approximately {required} retained file/directory handles in addition to {open_handles} already open handles, but the process soft handle limit is {}. Use a wholly missing destination, reduce the output set, or raise the process file-descriptor limit before retrying.",
            soft_limit.expect("checked above")
        );
    }
    Ok(())
}

struct StagedInitPublication {
    staging_root: Option<TempDir>,
    publish_source: PathBuf,
    publish_source_identity: path::RepositoryDirectoryCommit,
    publish_destination: PathBuf,
    publish_parent_identity: path::RepositoryDirectoryCommit,
    publish_permissions: fs::Permissions,
}

struct InitWriteStagingDirectory {
    directory: TempDir,
    identity: path::RepositoryDirectoryCommit,
}

fn verify_tracked_init_directories(
    directories: &BTreeMap<PathBuf, path::RepositoryDirectoryCommit>,
) -> Result<()> {
    for (directory, expected) in directories {
        let current = path::repository_directory_commit_matches_path(expected, directory)
            .with_context(|| {
                format!(
                    "Init output ancestor was replaced while init was running: {}",
                    directory.display()
                )
            })?;
        if !current {
            bail!(
                "Init output ancestor was replaced while init was running; refusing to mutate replacement directory {}",
                directory.display()
            );
        }
    }
    Ok(())
}

impl InitMutationTransaction {
    fn create(destination: &Path) -> Result<Self> {
        let (existing_ancestor, missing_tail) = path::split_existing_ancestor(destination)?;
        path::ensure_atomic_noreplace_publication_supported(&existing_ancestor)?;
        if !missing_tail.is_empty() {
            let mut staging_builder = TempFileBuilder::new();
            staging_builder.prefix(".jig-init-stage-");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;

                staging_builder.permissions(fs::Permissions::from_mode(0o700));
            }
            let staging_root = staging_builder
                .tempdir_in(&existing_ancestor)
                .with_context(|| {
                    format!(
                        "Failed to create private init staging directory in {}",
                        existing_ancestor.display()
                    )
                })?;
            let publish_source = staging_root.path().to_path_buf();
            let publish_source_identity = match path::repository_directory_commit_at(
                &publish_source,
            ) {
                Ok(identity) => identity,
                Err(primary) => {
                    let preserved = staging_root.keep();
                    bail!(
                        "{primary:#}\nCould not prove ownership of newly created private init staging; preserving it at {}",
                        preserved.display()
                    );
                }
            };
            let mut directory_identities =
                BTreeMap::from([(publish_source.clone(), publish_source_identity.clone())]);
            let setup = (|| -> Result<(
                fs::Permissions,
                path::RepositoryDirectoryCommit,
                PathBuf,
                path::RepositoryDirectoryCommit,
            )> {
                let permission_probe = staging_root.path().join(".jig-directory-mode-probe");
                fs::create_dir(&permission_probe).with_context(|| {
                    format!(
                        "Failed to probe final init directory permissions in {}",
                        staging_root.path().display()
                    )
                })?;
                let publish_permissions = fs::metadata(&permission_probe)
                    .with_context(|| {
                        format!(
                            "Failed to inspect final init directory permission probe {}",
                            permission_probe.display()
                        )
                    })?
                    .permissions();
                fs::remove_dir(&permission_probe).with_context(|| {
                    format!(
                        "Failed to remove final init directory permission probe {}",
                        permission_probe.display()
                    )
                })?;
                let publish_parent_identity =
                    path::repository_directory_commit_at(&existing_ancestor)?;
                let mut work_destination = staging_root.path().to_path_buf();
                for component in missing_tail.iter().skip(1) {
                    verify_tracked_init_directories(&directory_identities)?;
                    work_destination.push(component);
                    fs::create_dir(&work_destination).with_context(|| {
                        format!(
                            "Failed to create private init work-tree ancestor {}",
                            work_destination.display()
                        )
                    })?;
                    let identity =
                        path::repository_directory_commit_at(&work_destination).with_context(
                            || {
                                format!(
                                    "Private init work-tree ancestor is not a stable real directory: {}",
                                    work_destination.display()
                                )
                            },
                        )?;
                    directory_identities.insert(work_destination.clone(), identity);
                }
                verify_tracked_init_directories(&directory_identities)?;
                let destination_identity = directory_identities
                    .get(&work_destination)
                    .context("Private init work destination was not retained")?
                    .clone();
                Ok((
                    publish_permissions,
                    publish_parent_identity,
                    work_destination,
                    destination_identity,
                ))
            })();
            let (
                publish_permissions,
                publish_parent_identity,
                work_destination,
                destination_identity,
            ) = match setup {
                Ok(setup) => setup,
                Err(primary) => {
                    if let Err(boundary) = verify_tracked_init_directories(&directory_identities) {
                        let preserved = staging_root.keep();
                        bail!(
                            "{primary:#}\nPrivate init staging changed during setup ({boundary:#}); preserving the complete staging tree at {}",
                            preserved.display()
                        );
                    }
                    drop(directory_identities);
                    return Err(close_failed_staging(
                        staging_root,
                        &publish_source_identity,
                        primary,
                    ));
                }
            };
            let publish_destination = existing_ancestor.join(&missing_tail[0]);
            return Ok(Self {
                final_destination: destination.to_path_buf(),
                destination: work_destination,
                destination_identity,
                staged_publication: Some(StagedInitPublication {
                    staging_root: Some(staging_root),
                    publish_source,
                    publish_source_identity,
                    publish_destination,
                    publish_parent_identity,
                    publish_permissions,
                }),
                write_staging: BTreeMap::new(),
                next_snapshot: Cell::new(0),
                files: BTreeMap::new(),
                directory_identities,
                owned_directories: BTreeMap::new(),
                existing_generation_budget_sealed: false,
                armed: true,
            });
        }

        let metadata = fs::symlink_metadata(&existing_ancestor).with_context(|| {
            format!(
                "Failed to inspect init destination {}",
                existing_ancestor.display()
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            bail!(
                "Init destination must be a real directory: {}",
                existing_ancestor.display()
            );
        }
        Ok(Self {
            final_destination: destination.to_path_buf(),
            destination: existing_ancestor.clone(),
            destination_identity: path::repository_directory_commit_at(&existing_ancestor)?,
            staged_publication: None,
            write_staging: BTreeMap::new(),
            next_snapshot: Cell::new(0),
            files: BTreeMap::new(),
            directory_identities: BTreeMap::from([(
                existing_ancestor.clone(),
                path::repository_directory_commit_at(&existing_ancestor)?,
            )]),
            owned_directories: BTreeMap::new(),
            existing_generation_budget_sealed: false,
            armed: true,
        })
    }

    fn work_destination(&self) -> &Path {
        &self.destination
    }

    fn is_privately_staged(&self) -> bool {
        self.staged_publication.is_some()
    }

    fn verify_destination_identity(&self) -> Result<()> {
        let current = path::repository_directory_commit_matches_path(
            &self.destination_identity,
            &self.destination,
        )
        .with_context(|| {
            format!(
                "Init destination was replaced while init was running: {}",
                self.final_destination.display()
            )
        })?;
        if !current {
            bail!(
                "Init destination was replaced while init was running; refusing to mutate replacement path {}",
                self.final_destination.display()
            );
        }
        verify_tracked_init_directories(&self.directory_identities)?;
        for staging in self.write_staging.values() {
            if !path::repository_directory_commit_matches_path(
                &staging.identity,
                staging.directory.path(),
            )? {
                bail!(
                    "Private init write staging was replaced concurrently: {}",
                    staging.directory.path().display()
                );
            }
        }
        Ok(())
    }

    fn verify_rollback_root_and_preexisting_ancestors(&self) -> Result<()> {
        let current = path::repository_directory_commit_matches_path(
            &self.destination_identity,
            &self.destination,
        )?;
        if !current {
            bail!(
                "Init destination was replaced while rollback was starting: {}",
                self.final_destination.display()
            );
        }
        for (directory, expected) in &self.directory_identities {
            if self.owned_directories.contains_key(directory) {
                continue;
            }
            if !path::repository_directory_commit_matches_path(expected, directory)? {
                bail!(
                    "Pre-existing init output ancestor was replaced while rollback was starting: {}",
                    directory.display()
                );
            }
        }
        Ok(())
    }

    fn changed_owned_ancestor(&self, relative: &Path) -> Result<Option<PathBuf>> {
        let output = self.destination.join(relative);
        for (directory, expected) in &self.owned_directories {
            if !output.starts_with(directory) {
                continue;
            }
            match path::repository_directory_commit_matches_path(expected, directory) {
                Ok(true) => {}
                Ok(false) => return Ok(Some(directory.clone())),
                Err(error)
                    if error
                        .downcast_ref::<io::Error>()
                        .is_some_and(|error| error.kind() == io::ErrorKind::NotFound) =>
                {
                    return Ok(Some(directory.clone()));
                }
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }

    fn ensure_write_staging(&mut self, relative: &Path) -> Result<()> {
        if self.is_privately_staged() {
            return Ok(());
        }
        let output = self.destination.join(relative);
        let parent = output
            .parent()
            .with_context(|| format!("Init output has no parent: {}", output.display()))?
            .to_path_buf();
        if self.write_staging.contains_key(&parent) {
            return Ok(());
        }
        let mut builder = TempFileBuilder::new();
        builder.prefix(".jig-init-writes-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            builder.permissions(fs::Permissions::from_mode(0o700));
        }
        let outside_root = self.destination.parent().unwrap_or(&self.destination);
        let directory = if path::repository_paths_same_filesystem(&parent, outside_root)? {
            match builder.tempdir_in(outside_root) {
                Ok(directory) => directory,
                Err(outside_error) => builder.tempdir_in(&parent).with_context(|| {
                    format!(
                        "Failed to create same-filesystem private init write staging in {} after sibling staging in {} failed: {outside_error}",
                        parent.display(),
                        outside_root.display()
                    )
                })?,
            }
        } else {
            builder.tempdir_in(&parent).with_context(|| {
                format!(
                    "Failed to create same-filesystem private init write staging in {}",
                    parent.display()
                )
            })?
        };
        let identity = path::repository_directory_commit_at(directory.path())?;
        self.write_staging.insert(
            parent,
            InitWriteStagingDirectory {
                directory,
                identity,
            },
        );
        Ok(())
    }

    fn write_staging_path(&self, relative: &Path) -> Option<&Path> {
        self.destination
            .join(relative)
            .parent()
            .and_then(|parent| self.write_staging.get(parent))
            .map(|staging| staging.directory.path())
    }

    fn publication_permissions(&self, relative: &Path) -> Result<Option<fs::Permissions>> {
        let Some(mutation) = self.files.get(relative) else {
            return Ok(None);
        };
        let state = mutation
            .expected_jig_states
            .last()
            .unwrap_or(&mutation.before);
        match state {
            InitPathSnapshot::Regular(commit) => commit
                ._handle
                .metadata()
                .map(|metadata| Some(metadata.permissions()))
                .with_context(|| {
                    format!(
                        "Failed to inspect retained permissions for {}",
                        relative.display()
                    )
                }),
            InitPathSnapshot::Missing | InitPathSnapshot::Symlink { .. } => Ok(None),
        }
    }

    fn close_write_staging(&mut self) -> Result<()> {
        let staging = std::mem::take(&mut self.write_staging);
        let mut failures = Vec::new();
        for (_, staging) in staging {
            match path::repository_directory_commit_matches_path(
                &staging.identity,
                staging.directory.path(),
            ) {
                Ok(true) => {
                    drop(staging.identity);
                    if let Err(error) = staging.directory.close() {
                        failures.push(format!(
                            "failed to remove private init write staging: {error}"
                        ));
                    }
                }
                Ok(false) => {
                    let preserved = staging.directory.keep();
                    failures.push(format!(
                        "private init write staging was replaced concurrently; preserving foreign replacement {}",
                        preserved.display()
                    ));
                }
                Err(error) => {
                    let preserved = staging.directory.keep();
                    failures.push(format!(
                        "could not verify private init write staging {} for cleanup ({error:#}); preserving it",
                        preserved.display()
                    ));
                }
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            bail!("{}", failures.join("\n"))
        }
    }

    fn commit(&mut self) -> Result<()> {
        if !self.armed {
            return Ok(());
        }
        let staged_boundary = self
            .staged_publication
            .as_ref()
            .map(|_| verify_tracked_init_directories(&self.directory_identities));
        if let Some(publication) = self.staged_publication.as_mut() {
            let staging_root = publication
                .staging_root
                .take()
                .context("Private init staging root was already consumed")?;
            let publish_source = staging_root.path().to_path_buf();
            debug_assert_eq!(publish_source, publication.publish_source);
            if let Some(Err(error)) = staged_boundary {
                let preserved = staging_root.keep();
                return Err(anyhow::anyhow!(
                    "Private init work tree changed before publication: {error:#}. Preserving the complete staging tree at {}",
                    preserved.display()
                ));
            }
            let publish_parent = publication
                .publish_destination
                .parent()
                .context("Init publication destination has no parent")?;
            let parent_identity = match path::repository_path_identity(publish_parent) {
                Ok(identity) => identity,
                Err(error) => {
                    let primary = anyhow::anyhow!(
                        "Failed to verify init publication parent {}: {error:#}",
                        publish_parent.display()
                    );
                    return Err(close_failed_staging(
                        staging_root,
                        &publication.publish_source_identity,
                        primary,
                    ));
                }
            };
            if parent_identity != publication.publish_parent_identity.identity {
                let primary = anyhow::anyhow!(
                    "Init publication parent changed concurrently: {}",
                    publish_parent.display()
                );
                return Err(close_failed_staging(
                    staging_root,
                    &publication.publish_source_identity,
                    primary,
                ));
            }
            let source_identity = match path::repository_path_identity(&publish_source) {
                Ok(identity) => identity,
                Err(error) => {
                    let preserved = staging_root.keep();
                    return Err(anyhow::anyhow!(
                        "Failed to verify private init staging root {}: {error:#}. Preserving unverified staging path {}",
                        publish_source.display(),
                        preserved.display()
                    ));
                }
            };
            if source_identity != publication.publish_source_identity.identity {
                let primary = anyhow::anyhow!(
                    "Private init staging root changed concurrently: {}",
                    publish_source.display()
                );
                // The path is no longer proven to be ours. Disarm recursive
                // cleanup and surface the recovery path rather than deleting a
                // foreign replacement.
                let preserved = staging_root.keep();
                return Err(anyhow::anyhow!(
                    "{primary:#}\nPreserving unverified staging path {}",
                    preserved.display()
                ));
            }
            if let Err(error) =
                fs::set_permissions(&publish_source, publication.publish_permissions.clone())
            {
                let primary = anyhow::Error::new(error).context(format!(
                    "Failed to apply final directory permissions before publishing {}",
                    publication.publish_destination.display()
                ));
                return Err(close_failed_staging(
                    staging_root,
                    &publication.publish_source_identity,
                    primary,
                ));
            }
            let post_permissions_identity = (|| -> Result<bool> {
                Ok(path::repository_directory_commit_matches_path(
                    &publication.publish_parent_identity,
                    publish_parent,
                )? && path::repository_directory_commit_matches_path(
                    &publication.publish_source_identity,
                    &publish_source,
                )? && verify_tracked_init_directories(&self.directory_identities).is_ok())
            })();
            if !matches!(post_permissions_identity, Ok(true)) {
                let primary = anyhow::anyhow!(
                    "Init publication boundary changed after final permissions were applied; refusing to publish {}{}",
                    publication.publish_destination.display(),
                    post_permissions_identity
                        .err()
                        .map(|error| format!(": {error:#}"))
                        .unwrap_or_default()
                );
                return Err(close_failed_staging(
                    staging_root,
                    &publication.publish_source_identity,
                    primary,
                ));
            }
            if let Err(primary) =
                path::rename_entry_noreplace(&publish_source, &publication.publish_destination)
            {
                let primary = anyhow::Error::new(primary).context(format!(
                    "Failed to publish initialized repository without replacing concurrent path {}",
                    publication.publish_destination.display()
                ));
                return Err(close_failed_staging(
                    staging_root,
                    &publication.publish_source_identity,
                    primary,
                ));
            }
            self.armed = false;
            // Disarm TempDir cleanup after the rename. A watcher could recreate
            // the now-missing random source name before Drop; `keep` guarantees
            // Jig never recursively removes that foreign replacement.
            let _published_source_name = staging_root.keep();
            return Ok(());
        }

        self.verify_destination_identity()?;
        let mut cleanup_failures = Vec::new();
        for (relative, mutation) in &self.files {
            if let Err(error) = self.verify_destination_identity() {
                cleanup_failures.push(format!(
                    "stopped retained-preimage cleanup before {}: {error:#}",
                    relative.display()
                ));
                break;
            }
            let Some(preimage) = mutation.original_quarantine.as_ref() else {
                continue;
            };
            let snapshot = match self.snapshot_absolute_path(preimage) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    cleanup_failures.push(format!(
                        "{}: retained preimage {} could not be inspected: {error:#}",
                        relative.display(),
                        preimage.display()
                    ));
                    continue;
                }
            };
            if !init_snapshots_match(&snapshot, &mutation.before)? {
                cleanup_failures.push(format!(
                    "{}: retained preimage changed; preserving recovery artifact {}",
                    relative.display(),
                    preimage.display()
                ));
                continue;
            }
            if let Err(error) = self.dispose_snapshot_leaf(relative, preimage, &snapshot) {
                cleanup_failures.push(format!(
                    "{}: failed to remove retained preimage {}: {error:#}",
                    relative.display(),
                    preimage.display()
                ));
            }
        }
        if let Err(error) = self.close_write_staging() {
            cleanup_failures.push(format!("private write staging cleanup failed: {error:#}"));
        }
        self.armed = false;
        if !cleanup_failures.is_empty() {
            bail!(
                "Initialized repository was committed, but cleanup was incomplete:\n{}",
                cleanup_failures.join("\n")
            );
        }
        Ok(())
    }

    fn plan_staged_render(
        &mut self,
        staged: &staged_render::StagedRender,
        reserved_output_paths: &[PathBuf],
    ) -> Result<()> {
        if self.is_privately_staged() {
            return Ok(());
        }
        let mut planned = staged
            .active_paths
            .iter()
            .chain(staged.retirement_paths.iter())
            .chain(reserved_output_paths)
            .cloned()
            .collect::<BTreeSet<_>>();
        if !reserved_output_paths.is_empty() {
            planned.insert(PathBuf::from(managed_paths::AGENT_MAP_PATH));
        }
        // A scaffold refreshes agent-map.md after crate guides exist, retaining
        // one additional Jig generation beyond its staged-render publication.
        let repeated_generation_count = usize::from(!reserved_output_paths.is_empty());
        self.ensure_planned_noreplace_filesystems(&planned)?;
        validate_retained_generation_budget(
            &planned,
            repeated_generation_count,
            process_soft_handle_limit(),
            current_open_handle_count(),
        )?;
        for relative in &planned {
            self.ensure_file_mutation(relative)?;
        }
        self.existing_generation_budget_sealed = true;
        Ok(())
    }

    fn ensure_planned_noreplace_filesystems(&self, planned: &BTreeSet<PathBuf>) -> Result<()> {
        let mut verified_filesystems = Vec::<PathBuf>::new();
        for relative in planned {
            validate_repository_relative_ancestors(&self.destination, relative)?;
            let output = self.destination.join(relative);
            let parent = output
                .parent()
                .with_context(|| format!("Init output has no parent: {}", output.display()))?;
            let (existing_ancestor, _) = path::split_existing_ancestor(parent)?;
            let mut already_verified = false;
            for verified in &verified_filesystems {
                if path::repository_paths_same_filesystem(verified, &existing_ancestor)? {
                    already_verified = true;
                    break;
                }
            }
            if already_verified {
                continue;
            }
            path::ensure_atomic_noreplace_publication_supported(&existing_ancestor).with_context(
                || {
                    format!(
                        "Init output {} is on a filesystem without safe transactional publication",
                        relative.display()
                    )
                },
            )?;
            verified_filesystems.push(existing_ancestor);
        }
        Ok(())
    }

    fn plan_scaffold_files(&mut self, files: &[scaffold::ScaffoldFile]) -> Result<()> {
        if self.is_privately_staged() {
            return Ok(());
        }
        for file in files {
            if self.existing_generation_budget_sealed
                && !self.files.contains_key(Path::new(&file.relative))
            {
                bail!(
                    "Scaffold output {} was not included in the up-front existing-destination generation budget",
                    file.relative
                );
            }
            self.ensure_file_mutation(Path::new(&file.relative))?;
        }
        Ok(())
    }

    fn plan_regular_file_bytes(&mut self, relative: &Path, _contents: &[u8]) -> Result<()> {
        if self.existing_generation_budget_sealed && !self.files.contains_key(relative) {
            bail!(
                "Generated output {} was not included in the up-front existing-destination generation budget",
                relative.display()
            );
        }
        self.ensure_file_mutation(relative)
    }

    fn ensure_file_mutation(&mut self, relative: &Path) -> Result<()> {
        if self.is_privately_staged() {
            return Ok(());
        }
        self.verify_destination_identity()?;
        if !self.files.contains_key(relative) {
            let before = self.snapshot_path(&self.destination, relative)?;
            self.files.insert(
                relative.to_path_buf(),
                InitFileMutation {
                    before,
                    expected_jig_states: Vec::new(),
                    original_quarantine: None,
                },
            );
        }
        Ok(())
    }

    fn ensure_parent_directories(&mut self, relative: &Path) -> Result<()> {
        self.verify_destination_identity()?;
        validate_repository_relative_ancestors(&self.destination, relative)?;
        let Some(parent) = relative.parent() else {
            return Ok(());
        };
        let mut current = self.destination.clone();
        for component in parent.components() {
            let std::path::Component::Normal(component) = component else {
                bail!(
                    "Init output path must contain only normal relative components: {}",
                    relative.display()
                );
            };
            current.push(component);
            self.verify_destination_identity()?;
            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                    let identity = path::repository_directory_commit_at(&current)?;
                    if let Some(expected) = self.directory_identities.get(&current) {
                        if expected.identity != identity.identity {
                            bail!(
                                "Init output ancestor changed concurrently: {}",
                                current.display()
                            );
                        }
                    } else {
                        self.directory_identities.insert(current.clone(), identity);
                    }
                    self.verify_destination_identity()?;
                }
                Ok(_) => bail!(
                    "Init output parent is not a real directory: {}",
                    current.display()
                ),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    match fs::create_dir(&current) {
                        Ok(()) => {
                            let identity = path::repository_directory_commit_at(&current)?;
                            self.directory_identities
                                .insert(current.clone(), identity.clone());
                            self.owned_directories
                                .insert(current.clone(), identity.clone());
                            if let Err(primary) = self.verify_destination_identity() {
                                self.directory_identities.remove(&current);
                                self.owned_directories.remove(&current);
                                let cleanup = self.cleanup_owned_directory_after_failed_boundary(
                                    &current, identity,
                                );
                                return match cleanup {
                                    Ok(()) => Err(primary),
                                    Err(cleanup) => Err(anyhow::anyhow!(
                                        "{primary:#}\nAdditionally failed to clean the just-created init directory safely: {cleanup:#}"
                                    )),
                                };
                            }
                        }
                        Err(error) => {
                            validate_existing_init_directory_after_create_error(
                                &current, error, true,
                            )?;
                            let identity = path::repository_directory_commit_at(&current)?;
                            if let Some(expected) = self.directory_identities.get(&current) {
                                if expected.identity != identity.identity {
                                    bail!(
                                        "Init output ancestor changed concurrently: {}",
                                        current.display()
                                    );
                                }
                            } else {
                                self.directory_identities.insert(current.clone(), identity);
                            }
                            self.verify_destination_identity()?;
                        }
                    }
                }
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("Failed to inspect init output parent {}", current.display())
                    });
                }
            }
        }
        validate_repository_relative_ancestors(&self.destination, relative)?;
        self.verify_destination_identity()
    }

    fn cleanup_owned_directory_after_failed_boundary(
        &self,
        directory: &Path,
        expected_directory: path::RepositoryDirectoryCommit,
    ) -> Result<()> {
        let relative = directory.strip_prefix(&self.destination).with_context(|| {
            format!(
                "Created init directory {} is outside destination {}",
                directory.display(),
                self.destination.display()
            )
        })?;
        let quarantine = self.unique_recovery_path(relative)?;
        match path::rename_entry_noreplace(directory, &quarantine) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "Failed to quarantine just-created directory {}",
                        directory.display()
                    )
                });
            }
        }
        let identity = path::repository_path_identity(&quarantine)?;
        let empty = fs::read_dir(&quarantine)
            .with_context(|| format!("Failed to inspect {}", quarantine.display()))?
            .next()
            .is_none();
        if identity == expected_directory.identity && empty {
            return self.dispose_empty_owned_directory(
                relative,
                &quarantine,
                directory,
                expected_directory,
            );
        }
        path::rename_entry_noreplace(&quarantine, directory).with_context(|| {
            format!(
                "Created directory changed concurrently; preserved it at {} but could not restore {}",
                quarantine.display(),
                directory.display()
            )
        })?;
        bail!(
            "Created init directory changed concurrently; preserved {}",
            directory.display()
        )
    }

    fn prepare_file_publication(&mut self, relative: &Path) -> Result<()> {
        if self.is_privately_staged() {
            self.ensure_parent_directories(relative)?;
            return self.verify_destination_identity();
        }
        self.ensure_file_mutation(relative)?;
        self.ensure_parent_directories(relative)?;
        self.verify_destination_identity()?;
        self.ensure_write_staging(relative)?;
        self.verify_destination_identity()?;

        let expected = {
            let mutation = self
                .files
                .get(relative)
                .expect("init mutation was preflighted");
            mutation
                .expected_jig_states
                .last()
                .unwrap_or(&mutation.before)
                .clone()
        };
        let current = self.snapshot_destination_path(relative)?;
        if matches!(expected, InitPathSnapshot::Missing) {
            if matches!(current, InitPathSnapshot::Missing) {
                return Ok(());
            }
            bail!(
                "Init output {} appeared concurrently; refusing to replace it",
                self.destination.join(relative).display()
            );
        }
        if matches!(current, InitPathSnapshot::Missing) {
            bail!(
                "Init output {} disappeared concurrently; refusing to publish",
                self.destination.join(relative).display()
            );
        }

        let quarantine = self.unique_recovery_path(relative)?;
        path::rename_entry_noreplace(&self.destination.join(relative), &quarantine).with_context(
            || {
                format!(
                    "Failed to quarantine current init output {} before replacement",
                    self.destination.join(relative).display()
                )
            },
        )?;
        let quarantined = self.snapshot_absolute_path(&quarantine)?;
        let root_check = self.verify_destination_identity();
        let matches_expected = init_snapshots_match(&quarantined, &expected)?;
        if root_check.is_err() || !matches_expected {
            let restore =
                path::rename_entry_noreplace(&quarantine, &self.destination.join(relative));
            if let Err(error) = restore {
                bail!(
                    "Init output changed at the publication boundary; preserved the quarantined entry at {} but could not restore its original path: {error}",
                    quarantine.display()
                );
            }
            root_check?;
            bail!(
                "Init output {} changed concurrently; preserved it and refused to replace it",
                self.destination.join(relative).display()
            );
        }

        let retain_as_preimage = self.files.get(relative).is_some_and(|mutation| {
            mutation.original_quarantine.is_none()
                && mutation.expected_jig_states.is_empty()
                && !matches!(mutation.before, InitPathSnapshot::Missing)
        });
        if retain_as_preimage {
            self.files
                .get_mut(relative)
                .expect("init mutation was preflighted")
                .original_quarantine = Some(quarantine);
        } else {
            self.dispose_snapshot_leaf(relative, &quarantine, &quarantined)?;
        }
        Ok(())
    }

    fn record_regular_commit(
        &mut self,
        relative: &Path,
        commit: path::RepositoryFileCommit,
    ) -> Result<()> {
        if self.is_privately_staged() {
            self.verify_destination_identity()?;
            let current = path::repository_file_fingerprint_at(&self.destination.join(relative))?;
            if !path::repository_file_commits_match(&current, &commit) {
                bail!(
                    "Private init output {} was replaced immediately after publication",
                    self.destination.join(relative).display()
                );
            }
            return Ok(());
        }
        self.verify_destination_identity()?;
        let current = path::repository_file_fingerprint_at(&self.destination.join(relative))?;
        if !path::repository_file_commits_match(&current, &commit) {
            bail!(
                "Init output {} was replaced immediately after publication",
                self.destination.join(relative).display()
            );
        }
        self.files
            .get_mut(relative)
            .with_context(|| format!("Init transaction did not preflight {}", relative.display()))?
            .expected_jig_states
            .push(InitPathSnapshot::Regular(commit));
        Ok(())
    }

    fn record_symlink_commit(
        &mut self,
        relative: &Path,
        commit: path::RepositorySymlinkCommit,
    ) -> Result<()> {
        if self.is_privately_staged() {
            self.verify_destination_identity()?;
            let current = self.snapshot_destination_path(relative)?;
            let committed = InitPathSnapshot::Symlink {
                identity: commit.identity,
                target: commit.target,
                target_is_directory: commit.target_is_directory,
                _handle: commit._handle,
            };
            if !init_snapshots_match(&current, &committed)? {
                bail!(
                    "Private init symlink output {} was replaced immediately after publication",
                    self.destination.join(relative).display()
                );
            }
            return Ok(());
        }
        self.verify_destination_identity()?;
        let current = self.snapshot_destination_path(relative)?;
        let committed = InitPathSnapshot::Symlink {
            identity: commit.identity,
            target: commit.target,
            target_is_directory: commit.target_is_directory,
            _handle: commit._handle,
        };
        if !init_snapshots_match(&current, &committed)? {
            bail!(
                "Init symlink output {} was replaced immediately after publication",
                self.destination.join(relative).display()
            );
        }
        self.files
            .get_mut(relative)
            .with_context(|| format!("Init transaction did not preflight {}", relative.display()))?
            .expected_jig_states
            .push(committed);
        Ok(())
    }

    fn record_missing_commit(&mut self, relative: &Path) -> Result<()> {
        if self.is_privately_staged() {
            self.verify_destination_identity()?;
            if !matches!(
                self.snapshot_destination_path(relative)?,
                InitPathSnapshot::Missing
            ) {
                bail!(
                    "Private init output {} reappeared immediately after removal",
                    self.destination.join(relative).display()
                );
            }
            return Ok(());
        }
        self.verify_destination_identity()?;
        if !matches!(
            self.snapshot_destination_path(relative)?,
            InitPathSnapshot::Missing
        ) {
            bail!(
                "Init output {} reappeared immediately after removal",
                self.destination.join(relative).display()
            );
        }
        self.files
            .get_mut(relative)
            .with_context(|| format!("Init transaction did not preflight {}", relative.display()))?
            .expected_jig_states
            .push(InitPathSnapshot::Missing);
        Ok(())
    }

    fn unique_recovery_path(&self, relative: &Path) -> Result<PathBuf> {
        let path = self.destination.join(relative);
        let parent = path
            .parent()
            .with_context(|| format!("Init output has no parent: {}", path.display()))?;
        let name = path
            .file_name()
            .with_context(|| format!("Init output has no file name: {}", path.display()))?
            .to_string_lossy();
        for _ in 0..1024 {
            let index = self.next_snapshot.get();
            self.next_snapshot.set(index.saturating_add(1));
            let candidate = parent.join(format!(
                ".{name}.jig-init-recovery-{}-{index}",
                std::process::id()
            ));
            match fs::symlink_metadata(&candidate) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(candidate),
                Ok(_) => continue,
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("Failed to inspect recovery path {}", candidate.display())
                    });
                }
            }
        }
        bail!(
            "Failed to allocate a unique init recovery path beside {}",
            path.display()
        )
    }

    fn dispose_snapshot_leaf(
        &self,
        relative: &Path,
        inspected_path: &Path,
        expected: &InitPathSnapshot,
    ) -> Result<()> {
        let disposal = self.unique_recovery_path(relative)?;
        path::rename_entry_noreplace(inspected_path, &disposal).with_context(|| {
            format!(
                "Failed to move inspected init quarantine {} into a second no-replace disposal quarantine {}",
                inspected_path.display(),
                disposal.display()
            )
        })?;

        let actual = match self.snapshot_absolute_path(&disposal) {
            Ok(actual) => actual,
            Err(error) => {
                bail!(
                    "Could not verify second disposal quarantine {}; preserving it instead of unlinking: {error:#}",
                    disposal.display()
                );
            }
        };
        if !init_snapshots_match(&actual, expected)? {
            return Err(restore_changed_disposal_quarantine(
                &disposal,
                inspected_path,
                anyhow::anyhow!(
                    "Inspected init quarantine changed before disposal; refusing to unlink replacement {}",
                    disposal.display()
                ),
            ));
        }

        remove_snapshot_leaf_unchecked(&disposal, &actual).with_context(|| {
            format!(
                "Failed to remove exact second disposal quarantine {}; it remains available for recovery",
                disposal.display()
            )
        })
    }

    fn dispose_empty_owned_directory(
        &self,
        relative: &Path,
        inspected_path: &Path,
        restore_path: &Path,
        expected_directory: path::RepositoryDirectoryCommit,
    ) -> Result<()> {
        let expected_identity = expected_directory.identity.clone();
        let disposal = self.unique_recovery_path(relative)?;
        path::rename_entry_noreplace(inspected_path, &disposal).with_context(|| {
            format!(
                "Failed to move inspected owned directory {} into a second no-replace disposal quarantine {}",
                inspected_path.display(),
                disposal.display()
            )
        })?;

        let exact_empty_directory = (|| -> Result<bool> {
            let metadata = fs::symlink_metadata(&disposal).with_context(|| {
                format!(
                    "Failed to inspect disposal quarantine {}",
                    disposal.display()
                )
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Ok(false);
            }
            let identity = path::repository_path_identity(&disposal)?;
            let empty = fs::read_dir(&disposal)
                .with_context(|| {
                    format!("Failed to read disposal quarantine {}", disposal.display())
                })?
                .next()
                .is_none();
            Ok(identity == expected_identity && empty)
        })();
        match exact_empty_directory {
            Ok(true) => {
                drop(expected_directory);
                fs::remove_dir(&disposal).with_context(|| {
                format!(
                    "Failed to remove exact empty directory disposal quarantine {}; it remains available for recovery",
                    disposal.display()
                )
                })
            }
            Ok(false) => Err(restore_changed_disposal_quarantine(
                &disposal,
                restore_path,
                anyhow::anyhow!(
                    "Owned init directory changed before disposal; refusing to remove replacement {}",
                    disposal.display()
                ),
            )),
            Err(error) => Err(anyhow::anyhow!(
                "Could not verify owned-directory disposal quarantine {}; preserving it instead of removing: {error:#}",
                disposal.display()
            )),
        }
    }

    fn rollback(&mut self) -> Result<()> {
        if !self.armed {
            return Ok(());
        }
        self.armed = false;
        self.rollback_armed()
    }

    fn rollback_armed(&mut self) -> Result<()> {
        let staged_boundary = self
            .staged_publication
            .as_ref()
            .map(|_| verify_tracked_init_directories(&self.directory_identities));
        if let Some(publication) = self.staged_publication.as_mut() {
            if let Some(staging_root) = publication.staging_root.take() {
                if let Some(Err(error)) = staged_boundary {
                    let preserved = staging_root.keep();
                    bail!(
                        "Private init work tree changed before cleanup: {error:#}. Preserving the complete staging tree at {}",
                        preserved.display()
                    );
                }
                return cleanup_private_staging(staging_root, &publication.publish_source_identity)
                    .with_context(|| {
                        format!(
                            "Failed to remove private failed-init staging tree beside {}",
                            self.final_destination.display()
                        )
                    });
            }
            return Ok(());
        }

        let mut failures = Vec::new();
        if let Err(error) = self.verify_rollback_root_and_preexisting_ancestors() {
            failures.push(format!(
                "{}; refusing to touch replacement root. Any retained preimages remain at their .jig-init-recovery paths",
                error
            ));
            if let Err(cleanup) = self.close_write_staging() {
                failures.push(format!("private write staging cleanup failed: {cleanup:#}"));
            }
            return Err(anyhow::anyhow!(failures.join("\n")));
        }

        let mutations = self
            .files
            .iter()
            .rev()
            .map(|(relative, mutation)| (relative.clone(), mutation.clone()))
            .collect::<Vec<_>>();
        for (relative, mutation) in mutations {
            match self.changed_owned_ancestor(&relative) {
                Ok(Some(directory)) => {
                    failures.push(format!(
                        "{}: owned ancestor {} changed; preserving its subtree for directory-level recovery",
                        relative.display(),
                        directory.display()
                    ));
                    continue;
                }
                Ok(None) => {}
                Err(error) => {
                    failures.push(format!("{}: {error:#}", relative.display()));
                    continue;
                }
            }
            let current = match self.snapshot_destination_path(&relative) {
                Ok(current) => current,
                Err(error) => {
                    failures.push(format!("{}: {error:#}", relative.display()));
                    continue;
                }
            };
            let matches_before = match init_snapshots_match(&current, &mutation.before) {
                Ok(matches) => matches,
                Err(error) => {
                    failures.push(format!("{}: {error:#}", relative.display()));
                    continue;
                }
            };
            if matches_before && mutation.original_quarantine.is_none() {
                continue;
            }

            let current_quarantine = if matches!(current, InitPathSnapshot::Missing) {
                None
            } else {
                let quarantine = match self.unique_recovery_path(&relative) {
                    Ok(path) => path,
                    Err(error) => {
                        failures.push(format!("{}: {error:#}", relative.display()));
                        continue;
                    }
                };
                if let Err(error) =
                    path::rename_entry_noreplace(&self.destination.join(&relative), &quarantine)
                {
                    failures.push(format!(
                        "{}: failed to quarantine current rollback leaf: {error}",
                        relative.display()
                    ));
                    continue;
                }
                let quarantined = match self.snapshot_absolute_path(&quarantine) {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        failures.push(format!(
                            "{}: could not inspect rollback quarantine {}: {error:#}",
                            relative.display(),
                            quarantine.display()
                        ));
                        continue;
                    }
                };
                let is_jig = any_init_snapshot_matches(&quarantined, &mutation.expected_jig_states)
                    .unwrap_or(false);
                if !is_jig {
                    let retained_preimage = mutation
                        .original_quarantine
                        .as_ref()
                        .map(|path| format!("; original preimage remains at {}", path.display()))
                        .unwrap_or_default();
                    match path::rename_entry_noreplace(
                        &quarantine,
                        &self.destination.join(&relative),
                    ) {
                        Ok(()) => failures.push(format!(
                            "{} changed after Jig wrote it; preserved the current path{}",
                            relative.display(),
                            retained_preimage
                        )),
                        Err(error) => failures.push(format!(
                            "{} changed after Jig wrote it; preserved it at recovery path {} because its original path became occupied: {error}{}",
                            relative.display(),
                            quarantine.display(),
                            retained_preimage
                        )),
                    }
                    continue;
                }
                Some((quarantine, quarantined))
            };

            let restore_result = match (&mutation.before, &mutation.original_quarantine) {
                (InitPathSnapshot::Missing, _) => Ok(()),
                (_, Some(preimage)) => path::rename_entry_noreplace(
                    preimage,
                    &self.destination.join(&relative),
                )
                .with_context(|| {
                    format!(
                        "Failed to restore retained preimage from {}; it remains available for recovery",
                        preimage.display()
                    )
                }),
                (_, None) if matches_before => Ok(()),
                (_, None) => Err(anyhow::anyhow!(
                    "Original preimage for {} is unavailable; preserving recovery artifacts",
                    relative.display()
                )),
            };
            if let Err(error) = restore_result {
                failures.push(format!("{}: {error:#}", relative.display()));
                continue;
            }
            if let Some((quarantine, snapshot)) = current_quarantine {
                if let Err(error) = self.dispose_snapshot_leaf(&relative, &quarantine, &snapshot) {
                    failures.push(format!(
                        "{}: restored the preimage but failed to remove quarantined Jig output {}: {error:#}",
                        relative.display(),
                        quarantine.display()
                    ));
                }
            }
        }

        if let Err(error) = self.close_write_staging() {
            failures.push(format!("private write staging cleanup failed: {error:#}"));
        }

        let mut directories = self.owned_directories.keys().cloned().collect::<Vec<_>>();
        directories.sort_by(|left, right| {
            right
                .components()
                .count()
                .cmp(&left.components().count())
                .then_with(|| right.cmp(left))
        });
        for directory in directories {
            let expected_directory = self
                .owned_directories
                .remove(&directory)
                .expect("owned directory disappeared from transaction state");
            self.directory_identities.remove(&directory);
            match fs::symlink_metadata(&directory) {
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => {
                    failures.push(format!(
                        "{}: failed to inspect owned init directory: {error}",
                        directory.display()
                    ));
                    continue;
                }
            }
            let relative = match directory.strip_prefix(&self.destination) {
                Ok(relative) => relative,
                Err(error) => {
                    failures.push(format!("{}: {error}", directory.display()));
                    continue;
                }
            };
            let quarantine = match self.unique_recovery_path(relative) {
                Ok(path) => path,
                Err(error) => {
                    failures.push(format!("{}: {error:#}", directory.display()));
                    continue;
                }
            };
            if let Err(error) = path::rename_entry_noreplace(&directory, &quarantine) {
                failures.push(format!(
                    "{}: preserving changed init directory ({error})",
                    directory.display()
                ));
                continue;
            }
            let identity = path::repository_path_identity(&quarantine);
            let empty = fs::read_dir(&quarantine)
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(false);
            if identity
                .as_ref()
                .is_ok_and(|identity| identity == &expected_directory.identity)
                && empty
            {
                if let Err(error) = self.dispose_empty_owned_directory(
                    relative,
                    &quarantine,
                    &directory,
                    expected_directory,
                ) {
                    failures.push(format!(
                        "{}: failed to dispose exact empty owned directory quarantine {} safely ({error:#})",
                        directory.display(),
                        quarantine.display()
                    ));
                }
            } else {
                match path::rename_entry_noreplace(&quarantine, &directory) {
                    Ok(()) => failures.push(format!(
                        "{}: preserving non-empty or changed init directory",
                        directory.display()
                    )),
                    Err(error) => failures.push(format!(
                        "{}: preserving non-empty or changed init directory at recovery path {} ({error})",
                        directory.display(),
                        quarantine.display()
                    )),
                }
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            bail!("{}", failures.join("\n"))
        }
    }

    fn snapshot_path(&self, root: &Path, relative: &Path) -> Result<InitPathSnapshot> {
        use path::RepositoryFileLeaf;

        match path::validate_repository_relative_file_leaf(root, relative)? {
            RepositoryFileLeaf::Missing => Ok(InitPathSnapshot::Missing),
            RepositoryFileLeaf::RegularFile => Ok(InitPathSnapshot::Regular(
                path::repository_file_fingerprint_at(&root.join(relative))?,
            )),
            RepositoryFileLeaf::Symlink => {
                let commit = path::repository_symlink_commit_at(&root.join(relative))?;
                Ok(InitPathSnapshot::Symlink {
                    identity: commit.identity,
                    target: commit.target,
                    target_is_directory: commit.target_is_directory,
                    _handle: commit._handle,
                })
            }
        }
    }

    fn snapshot_destination_path(&self, relative: &Path) -> Result<InitPathSnapshot> {
        self.snapshot_path(&self.destination, relative)
    }

    fn snapshot_absolute_path(&self, absolute: &Path) -> Result<InitPathSnapshot> {
        let metadata = fs::symlink_metadata(absolute)
            .with_context(|| format!("Failed to inspect {}", absolute.display()))?;
        if metadata.file_type().is_symlink() {
            let commit = path::repository_symlink_commit_at(absolute)?;
            return Ok(InitPathSnapshot::Symlink {
                identity: commit.identity,
                target: commit.target,
                target_is_directory: commit.target_is_directory,
                _handle: commit._handle,
            });
        }
        if metadata.is_file() {
            return Ok(InitPathSnapshot::Regular(
                path::repository_file_fingerprint_at(absolute)?,
            ));
        }
        bail!(
            "Rollback leaf is not a file or symlink: {}",
            absolute.display()
        )
    }

    fn finish_failed_init(&mut self, primary: anyhow::Error) -> anyhow::Error {
        match self.rollback() {
            Ok(()) => primary,
            Err(rollback) => anyhow::anyhow!(
                "{primary:#}\nAdditionally, failed to roll back init changes:\n{rollback:#}"
            ),
        }
    }
}

fn close_failed_staging(
    staging_root: TempDir,
    expected_identity: &path::RepositoryDirectoryCommit,
    primary: anyhow::Error,
) -> anyhow::Error {
    match cleanup_private_staging(staging_root, expected_identity) {
        Ok(()) => primary,
        Err(cleanup) => anyhow::anyhow!(
            "{primary:#}\nAdditionally, staging cleanup was incomplete:\n{cleanup:#}"
        ),
    }
}

fn cleanup_private_staging(
    staging_root: TempDir,
    expected_identity: &path::RepositoryDirectoryCommit,
) -> Result<()> {
    let staging_path = staging_root.path().to_path_buf();
    match path::repository_directory_commit_matches_path(expected_identity, &staging_path) {
        Ok(false) => {
            let preserved = staging_root.keep();
            bail!(
                "Private staging path was replaced concurrently; preserving foreign replacement {}",
                preserved.display()
            );
        }
        Err(error)
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|error| error.kind() == io::ErrorKind::NotFound) =>
        {
            return Ok(());
        }
        Err(error) => {
            let preserved = staging_root.keep();
            bail!(
                "Could not verify private staging path {} for cleanup ({error:#}); preserving it",
                preserved.display()
            );
        }
        Ok(true) => {}
    }

    let mut cleanup_failures = Vec::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        if let Err(error) = fs::set_permissions(&staging_path, fs::Permissions::from_mode(0o700)) {
            cleanup_failures.push(format!(
                "failed to restore private staging permissions on {}: {error}",
                staging_path.display()
            ));
        }
    }
    if let Err(error) = staging_root.close() {
        cleanup_failures.push(format!(
            "failed to remove private staging tree {}: {error}",
            staging_path.display()
        ));
    }
    if cleanup_failures.is_empty() {
        Ok(())
    } else {
        bail!("{}", cleanup_failures.join("\n"))
    }
}

fn any_init_snapshot_matches(
    state: &InitPathSnapshot,
    candidates: &[InitPathSnapshot],
) -> Result<bool> {
    for candidate in candidates {
        if init_snapshots_match(state, candidate)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn init_snapshots_match(left: &InitPathSnapshot, right: &InitPathSnapshot) -> Result<bool> {
    Ok(match (left, right) {
        (InitPathSnapshot::Missing, InitPathSnapshot::Missing) => true,
        (InitPathSnapshot::Regular(left), InitPathSnapshot::Regular(right)) => {
            path::repository_file_commits_match(left, right)
        }
        (
            InitPathSnapshot::Symlink {
                identity: left_identity,
                target: left_target,
                target_is_directory: left_is_directory,
                ..
            },
            InitPathSnapshot::Symlink {
                identity: right_identity,
                target: right_target,
                target_is_directory: right_is_directory,
                ..
            },
        ) => {
            left_identity == right_identity
                && left_target == right_target
                && left_is_directory == right_is_directory
        }
        _ => false,
    })
}

fn restore_changed_disposal_quarantine(
    disposal: &Path,
    inspected_path: &Path,
    primary: anyhow::Error,
) -> anyhow::Error {
    match path::rename_entry_noreplace(disposal, inspected_path) {
        Ok(()) => anyhow::anyhow!(
            "{primary:#}\nRestored the changed entry to {}",
            inspected_path.display()
        ),
        Err(error) => anyhow::anyhow!(
            "{primary:#}\nPreserved the changed entry at {} because {} became occupied: {error}",
            disposal.display(),
            inspected_path.display()
        ),
    }
}

fn remove_snapshot_leaf_unchecked(path: &Path, snapshot: &InitPathSnapshot) -> Result<()> {
    match snapshot {
        InitPathSnapshot::Missing => Ok(()),
        InitPathSnapshot::Regular(_) | InitPathSnapshot::Symlink { .. } => fs::remove_file(path)
            .with_context(|| format!("Failed to remove quarantined init leaf {}", path.display())),
    }
}

fn validate_existing_init_directory_after_create_error(
    directory: &Path,
    create_error: io::Error,
    require_real_directory: bool,
) -> Result<()> {
    match fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(()),
        Ok(metadata) if metadata.file_type().is_symlink() && !require_real_directory => {
            match fs::metadata(directory) {
                Ok(target_metadata) if target_metadata.is_dir() => Ok(()),
                Ok(_) => bail!(
                    "Init destination path component does not resolve to a directory: {}",
                    directory.display()
                ),
                Err(inspect_error) => Err(inspect_error).with_context(|| {
                    format!(
                        "Failed to inspect init destination path component {}",
                        directory.display()
                    )
                }),
            }
        }
        Ok(_) => bail!(
            "Init destination path component is not a real directory: {}",
            directory.display()
        ),
        Err(inspect_error) if create_error.kind() == io::ErrorKind::AlreadyExists => {
            Err(inspect_error).with_context(|| {
                format!(
                    "Failed to inspect existing init destination directory {}",
                    directory.display()
                )
            })
        }
        Err(_) => Err(create_error).with_context(|| {
            format!(
                "Failed to create init destination directory {}",
                directory.display()
            )
        }),
    }
}

impl Drop for InitMutationTransaction {
    fn drop(&mut self) {
        if self.armed {
            self.armed = false;
            let _ = self.rollback_armed();
        }
    }
}

pub fn run_init(mut opts: InitOpts) -> Result<Value> {
    let invocation_cwd = bootstrap_invocation_cwd()?;
    let destination = path::resolve_init_destination(&opts.path, &invocation_cwd)?;
    // This first validation deliberately precedes answer loading and template
    // resolution so unsafe or non-empty destinations fail without interaction.
    validate_init_destination(&destination, opts.force)?;
    ensure_init_destination_noreplace_supported(&destination)?;
    let progress = CliProgress::new("init");
    progress.header_for_path("render harness into new repo", &destination);
    progress.step("validate destination", "empty directory or --force");
    progress.log_blocked_on_err(validate_init_destination(&destination, opts.force))?;
    progress.step("read init answers", "--answers-file and CLI precedence");
    let answer_input =
        progress.log_blocked_on_err(AnswerInput::from_opts_at(&opts.answers, &invocation_cwd))?;
    let mut answers = progress.log_blocked_on_err(answer_input.effective_opts(&opts.answers))?;
    opts.scaffold.normalize_minimal_harness_shape(&answers);
    progress.log_blocked_on_err(opts.scaffold.validate_init_invariants(&answers))?;
    opts.scaffold.apply_init_answer_defaults(&mut answers);
    let scaffold_plan = progress.log_blocked_on_err(scaffold::InitScaffoldPlan::from_opts(
        &opts.scaffold,
        &answers,
        &destination,
    ))?;
    if let Some(plan) = &scaffold_plan {
        plan.apply_answer_defaults(&mut answers);
    }
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
    let mut transaction = InitMutationTransaction::create(&destination)?;
    let work_destination = transaction.work_destination().to_path_buf();
    let init_result = (|| -> Result<Value> {
        // Revalidate after creation: another process may have populated a path
        // between the initial preflight and our atomic create_dir calls.
        progress.log_blocked_on_err(validate_init_destination(&destination, opts.force))?;
        if let Some(plan) = &scaffold_plan {
            progress.step("preflight project scaffold", plan.summary());
            progress.log_blocked_on_err(plan.preflight(&work_destination, opts.force))?;
            progress.log_blocked_on_err(path::validate_repository_regular_file_leaf(
                &work_destination,
                Path::new(managed_paths::AGENT_MAP_PATH),
            ))?;
        }

        let copy_result = render_and_copy_bootstrap_template(BootstrapCopyRequest {
            destination: &work_destination,
            template: &template,
            answers: &answers,
            answer_input: Some(answer_input),
            use_defaults: opts.defaults,
            force: opts.force,
            dry_run: false,
            backup_root: None,
            seed_repo_path: None,
            prior_harness_footprint: None,
            prior_managed_paths: None,
            reconcile_runtime_config: false,
            allow_answers_overwrite: false,
            allow_contract_overwrite: false,
            reserved_output_paths: scaffold_plan
                .as_ref()
                .map(scaffold::InitScaffoldPlan::output_paths)
                .unwrap_or_default(),
            init_transaction: Some(&mut transaction),
            progress,
        })?;
        let scaffold_report = if let Some(plan) = &scaffold_plan {
            progress.step("scaffold project", plan.summary());
            if let Some(note) = plan.sanitized_repo_name_note() {
                progress.info("scaffold note", note);
            }
            let files = progress.log_blocked_on_err(plan.render_files())?;
            progress.log_blocked_on_err(transaction.plan_scaffold_files(&files))?;
            let report = progress.log_blocked_on_err(plan.write_rendered_with_transaction(
                &work_destination,
                files,
                opts.force,
                Some(&mut transaction),
            ))?;
            progress.step("refresh agent map", "include scaffold crate guides");
            let agent_map_path = Path::new(managed_paths::AGENT_MAP_PATH);
            let agent_map = progress.log_blocked_on_err(crate::policy::render_agent_map(
                &work_destination,
                agent_map_path,
            ))?;
            progress.log_blocked_on_err(
                transaction.plan_regular_file_bytes(agent_map_path, &agent_map),
            )?;
            progress.log_blocked_on_err(transaction.prepare_file_publication(agent_map_path))?;
            let agent_map_commit = if transaction.is_privately_staged() {
                let expected_leaf = progress.log_blocked_on_err(
                    path::validate_repository_regular_file_leaf(&work_destination, agent_map_path),
                )?;
                progress.log_blocked_on_err(path::write_repository_file_atomic_staged(
                    &work_destination,
                    agent_map_path,
                    &agent_map,
                    expected_leaf,
                    || transaction.verify_destination_identity(),
                ))?
            } else {
                let desired_permissions = transaction.publication_permissions(agent_map_path)?;
                let temporary_directory = transaction
                    .write_staging_path(agent_map_path)
                    .context("Existing-destination init write staging is unavailable")?
                    .to_path_buf();
                progress.log_blocked_on_err(path::write_repository_file_atomic_guarded(
                    &work_destination,
                    agent_map_path,
                    &agent_map,
                    desired_permissions,
                    &temporary_directory,
                    || transaction.verify_destination_identity(),
                ))?
            };
            progress.log_blocked_on_err(
                transaction.record_regular_commit(agent_map_path, agent_map_commit),
            )?;
            Some(report)
        } else {
            None
        };
        let default_branch = copy_result
            .default_branch
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing default_branch in staged {}", ANSWERS_FILE))?;
        progress.step("initialize git", format!("default branch {default_branch}"));
        let git_initialized =
            init_git_repo_with_validation(&work_destination, default_branch, || {
                transaction.verify_destination_identity()
            })?;

        Ok(json!({
            "ok": true,
            "command": "init",
            "render_mode": "copy",
            "template": template.source(),
            "destination": destination.display().to_string(),
            "answers_file": ANSWERS_FILE,
            "git_initialized": git_initialized,
            "scaffold": scaffold_report,
            "render_report": initial_render_report(&copy_result),
            "next_steps": initial_next_steps(
                InitialCommand::Init,
                &destination,
                &copy_result,
                scaffold_plan
                    .as_ref()
                    .is_some_and(scaffold::InitScaffoldPlan::database_enabled),
            ),
            "notes": initial_notes(
                copy_result.notes,
                copy_result.frontend_apps_configured,
                scaffold_plan.as_ref(),
                false,
            ),
        }))
    })();

    match init_result {
        Ok(report) => {
            transaction.commit()?;
            progress.done("init complete");
            Ok(report)
        }
        Err(primary) => Err(transaction.finish_failed_init(primary)),
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

#[derive(Debug)]
struct InitialTemplateRequest<'a> {
    template: &'a str,
    vcs_ref: Option<Cow<'a, str>>,
    used_default: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BuildTemplatePinPolicy {
    Released,
    Unreleased,
    Unknown,
}

#[cfg(test)]
thread_local! {
    static TEST_BUILD_TEMPLATE_PIN_POLICY: Cell<Option<BuildTemplatePinPolicy>> = const { Cell::new(None) };
}

fn resolve_initial_template_request<'a>(
    template: Option<&'a str>,
    vcs_ref: &'a Option<String>,
) -> Result<InitialTemplateRequest<'a>> {
    resolve_initial_template_request_with_policy(
        template,
        vcs_ref,
        current_build_template_pin_policy(),
    )
}

fn resolve_initial_template_request_with_policy<'a>(
    template: Option<&'a str>,
    vcs_ref: &'a Option<String>,
    pin_policy: BuildTemplatePinPolicy,
) -> Result<InitialTemplateRequest<'a>> {
    match template {
        Some(template) if is_official_template_source(template) => {
            official_initial_template_request(vcs_ref, pin_policy)
        }
        Some(template) => Ok(InitialTemplateRequest {
            template,
            vcs_ref: vcs_ref.as_deref().map(Cow::Borrowed),
            used_default: false,
        }),
        None => default_initial_template_request(vcs_ref, pin_policy),
    }
}

fn default_initial_template_request(
    vcs_ref: &Option<String>,
    pin_policy: BuildTemplatePinPolicy,
) -> Result<InitialTemplateRequest<'_>> {
    if vcs_ref.is_none() && pin_policy == BuildTemplatePinPolicy::Unreleased {
        // Omitted template on local builds is offline-friendly; explicitly naming
        // the official URL still means "use remote official template code".
        return Ok(InitialTemplateRequest {
            template: EMBEDDED_TEMPLATE_SOURCE,
            vcs_ref: None,
            used_default: true,
        });
    }

    official_initial_template_request(vcs_ref, pin_policy)
}

fn official_initial_template_request(
    vcs_ref: &Option<String>,
    pin_policy: BuildTemplatePinPolicy,
) -> Result<InitialTemplateRequest<'_>> {
    if vcs_ref.is_none() && pin_policy == BuildTemplatePinPolicy::Unreleased {
        bail!(
            "This jig binary was built from unreleased or dirty local source version {}.\nThe default official template pin {} may not match this binary.\nTo render from your checkout, pass --template /path/to/jig-sh --template-mode committed.\nTo use official remote template code, pass --vcs-ref <ref>.",
            env!("CARGO_PKG_VERSION"),
            official_template_ref(),
        );
    }

    Ok(InitialTemplateRequest {
        template: OFFICIAL_TEMPLATE_SOURCE,
        // The release workflow tags the whole workspace as vVERSION. Keep the
        // default template pinned to the installed jig binary's workspace version.
        vcs_ref: Some(
            vcs_ref
                .as_deref()
                .map(Cow::Borrowed)
                .unwrap_or_else(|| Cow::Owned(official_template_ref())),
        ),
        used_default: true,
    })
}

fn current_build_template_pin_policy() -> BuildTemplatePinPolicy {
    #[cfg(test)]
    {
        TEST_BUILD_TEMPLATE_PIN_POLICY
            .with(Cell::get)
            .unwrap_or(BuildTemplatePinPolicy::Released)
    }

    #[cfg(not(test))]
    {
        build_template_pin_policy_from_env(option_env!("JIG_BUILD_OFFICIAL_TEMPLATE_PIN"))
    }
}

fn build_template_pin_policy_from_env(value: Option<&str>) -> BuildTemplatePinPolicy {
    match value {
        Some(BUILD_TEMPLATE_PIN_RELEASED) => BuildTemplatePinPolicy::Released,
        Some(BUILD_TEMPLATE_PIN_UNRELEASED) => BuildTemplatePinPolicy::Unreleased,
        // Published crates do not carry .git metadata, so build.rs emits
        // unknown. Missing or unrecognized values keep the same release-pin
        // behavior rather than failing crates.io and packaged installs.
        _ => BuildTemplatePinPolicy::Unknown,
    }
}

fn is_official_template_source(template: &str) -> bool {
    canonical_template_source(template) == canonical_template_source(OFFICIAL_TEMPLATE_SOURCE)
}

fn canonical_template_source(template: &str) -> &str {
    template.strip_suffix(".git").unwrap_or(template)
}

fn official_template_ref() -> String {
    // The published binary and the template tag share the workspace version.
    official_template_ref_for_version(env!("CARGO_PKG_VERSION"))
}

fn official_template_ref_for_version(version: &str) -> String {
    format!("v{version}")
}

fn prepare_initial_template_source(
    request: &InitialTemplateRequest<'_>,
    template_mode: Option<TemplateMode>,
    path_base: &Path,
) -> Result<template_source::PreparedTemplateSource> {
    if request.used_default && template_mode.is_some() {
        // Keep local-only mode errors direct; wrapping them as default-source
        // resolution failures would incorrectly suggest a network or tag issue.
        bail!(REMOTE_TEMPLATE_MODE_ERROR);
    }

    let result = template_source::prepare_template_source_from_base(
        request.template,
        template_mode,
        request.vcs_ref.as_deref(),
        path_base,
    );
    if request.used_default {
        result.with_context(|| default_template_failure_context(request))
    } else {
        result
    }
}

fn default_template_failure_context(request: &InitialTemplateRequest<'_>) -> String {
    let Some(vcs_ref) = request.vcs_ref.as_deref() else {
        return format!(
            "Failed to resolve the official Jig template {}. For offline use, pass --template <local-path>. To use a specific official ref such as main, pass --vcs-ref <ref>.",
            request.template
        );
    };
    let ref_requirement = if vcs_ref == official_template_ref() {
        "network access and a matching release tag. If this Jig binary was built from a prerelease or development version, that tag may not exist yet"
    } else {
        "network access and the selected ref must exist"
    };
    format!(
        "Failed to resolve the official Jig template {} at {}. The official template requires {}. For offline use, pass --template <local-path>. To use a different official ref such as main, pass --vcs-ref <ref>.",
        request.template, vcs_ref, ref_requirement
    )
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
    if result.bootstrap_command_configured {
        steps.push("scripts/jig bootstrap".into());
    }
    steps.push("scripts/jig doctor".into());
    if result.codex_skills_configured {
        steps.push("scripts/jig agent bootstrap".into());
    }
    steps.push("scripts/jig check contract".into());
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
        steps.push("Provide scripts/dump-schema.sh, then run scripts/jig schema-dump.".into());
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
    fn custom_scaffold_label(self) -> &'static str {
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
            let backend_prefix = jig_core::dev_app_env_prefix(RUST_REACT_BACKEND_DEV_APP_NAME);
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
                if jig_core::dev_app_env_prefix(frontend_name) == backend_prefix {
                    bail!(
                        "Rust React frontend app name '{}' conflicts with the reserved backend dev app '{}' because both derive dev environment prefix {}; choose another frontend name",
                        frontend_name,
                        RUST_REACT_BACKEND_DEV_APP_NAME,
                        backend_prefix
                    );
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

fn default_true() -> bool {
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

fn external_program(env_key: &str, fallback: &str) -> String {
    env::var(env_key).unwrap_or_else(|_| fallback.to_string())
}

#[cfg(test)]
mod tests;
