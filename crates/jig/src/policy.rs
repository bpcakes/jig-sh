use std::collections::{BTreeMap, HashSet};
use std::error::Error;
use std::fmt;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use jig_owned_process::{
    BoundedProcessOutput, ProcessOutputLimits, run_owned_process_tree_with_output_limits,
};
use serde_json::{Value, json};

use crate::context::{RepoContext, WorkGate};
#[cfg(test)]
use crate::execution::NoopExecutionObserver;
use crate::execution::{ExecutionCommandError, ExecutionControl};
use crate::repository_path::validate_repository_directory_path;
use crate::tool_defs::{self, kind};

const EMPTY_TREE_HASH: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
// New or growing files above this fail unless an explicit exception is present.
const HARD_LIMIT: usize = 800;
// Files above this fail even with an exception unless they are legacy and non-increasing.
const ABSOLUTE_MAX: usize = 1000;
// Files entering this band warn but do not fail.
const SOFT_LIMIT_START: usize = 500;
// Files above this warn that they are approaching the hard limit.
const SOFT_LIMIT_END: usize = 600;
// Files above this emit informational guidance for agent-review ergonomics.
const TARGET_HIGH: usize = 400;
// Git output is sometimes repository authority rather than user-facing
// diagnostics (for example an unborn schema snapshot's complete file list).
// Keep that capture bounded, but large enough for ordinary repositories and
// fail closed below if the bound is ever exceeded.
const CONTROLLED_GIT_OUTPUT_LIMIT: usize = 64 * 1024 * 1024;

pub(crate) struct AgentMapInput {
    pub(crate) map_path: PathBuf,
}

pub(crate) struct RustFileLocInput {
    pub(crate) staged: bool,
    pub(crate) changed_against: Option<String>,
    pub(crate) all: bool,
}

pub(crate) struct MigrationImmutabilityInput {
    pub(crate) changed_against: String,
}

pub(crate) struct SqlxTodoInput {
    pub(crate) output: Option<PathBuf>,
}

#[derive(Debug)]
pub(crate) struct NativeToolOutput {
    pub(crate) exit_status: i32,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

#[derive(Debug)]
pub(crate) struct ContractValidationError {
    errors: Vec<String>,
}

impl fmt::Display for ContractValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, error) in self.errors.iter().enumerate() {
            if index > 0 {
                formatter.write_str("\n")?;
            }
            write!(formatter, "ERROR: {error}")?;
        }
        Ok(())
    }
}

impl Error for ContractValidationError {}

pub(crate) enum PolicyDirectCommand {
    AgentMapGenerate(AgentMapInput),
    GenerateSqlxUncheckedQueriesTodo(SqlxTodoInput),
}

pub(crate) fn run_direct(ctx: &RepoContext, command: PolicyDirectCommand) -> Result<Value> {
    match command {
        PolicyDirectCommand::AgentMapGenerate(opts) => agent_map::generate(ctx, &opts),
        PolicyDirectCommand::GenerateSqlxUncheckedQueriesTodo(opts) => {
            sqlx::generate_todo(ctx, &opts)
        }
    }
}

pub(crate) enum PolicyCheckCommand {
    AgentMap(AgentMapInput),
    AgentGuides,
    RustFileLoc(RustFileLocInput),
    NoModRs,
    MigrationImmutability(MigrationImmutabilityInput),
    SqlxUncheckedNonTest,
}

pub(crate) fn run_check(ctx: &RepoContext, command: PolicyCheckCommand) -> Result<Value> {
    match command {
        PolicyCheckCommand::AgentMap(opts) => agent_map::check(ctx, &opts),
        PolicyCheckCommand::AgentGuides => agent_map::check_guides(ctx),
        PolicyCheckCommand::RustFileLoc(opts) => check_rust_file_loc(ctx, &opts),
        PolicyCheckCommand::NoModRs => check_no_mod_rs(ctx),
        PolicyCheckCommand::MigrationImmutability(opts) => check_migration_immutability(ctx, &opts),
        PolicyCheckCommand::SqlxUncheckedNonTest => sqlx::check_non_test(ctx),
    }
}

