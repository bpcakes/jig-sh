use std::path::PathBuf;

use clap::{Args, ValueEnum};
use jig_context::{ExecutionConfig, RustMigrationLayout};
use serde::{Deserialize, Serialize};

use super::adopt_infer::ComponentSelectionOpts;
use super::apps::{DevApp, FrontendApp, parse_frontend_app};
use super::scaffold_opts::ScaffoldOpts;

#[derive(Clone, Debug, Default)]
pub struct DevSettingsAnswers {
    pub proxy_port: Option<u16>,
    pub https_port: Option<u16>,
    pub https: Option<bool>,
    pub http2: Option<bool>,
    pub lan: Option<bool>,
    pub tld: Option<String>,
    pub workspace_discovery: Option<bool>,
}

#[derive(Args, Clone, Debug, Default)]
pub struct AnswerOpts {
    #[arg(skip)]
    pub adoption_components: Option<super::adopt_infer::ComponentCandidates>,
    #[arg(
        long,
        help_heading = "Automation",
        help = "Read renderer answers from a TOML file"
    )]
    pub answers_file: Option<PathBuf>,
    #[arg(
        long,
        help_heading = "Common Answers",
        help = "Repository display name written into generated docs"
    )]
    pub repo_name: Option<String>,
    #[arg(
        long,
        help_heading = "Project Shape",
        help = "Go import module for --preset go-react, e.g. github.com/acme/my-app"
    )]
    pub go_module: Option<String>,
    #[arg(
        long,
        help_heading = "Common Answers",
        help = "Default branch used for generated CI and comparison commands"
    )]
    pub default_branch: Option<String>,
    #[arg(
        long,
        help_heading = "Common Answers",
        help = "GitHub Actions runs-on value for generated workflows"
    )]
    pub ci_github_runner: Option<String>,
    #[arg(
        long,
        hide = true,
        help = "Legacy render input retained for committed pre-v4 templates; current v4 renders use contract compatibility and do not persist this value"
    )]
    pub jig_version: Option<String>,
    #[arg(
        long,
        help_heading = "Advanced Template Source",
        help = "Portable canonical template source URL for future updates"
    )]
    pub template_source_url: Option<String>,
    /// Set by `jig adopt --minimal`; not a public answer flag.
    #[arg(skip)]
    pub harness_footprint: Option<super::answers::HarnessFootprint>,
    /// Set by an application scaffold; persisted so updates retain backend-specific policy.
    #[arg(skip)]
    pub backend_language: Option<jig_context::backend::BackendLanguage>,
    /// Initial-render-only selection for authoring ordinary repository records.
    #[arg(skip)]
    pub repository_projection_hint: super::repository_model::RepositoryProjectionHint,
    /// Set by the Go scaffold to `none` or `postgres`.
    #[arg(skip)]
    pub go_database: Option<jig_context::backend::GoDatabase>,
    /// Derived from preserved repository authority for scaffold command/path rendering.
    #[arg(skip)]
    pub scaffold_go_component_roots: Vec<String>,
    /// Backend-neutral migration policy path loaded from persisted answers.
    #[arg(skip)]
    pub migration_dir: Option<String>,
    #[arg(
        long,
        help_heading = "Common Answers",
        help = "Generate SQLx and migration contract tools"
    )]
    pub sqlx_enabled: Option<bool>,
    #[arg(
        long = "rust-crate-root",
        help_heading = "Common Answers",
        help = "Directory whose direct children are Rust crates; may be repeated"
    )]
    pub rust_crate_roots: Vec<String>,
    #[arg(
        long,
        help_heading = "Common Answers",
        help = "SQL migration directory for SQLx-enabled repos"
    )]
    pub rust_migration_dir: Option<String>,
    #[arg(
        long,
        help_heading = "Common Answers",
        value_enum,
        help = "SQL migration layout: flat_migrations or versioned_artifacts"
    )]
    pub rust_migration_layout: Option<RustMigrationLayout>,
    #[arg(
        long,
        help_heading = "Common Answers",
        help = "Committed SQLx metadata directory"
    )]
    pub rust_sqlx_metadata_dir: Option<String>,
    #[arg(
        long,
        help_heading = "Common Answers",
        help = "Generate schema dump and freshness commands"
    )]
    pub schema_dump_enabled: Option<bool>,
    #[arg(
        long,
        help_heading = "Advanced Command Overrides",
        help = "Command used by scripts/jig sqlx schema dump"
    )]
    pub schema_dump_command: Option<String>,
    #[arg(
        long,
        help_heading = "Common Answers",
        help = "Committed repository-relative schema documentation directory"
    )]
    pub schema_docs_dir: Option<String>,
    #[arg(
        long,
        help_heading = "Advanced Command Overrides",
        help = "Command used by legacy schema-check manifests"
    )]
    pub schema_check_command: Option<String>,
    #[arg(
        long,
        help_heading = "Advanced Command Overrides",
        help = "Command used by scripts/jig check sqlx"
    )]
    pub sqlx_check_command: Option<String>,
    #[arg(
        long,
        help_heading = "Advanced Command Overrides",
        help = "Command used by legacy migration-add manifests"
    )]
    pub migration_add_command: Option<String>,
    #[arg(
        long,
        help_heading = "Advanced Command Overrides",
        help = "Command used by scripts/jig bootstrap"
    )]
    pub bootstrap_command: Option<String>,
    #[arg(
        long,
        help_heading = "Advanced Command Overrides",
        help = "Command used by legacy contract-check manifests"
    )]
    pub contract_check_command: Option<String>,
    #[arg(
        long,
        help_heading = "Advanced Command Overrides",
        help = "Deprecated; configure [dev] and [[dev.apps]] instead"
    )]
    pub dev_command: Option<String>,
    #[arg(
        long,
        help_heading = "Advanced Command Overrides",
        help = "Command used by scripts/jig check fmt"
    )]
    pub rust_fmt_check_command: Option<String>,
    #[arg(
        long,
        help_heading = "Advanced Command Overrides",
        help = "Command used by scripts/jig check clippy"
    )]
    pub rust_clippy_command: Option<String>,
    #[arg(
        long,
        help_heading = "Advanced Command Overrides",
        help = "Command used by scripts/jig check test"
    )]
    pub rust_test_command: Option<String>,
    #[arg(
        long,
        help_heading = "Advanced Command Overrides",
        help = "Command used by scripts/jig check test-locked"
    )]
    pub rust_test_locked_command: Option<String>,
    #[arg(
        long,
        help_heading = "Common Answers",
        value_parser = ["bun", "npm", "pnpm", "yarn"],
        help = "Web package manager for generated web app checks"
    )]
    pub web_package_manager: Option<String>,
    #[arg(
        long,
        help_heading = "Common Answers",
        help = "Explicitly enable or disable required application-contract and public-artifact gates backed by the repository-owned scripts/contracts.mjs v1 interface"
    )]
    pub application_contracts_enabled: Option<bool>,
    #[arg(
        long = "frontend-app",
        help_heading = "Common Answers",
        value_parser = parse_frontend_app,
        help = "Existing frontend app to wire into CI and dev checks",
        long_help = "Frontend CI app as name:dir:coverage_threshold[:kind[:role]]. Kind defaults to vite; an omitted role defaults to astro for env-port, admin for the historical admin/admin-panel names, and spa otherwise. Roles accept spa, admin, or astro. Example: --frontend-app console:console:80:vite:admin. package.json must expose lint, typecheck, build:bundle, and test:coverage; may be repeated."
    )]
    pub frontend_apps: Vec<FrontendApp>,
    /// Persisted JavaScript workspace ownership discovered during adoption;
    /// there is intentionally no broad CLI spelling for this generated policy.
    #[arg(skip)]
    pub frontend_workspace_roots: Vec<String>,
    #[arg(skip)]
    pub dev_apps: Vec<DevApp>,
    /// Preserved scalar `[dev]` answers used by both harness and scaffold rendering.
    #[arg(skip)]
    pub dev_settings: Option<DevSettingsAnswers>,
    #[arg(skip)]
    pub execution: Option<ExecutionConfig>,
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
  Presets create starter project code once. After creation, that project code is project-owned.
  `jig update` keeps the Jig harness current; it does not rewrite scaffolded app code.

