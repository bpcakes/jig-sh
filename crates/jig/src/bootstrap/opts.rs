use std::path::PathBuf;

use clap::Args;

use super::{DevApp, FrontendApp, parse_frontend_app};
use crate::context::{ExecutionConfig, RustMigrationLayout, StatusConfig};

#[derive(Args, Clone, Debug, Default)]
pub struct AnswerOpts {
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
        long = "frontend-app",
        help_heading = "Common Answers",
        value_parser = parse_frontend_app,
        help = "Existing frontend app to wire into CI and dev checks",
        long_help = "Frontend CI app as name:dir:coverage_threshold[:kind[:role]]. Kind defaults to vite; an omitted role defaults to astro for env-port, admin for the historical admin/admin-panel names, and spa otherwise. Roles accept spa, admin, or astro. Example: --frontend-app console:console:80:vite:admin. package.json must expose lint, typecheck, build:bundle, and test:coverage; may be repeated."
    )]
    pub frontend_apps: Vec<FrontendApp>,
    #[arg(skip)]
    pub dev_apps: Vec<DevApp>,
    #[arg(skip)]
    pub(crate) status: Option<StatusConfig>,
    #[arg(skip)]
    pub(crate) execution: Option<ExecutionConfig>,
}