pub(crate) fn contract_check(ctx: &RepoContext) -> NativeToolOutput {
    if let Err(error) = validate_contract(ctx) {
        let mut stderr = String::new();
        for error in error.errors {
            writeln!(&mut stderr, "ERROR: {error}")
                .expect("writing contract errors to a String cannot fail");
        }
        return NativeToolOutput {
            exit_status: 1,
            stdout: String::new(),
            stderr,
        };
    }

    let manifest_path = ctx.root().join(".agent/jig-contract.json");
    NativeToolOutput {
        exit_status: 0,
        stdout: format!(
            "jig contract check passed.\n  - manifest: {}\n  - contract version: {}\n  - runtime version: {}\n  - tool definitions: {}\n",
            manifest_path.display(),
            ctx.contract_version(),
            env!("CARGO_PKG_VERSION"),
            ctx.tool_specs().len()
        ),
        stderr: String::new(),
    }
}

pub(crate) fn validate_contract(
    ctx: &RepoContext,
) -> std::result::Result<(), ContractValidationError> {
    let mut errors = Vec::new();
    let root = ctx.root();
    let mcp_path = root.join(".mcp.json");
    let jig_script = root.join("scripts/jig");
    let install_script = root.join("scripts/install-jig.sh");

    if ctx.tool_specs().iter().any(|tool| tool.kind == "memory") {
        errors.push("Runtime state tools must not be declared in .agent/jig-contract.json.".into());
    }
    if !ctx.is_minimal_footprint() {
        if !mcp_path.exists() {
            errors.push("Missing .mcp.json.".into());
        }
        if !jig_script.exists() {
            errors.push("Missing scripts/jig launcher.".into());
        }
        if !install_script.exists() {
            errors.push("Missing scripts/install-jig.sh installer.".into());
        }
    }
    if ctx.migration_policy_enabled() && ctx.migration_dir().trim().is_empty() {
        errors.push(
            "Migration immutability is enabled, but migration_dir is empty and no legacy rust_migration_dir fallback is configured."
                .into(),
        );
    }
    // RepoContext construction has already rejected unsupported contract epochs.
    for command_key in ctx.required_commands() {
        if !ctx.supports_command_key(command_key) {
            errors.push(format!(
                "Unsupported required command in jig contract: {command_key}."
            ));
            continue;
        }
        if let Err(error) = ctx.command_for_key(command_key) {
            errors.push(error.to_string());
        }
    }
    if ctx.contract_version() >= 6 {
        match crate::repository::RepositoryCatalog::from_context(ctx) {
            Ok(catalog) => {
                for gate in ctx.work_gates() {
                    let WorkGate::Evidence(gate) = gate else {
                        continue;
                    };
                    if let Err(error) =
                        crate::repository::resolve_evidence_targets(&catalog, &gate.selector)
                    {
                        errors.push(format!("Work gate '{}': {error}.", gate.id));
                    }
                }
            }
            Err(error) => errors.push(format!("Invalid repository model: {error}.")),
        }
        for action in ctx.action_specs() {
            match &action.runner {
                jig_contract::ActionRunner::Command { command, .. } => {
                    if !ctx
                        .required_commands()
                        .iter()
                        .any(|required| required == command)
                    {
                        errors.push(format!(
                            "Target {} references command {command}, but it is not declared in required_commands.",
                            action.target
                        ));
                    } else if let Err(error) = ctx.command_for_key(command) {
                        errors.push(format!("Target {}: {error}.", action.target));
                    }
                }
                jig_contract::ActionRunner::Native { operation } => {
                    if !jig_features::is_supported_native_tool(operation) {
                        errors.push(format!(
                            "Target {} references unsupported native operation {operation}.",
                            action.target
                        ));
                    }
                }
            }
        }
    } else {
        for gate in ctx.work_gates() {
            if let WorkGate::Evidence(gate) = gate {
                errors.push(format!(
                    "Work gate '{}': evidence gates require jig contract version 6 or later.",
                    gate.id
                ));
            }
        }
    }

    let tool_names = ctx
        .tool_specs()
        .iter()
        .map(|tool| tool.name.as_str())
        .collect::<HashSet<_>>();
    for required in jig_features::required_contract_tools(ctx) {
        if !tool_names.contains(required) {
            errors.push(format!("Missing required jig tool definition: {required}."));
        }
    }
    for tool in ctx.tool_specs() {
        if let Some(error) = jig_features::tool_admission_error(ctx, &tool.name) {
            errors.push(error);
        }
        match tool.kind.as_str() {
            kind::NATIVE => {
                if !jig_features::is_supported_native_tool(&tool.name) {
                    errors.push(format!("Unsupported native tool: {}.", tool.name));
                }
            }
            kind::COMMAND => {
                let Some(command_key) = tool.command.as_deref().filter(|key| !key.is_empty())
                else {
                    errors.push(format!(
                        "Command-backed tool {} is missing command.",
                        tool.name
                    ));
                    continue;
                };
                if !ctx
                    .required_commands()
                    .iter()
                    .any(|required| required == command_key)
                {
                    errors.push(format!(
                        "Command-backed tool {} references undeclared command {command_key}.",
                        tool.name
                    ));
                }
            }
            other => errors.push(format!("Unsupported tool kind for {}: {other}.", tool.name)),
        }
    }

    let mut work_tools = HashSet::new();
    for name in ctx.work_check_tools() {
        if !work_tools.insert(name.clone()) {
            continue;
        }
        let Some(tool) = ctx.tool_spec(&name) else {
            errors.push(format!(
                "Configured work check or gate references undeclared tool: {name}."
            ));
            continue;
        };
        if !tool_defs::is_no_arg_execution_tool(tool) {
            if !tool_defs::is_execution_tool(tool) {
                errors.push(format!(
                    "Configured work check or gate tool is not an execution tool: {name}."
                ));
            } else {
                errors.push(format!(
                    "Configured work check or gate tool requires an argument: {name}."
                ));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(ContractValidationError { errors })
    }
}

mod migration_add;
pub(crate) use migration_add::migration_add;

#[cfg(test)]
pub(crate) fn schema_check(ctx: &RepoContext) -> Result<NativeToolOutput> {
    schema_check_with_observer(ctx, &mut NoopExecutionObserver)
        .map_err(ExecutionCommandError::into_anyhow)
}

pub(crate) fn schema_check_with_observer(
    ctx: &RepoContext,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<NativeToolOutput, ExecutionCommandError> {
    if observer.cancelled() {
        return Err(ExecutionCommandError::CancelledBeforeStart);
    }
    schema::check_with_control(ctx, ctx.command_timeout().duration(), &|| {
        observer.cancelled()
    })
    .map_err(|error| schema_execution_error(error, ctx.command_timeout().as_secs()))
}

pub(crate) fn schema_check_with_control(
    ctx: &RepoContext,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeToolOutput> {
    schema::check_with_control(ctx, timeout, cancelled)
}

fn schema_execution_error(error: anyhow::Error, timeout_seconds: u64) -> ExecutionCommandError {
    match error.downcast_ref::<jig_owned_process::OwnedProcessTreeError>() {
        Some(jig_owned_process::OwnedProcessTreeError::CancelledBeforeStart) => {
            ExecutionCommandError::CancelledBeforeStart
        }
        Some(jig_owned_process::OwnedProcessTreeError::Cancelled) => {
            ExecutionCommandError::Cancelled
        }
        Some(jig_owned_process::OwnedProcessTreeError::TimedOut) => ExecutionCommandError::failed(
            anyhow::anyhow!("Schema check timed out after {timeout_seconds} seconds"),
        ),
        _ => ExecutionCommandError::failed(error),
    }
}

struct ControlledOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

struct ControlledBytesOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
}

fn controlled_output(
    command: &mut Command,
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<ControlledOutput> {
    controlled_output_with_limits(command, deadline, ProcessOutputLimits::default(), cancelled)
}

fn controlled_output_with_limits(
    command: &mut Command,
    deadline: Instant,
    limits: ProcessOutputLimits,
    cancelled: &dyn Fn() -> bool,
) -> Result<ControlledOutput> {
    let output = controlled_output_bytes_with_limits(command, deadline, limits, cancelled)?;
    Ok(ControlledOutput {
        status: output.status,
        stdout: controlled_bytes_text(output.stdout, output.stdout_truncated),
        stderr: controlled_bytes_text(output.stderr, output.stderr_truncated),
    })
}

fn controlled_output_bytes_with_limits(
    command: &mut Command,
    deadline: Instant,
    limits: ProcessOutputLimits,
    cancelled: &dyn Fn() -> bool,
) -> Result<ControlledBytesOutput> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(jig_owned_process::OwnedProcessTreeError::TimedOut.into());
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = run_owned_process_tree_with_output_limits(command, remaining, limits, cancelled)?;
    let stdout_truncated = output
        .stdout
        .as_ref()
        .is_some_and(|output| output.truncated);
    let stderr_truncated = output
        .stderr
        .as_ref()
        .is_some_and(|output| output.truncated);
    let stdout = controlled_output_bytes(output.stdout, "stdout")?;
    let stderr = controlled_output_bytes(output.stderr, "stderr")?;
    Ok(ControlledBytesOutput {
        status: output.status,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    })
}

#[cfg(test)]
fn controlled_output_text(output: Option<BoundedProcessOutput>, stream: &str) -> Result<String> {
    let output = output.with_context(|| format!("{stream} was not captured"))?;
    let truncated = output.truncated;
    let bytes = controlled_output_bytes(Some(output), stream)?;
    Ok(controlled_bytes_text(bytes, truncated))
}

fn controlled_output_bytes(output: Option<BoundedProcessOutput>, stream: &str) -> Result<Vec<u8>> {
    let output = output.with_context(|| format!("{stream} was not captured"))?;
    if !output.complete {
        bail!("{stream} capture did not complete");
    }
    Ok(output.bytes)
}

fn controlled_bytes_text(bytes: Vec<u8>, truncated: bool) -> String {
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        text.push_str("\n[output truncated by Jig]\n");
    }
    text
}

fn controlled_git_text(
    root: &Path,
    args: &[&str],
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<String> {
    let output = controlled_git_output(root, args, deadline, cancelled)?;
    if !output.status.success() {
        bail!(
            "git {} failed with status {}\nstderr:\n{}",
            args.join(" "),
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8(output.stdout)
        .with_context(|| format!("git {} returned non-UTF-8 text", args.join(" ")))
}

fn controlled_git_bytes(
    root: &Path,
    args: &[&str],
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>> {
    let output = controlled_git_output(root, args, deadline, cancelled)?;
    if !output.status.success() {
        bail!(
            "git {} failed with status {}\nstderr:\n{}",
            args.join(" "),
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output.stdout)
}

fn controlled_git_output(
    root: &Path,
    args: &[&str],
    deadline: Instant,
    cancelled: &dyn Fn() -> bool,
) -> Result<ControlledBytesOutput> {
    let mut command = Command::new("git");
    command.current_dir(root).args(args);
    crate::bootstrap::scrub_known_repository_git_environment(&mut command);
    let output = controlled_output_bytes_with_limits(
        &mut command,
        deadline,
        ProcessOutputLimits {
            stdout: CONTROLLED_GIT_OUTPUT_LIMIT,
            stderr: CONTROLLED_GIT_OUTPUT_LIMIT,
        },
        cancelled,
    )?;
    if output.stdout_truncated || output.stderr_truncated {
        bail!(
            "git {} output exceeded the {} byte schema-check capture limit",
            args.join(" "),
            CONTROLLED_GIT_OUTPUT_LIMIT
        );
    }
    Ok(output)
}

pub(crate) fn write_agent_map(root: &Path, map_path: &Path) -> Result<()> {
    agent_map::write(root, map_path)
}

pub(crate) fn render_agent_map(root: &Path, map_path: &Path) -> Result<Vec<u8>> {
    agent_map::render(root, map_path)
}

fn check_no_mod_rs(ctx: &RepoContext) -> Result<Value> {
    let tracked = git_list_files(ctx.root(), ctx.rust_crate_roots())?;
    let violations = tracked
        .into_iter()
        .filter(|path| path == "mod.rs" || path.ends_with("/mod.rs"))
        .collect::<Vec<_>>();
    Ok(json!({ "ok": violations.is_empty(), "violations": violations }))
}

fn check_rust_file_loc(ctx: &RepoContext, opts: &RustFileLocInput) -> Result<Value> {
    let mode_count = [opts.staged, opts.changed_against.is_some(), opts.all]
        .into_iter()
        .filter(|value| *value)
        .count();
    if mode_count != 1 {
        bail!("Exactly one of --staged, --changed-against, or --all is required.");
    }
    let previous_ref = if opts.staged {
        if git_success(ctx.root(), &["rev-parse", "--verify", "HEAD"])? {
            "HEAD".to_string()
        } else {
            EMPTY_TREE_HASH.into()
        }
    } else if let Some(ref_name) = &opts.changed_against {
        ref_name.clone()
    } else {
        EMPTY_TREE_HASH.into()
    };
    let candidates = rust_candidate_files(ctx, opts)?;
    let renames = if opts.all {
        BTreeMap::new()
    } else {
        rust_renames(ctx, opts)?
    };
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut infos = Vec::new();
    for file in candidates {
        if !ctx.root().join(&file).is_file() && !opts.staged {
            continue;
        }
        let current = if opts.staged {
            git_blob(ctx.root(), &format!(":{file}"))?
        } else {
            let path = ctx.root().join(&file);
            fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?
        };
        let current_count = current.lines().count();
        let previous_count =
            previous_line_count(ctx.root(), &previous_ref, &file, renames.get(&file))?;
        let has_exception = current
            .lines()
            .take(40)
            .any(|line| line.contains("agentic-loc-exception:") || line.contains("@generated"));
        if current_count > ABSOLUTE_MAX {
            if current_count <= previous_count && previous_count > ABSOLUTE_MAX {
                warnings.push(format!(
                    "{file} remains above the absolute max at {current_count} LOC but did not increase."
                ));
            } else {
                errors.push(format!(
                    "{file} is {current_count} LOC, above the absolute max of {ABSOLUTE_MAX}."
                ));
            }
        } else if current_count > HARD_LIMIT {
            if current_count <= previous_count && previous_count > HARD_LIMIT {
                warnings.push(format!(
                    "{file} remains above the hard limit at {current_count} LOC but did not increase."
                ));
            } else if has_exception {
                warnings.push(format!(
                    "{file} is {current_count} LOC and uses an explicit exception annotation."
                ));
            } else {
                errors.push(format!(
                    "{file} is {current_count} LOC, above the hard limit of {HARD_LIMIT}."
                ));
            }
        } else if current_count > SOFT_LIMIT_END {
            warnings.push(format!(
                "{file} is {current_count} LOC and is approaching the hard limit."
            ));
        } else if current_count > SOFT_LIMIT_START {
            warnings.push(format!(
                "{file} is {current_count} LOC and is above the soft limit."
            ));
        } else if current_count > TARGET_HIGH {
            infos.push(format!(
                "{file} is {current_count} LOC and is approaching the soft limit."
            ));
        }
    }
    Ok(json!({
        "ok": errors.is_empty(),
        "errors": errors,
        "warnings": warnings,
        "infos": infos,
    }))
}

fn check_migration_immutability(
    ctx: &RepoContext,
    opts: &MigrationImmutabilityInput,
) -> Result<Value> {
    let dir = ctx.migration_dir();
    if dir.trim().is_empty() {
        bail!("migration_dir is empty and no legacy rust_migration_dir fallback is configured");
    }
    let bytes = git_output(
        ctx.root(),
        &[
            "diff",
            "--name-status",
            "-z",
            "-M",
            "--diff-filter=ADMRT",
            &opts.changed_against,
            "HEAD",
            "--",
            dir,
        ],
    )?;
    let violations = migration_immutability_violations(&bytes);
    Ok(json!({ "ok": violations.is_empty(), "violations": violations }))
}

fn migration_immutability_violations(bytes: &[u8]) -> Vec<String> {
    let mut violations = Vec::new();
    let entries = split_nul(bytes);
    let mut index = 0usize;
    while index < entries.len() {
        let status = &entries[index];
        index += 1;
        if status == "A" {
            index += 1;
        } else if status.starts_with('R') {
            if index + 2 > entries.len() {
                break;
            }
            let old_path = entries[index].clone();
            let new_path = entries[index + 1].clone();
            index += 2;
            violations.push(format!(
                "{old_path}: Existing migration files are immutable. Rename detected ({status}): {old_path} -> {new_path}. Add a new forward-only migration instead."
            ));
        } else if index < entries.len() {
            let path = entries[index].clone();
            index += 1;
            violations.push(format!(
                "{path}: Existing migration files are immutable. Change detected ({status}) in {path}. Add a new forward-only migration instead."
            ));
        }
    }
    violations
}

fn rust_candidate_files(ctx: &RepoContext, opts: &RustFileLocInput) -> Result<Vec<String>> {
    let mut args = vec!["diff", "--name-only", "--diff-filter=AMRT", "-z"];
    let changed_ref;
    if opts.staged {
        args.push("--cached");
    } else if let Some(reference) = &opts.changed_against {
        changed_ref = reference.as_str();
        args.push(changed_ref);
        args.push("HEAD");
    }
    if opts.all {
        return git_list_files(ctx.root(), ctx.rust_crate_roots()).map(|files| {
            files
                .into_iter()
                .filter(|path| path.ends_with(".rs"))
                .collect::<Vec<_>>()
        });
    }
    args.push("--");
    let root_args = ctx
        .rust_crate_roots()
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    args.extend(root_args);
    Ok(split_nul(&git_output(ctx.root(), &args)?)
        .into_iter()
        .filter(|path| path.ends_with(".rs"))
        .collect())
}

fn rust_renames(ctx: &RepoContext, opts: &RustFileLocInput) -> Result<BTreeMap<String, String>> {
    let mut args = vec!["diff", "--name-status", "--diff-filter=R", "-z"];
    if opts.staged {
        args.push("--cached");
    } else if let Some(reference) = &opts.changed_against {
        args.push(reference);
        args.push("HEAD");
    }
    args.push("--");
    let root_args = ctx
        .rust_crate_roots()
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    args.extend(root_args);
    let entries = split_nul(&git_output(ctx.root(), &args)?);
    let mut renames = BTreeMap::new();
    let mut index = 0usize;
    while index + 2 < entries.len() {
        let _status = &entries[index];
        let old = entries[index + 1].clone();
        let new = entries[index + 2].clone();
        renames.insert(new, old);
        index += 3;
    }
    Ok(renames)
}

fn previous_line_count(
    root: &Path,
    reference: &str,
    path: &str,
    renamed_from: Option<&String>,
) -> Result<usize> {
    if let Some(contents) = git_blob_optional(root, &format!("{reference}:{path}"))? {
        return Ok(contents.lines().count());
    }
    let Some(old) = renamed_from else {
        return Ok(0);
    };
    Ok(git_blob_optional(root, &format!("{reference}:{old}"))?
        .map(|contents| contents.lines().count())
        .unwrap_or(0))
}

fn git_list_files(root: &Path, roots: &[String]) -> Result<Vec<String>> {
    let mut args = vec!["ls-files", "-z", "--"];
    args.extend(roots.iter().map(String::as_str));
    Ok(split_nul(&git_output(root, &args)?))
}

fn git_blob(root: &Path, spec: &str) -> Result<String> {
    git_text(root, &["show", spec])
}

fn git_blob_optional(root: &Path, spec: &str) -> Result<Option<String>> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["show", spec])
        .stderr(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()))
}

pub(super) fn git_success(root: &Path, args: &[&str]) -> Result<bool> {
    Ok(Command::new("git")
        .current_dir(root)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?
        .success())
}

fn git_text(root: &Path, args: &[&str]) -> Result<String> {
    Ok(String::from_utf8_lossy(&git_output(root, args)?).into_owned())
}

pub(super) fn git_output(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git").current_dir(root).args(args).output()?;
    if !output.status.success() {
        bail!(
            "git {} failed with status {}\nstderr:\n{}",
            args.join(" "),
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output.stdout)
}

pub(super) fn split_nul(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect()
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut last_was_sep = false;
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_was_sep = false;
        } else if !last_was_sep && !slug.is_empty() {
            slug.push('_');
            last_was_sep = true;
        }
    }
    slug.trim_matches('_').to_string()
}

fn utc_timestamp() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{:04}{:02}{:02}{:02}{:02}{:02}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

mod agent_map;
mod schema;
mod sqlx;
#[cfg(test)]
mod tests;