Interaction modes:
  Interactive terminals prompt only for unresolved project-shape choices.
  --defaults uses rust-react, database none, and frontend web when those choices are omitted.
  --no-input and non-terminal execution require the project shape to be fully specified.

Examples:
  jig init /path/to/new-repo
  jig init /path/to/new-repo --preset harness-only --repo-name new-repo --sqlx-enabled false --no-input --no-vault
  jig init /path/to/new-repo --preset harness-only --no-input --no-vault
  jig init /path/to/new-repo --preset rust-library --no-input --no-vault
  jig init /path/to/new-repo --preset rust-cli --no-input --no-vault
  jig init /path/to/new-repo --preset rust-react
  jig init /path/to/new-repo --preset rust-react --db postgres --frontends web,landing,admin
  jig init /path/to/new-repo --preset go-react --db postgres --frontends web --go-module github.com/acme/new-repo
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
        long_help = "Skip the init wizard and require --preset. The rust-react and go-react application presets require an explicit --db choice plus --frontend/--frontends or effective frontend_apps from --answers-file; go-react also requires --go-module. The harness-only, rust-library, and rust-cli presets need no database or frontend choice and reject those scaffold flags. Non-terminal execution without --defaults follows this strict behavior."
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
    #[command(flatten)]
    pub components: ComponentSelectionOpts,
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
        long_help = "Render a loop-ready minimal footprint: .jig.toml, .agent/jig-contract.json, and .agent/ scaffolding, plus block-managed .gitignore/.gitattributes. Omits scripts/, .github/workflows/, AGENTS.md, agent-map.md. Stores harness_footprint = \"minimal\" so jig update keeps the same footprint until you re-adopt without --minimal."
    )]
    pub minimal: bool,
    #[arg(
        long,
        help = "Use default answers for omitted configuration prompts and adopt write confirmation; vault setup captures credentials before rendering"
    )]
    pub defaults: bool,
    #[arg(
        long,
        help = "Fail instead of prompting for missing answers and skip adopt write confirmation; vault setup requires --no-vault or an operator-provided JIG_VAULT_PASSPHRASE"
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
  jig update --launcher-only repairs only scripts/jig and scripts/install-jig.sh.
  Add --force only when changed template-managed files should be replaced.

Examples:
  jig update
  jig update --recopy
  jig update /path/to/repo --launcher-only --force
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
    #[arg(
        long,
        requires = "force",
        conflicts_with_all = [
            "template",
            "template_mode",
            "recopy",
            "vcs_ref",
            "defaults",
            "no_input"
        ],
        help = "Repair only the managed launcher and installer from this binary's embedded templates"
    )]
    pub launcher_only: bool,
    #[arg(long, help = "Overwrite changed template-managed files")]
    pub force: bool,
    #[arg(long, help = "Git revision to render from the template source")]
    pub vcs_ref: Option<String>,
    #[arg(long, help = "Use default answers for omitted configuration prompts")]
    pub defaults: bool,
    #[arg(long, help = "Fail instead of prompting for missing answers")]
    pub no_input: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum TemplateMode {
    Committed,
}

impl TemplateMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Committed => "committed",
        }
    }
}
