use std::io::Write;

use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use jig_context::RepoContext;
use jig_repository::InspectRequest;

use super::output::{emit, print_json};
use super::run::finish_after_json_output;
use super::structured_error::require_json_ok;
use crate::{doctor, info};

mod freshness;
pub(super) mod render;

pub(super) const INFO_AFTER_HELP: &str = "\
Summarizes what Jig believes about the current repo from .jig.toml and the
generated contract manifest.

Use --commands for repository-specific command availability. Status describes
each root command's primary workflow; setup and diagnostic subcommands or flags
may still work when that status is not ready. Invocation still runs
command-specific preflight.
Stable JSON status codes are ready, not_configured, needs_setup, and unavailable.
When Codex marketplaces are configured, this view checks machine-local Codex
readiness and may wait up to five seconds for that probe.

Human-readable output is the default. Pass --json for structured automation output.

Examples:
  jig info
  jig info --json
  jig info components
  jig info target api:test --json
  jig info --commands
  jig info --commands --json  # also works before adoption
  jig explain --json";

#[derive(Args, Debug, Default)]
pub(crate) struct InfoOpts {
    #[arg(
        long,
        help = "Show root commands with repository-specific availability and remediation"
    )]
    pub(crate) commands: bool,
    #[arg(
        long,
        global = true,
        value_enum,
        default_value_t,
        help = "Select the standard or opt-in agent-v1 inspection projection"
    )]
    pub(crate) projection: jig_repository::surface::ResponseSurface,
    #[command(subcommand)]
    pub(crate) subject: Option<InfoCommand>,
}

impl InfoOpts {
    pub(crate) fn validate_projection(&self) -> anyhow::Result<()> {
        if self.projection == jig_repository::surface::ResponseSurface::Standard
            || matches!(
                self.subject.as_ref(),
                Some(
                    InfoCommand::Workspace
                        | InfoCommand::Component { .. }
                        | InfoCommand::Targets
                        | InfoCommand::Target { .. }
                )
            )
        {
            return Ok(());
        }
        anyhow::bail!(
            "--projection agent-v1 requires a target-bearing info subject: workspace, component, targets, or target"
        )
    }
}

#[derive(Debug, Subcommand)]
pub(crate) enum InfoCommand {
    /// Preview action input declarations and conservative adoption recommendations.
    #[command(alias = "freshness")]
    Inputs(FreshnessOpts),
    /// Print the highest Go module toolchain selector used by managed CI.
    #[command(name = "go-version", hide = true)]
    GoVersion,
    /// Inspect the normalized workspace catalog.
    Workspace,
    /// List addressable repository components.
    Components,
    /// Inspect one component and its targets.
    Component { id: String },
    /// List executable component/action targets.
    Targets,
    /// Inspect one target by its component:action address.
    Target { id: String },
    /// List checked-in target profiles.
    Profiles,
    /// Inspect one checked-in profile.
    Profile { id: String },
}

#[derive(Args, Debug, Default)]
pub(crate) struct FreshnessOpts {
    /// Limit the preview to an exact component:action target (repeatable).
    #[arg(long = "target")]
    pub(crate) targets: Vec<jig_contract::TargetId>,
    /// Assert selected command checks are independent of staging, commits and branches.
    #[arg(long, requires = "targets")]
    pub(crate) assert_worktree: bool,
    /// Assert the reviewed inputs cover every repository file selected checks read.
    #[arg(long, requires = "targets")]
    pub(crate) assert_exhaustive: bool,
    /// Add a repository-relative input glob to each explicitly selected check.
    #[arg(long = "input", requires = "assert_exhaustive")]
    pub(crate) inputs: Vec<String>,
    /// Print a paired unified patch; with --json, include it in the report.
    #[arg(long)]
    pub(crate) patch: bool,
}

pub(super) fn run_info_command(opts: InfoOpts, json_output: bool) -> Result<()> {
    opts.validate_projection()?;
    if let Some(InfoCommand::Inputs(inputs)) = opts.subject.as_ref() {
        if opts.commands {
            bail!("--commands cannot be combined with an info subject");
        }
        return freshness::run(inputs, json_output);
    }
    if matches!(opts.subject.as_ref(), Some(InfoCommand::GoVersion)) {
        if opts.commands {
            bail!("--commands cannot be combined with an info subject");
        }
        return run_go_version(json_output);
    }
    let request = opts.subject.map(|subject| match subject {
        InfoCommand::GoVersion | InfoCommand::Inputs(_) => {
            unreachable!("handled above")
        }
        InfoCommand::Workspace => InspectRequest::Workspace,
        InfoCommand::Components => InspectRequest::Components,
        InfoCommand::Component { id } => InspectRequest::Component(id),
        InfoCommand::Targets => InspectRequest::Targets,
        InfoCommand::Target { id } => InspectRequest::Target(id),
        InfoCommand::Profiles => InspectRequest::Profiles,
        InfoCommand::Profile { id } => InspectRequest::Profile(id),
    });
    let output = info::run(opts.commands, json_output, request, opts.projection)?;
    emit(json_output, render::format_info_summary, &output)?;
    finish_after_json_output(require_json_ok(true, &output), json_output)
}

fn run_go_version(json_output: bool) -> Result<()> {
    let ctx = RepoContext::load()?;
    let selector = doctor::go_version_selector(&ctx)?;
    if json_output {
        print_json(&serde_json::json!({
            "ok": true,
            "command": "info go-version",
            "version": selector,
        }))?;
    } else {
        writeln!(std::io::stdout().lock(), "{selector}")?;
    }
    Ok(())
}
