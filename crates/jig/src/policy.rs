use std::collections::HashSet;
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

// Git output is sometimes repository authority rather than user-facing
// diagnostics (for example an unborn schema snapshot's complete file list).
// Keep that capture bounded, but large enough for ordinary repositories and
// fail closed below if the bound is ever exceeded.
const CONTROLLED_GIT_OUTPUT_LIMIT: usize = 64 * 1024 * 1024;

pub(crate) struct AgentMapInput {
    pub(crate) map_path: PathBuf,
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
    MigrationImmutability(MigrationImmutabilityInput),
    SqlxUncheckedNonTest,
}

pub(crate) fn run_check(ctx: &RepoContext, command: PolicyCheckCommand) -> Result<Value> {
    match command {
        PolicyCheckCommand::AgentMap(opts) => agent_map::check(ctx, &opts),
        PolicyCheckCommand::AgentGuides => agent_map::check_guides(ctx),
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
    validate_contract_basics(ctx, &mut errors);
    validate_required_commands(ctx, &mut errors);
    let catalog = validate_repository_model(ctx, &mut errors);
    validate_actions_and_evidence_gates(ctx, &mut errors);

    validate_tool_definitions(ctx, catalog.as_ref(), &mut errors);
    validate_work_tools(ctx, catalog.as_ref(), &mut errors);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(ContractValidationError { errors })
    }
}

fn validate_contract_basics(ctx: &RepoContext, errors: &mut Vec<String>) {
    if ctx.tool_specs().iter().any(|tool| tool.kind == "memory") {
        errors.push("Runtime state tools must not be declared in .agent/jig-contract.json.".into());
    }
    if !ctx.is_minimal_footprint() {
        for (path, message) in [
            (ctx.root().join(".mcp.json"), "Missing .mcp.json."),
            (
                ctx.root().join("scripts/jig"),
                "Missing scripts/jig launcher.",
            ),
            (
                ctx.root().join("scripts/install-jig.sh"),
                "Missing scripts/install-jig.sh installer.",
            ),
        ] {
            if !path.exists() {
                errors.push(message.into());
            }
        }
    }
    if ctx.migration_policy_enabled() && ctx.migration_dir().trim().is_empty() {
        errors.push(
            "Migration immutability is enabled, but migration_dir is empty and no legacy rust_migration_dir fallback is configured."
                .into(),
        );
    }
}

fn validate_required_commands(ctx: &RepoContext, errors: &mut Vec<String>) {
    // RepoContext construction has already rejected unsupported contract epochs.
    for command_key in ctx.required_commands() {
        if !ctx.supports_command_key(command_key) {
            errors.push(format!(
                "Unsupported required command in jig contract: {command_key}."
            ));
        } else if let Err(error) = ctx.command_for_key(command_key) {
            errors.push(error.to_string());
        }
    }
}

fn validate_repository_model(
    ctx: &RepoContext,
    errors: &mut Vec<String>,
) -> Option<crate::repository::RepositoryCatalog> {
    if ctx.contract_version() < 6 {
        return None;
    }
    let catalog = match crate::repository::RepositoryCatalog::from_context(ctx) {
        Ok(catalog) => catalog,
        Err(error) => {
            errors.push(format!("Invalid repository model: {error}."));
            return None;
        }
    };
    for gate in ctx.work_gates() {
        let WorkGate::Evidence(gate) = gate else {
            continue;
        };
        if let Err(error) = crate::repository::resolve_evidence_targets(&catalog, &gate.selector) {
            errors.push(format!("Work gate '{}': {error}.", gate.id));
        }
    }
    Some(catalog)
}

fn validate_actions_and_evidence_gates(ctx: &RepoContext, errors: &mut Vec<String>) {
    if ctx.contract_version() < 6 {
        for gate in ctx.work_gates() {
            if let WorkGate::Evidence(gate) = gate {
                errors.push(format!(
                    "Work gate '{}': evidence gates require jig contract version 6 or later.",
                    gate.id
                ));
            }
        }
        return;
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
            jig_contract::ActionRunner::Native { operation, .. } => {
                if !jig_features::is_supported_native_tool(operation) {
                    errors.push(format!(
                        "Target {} references unsupported native operation {operation}.",
                        action.target
                    ));
                } else if operation == jig_contract::tool::SCHEMA_CHECK
                    && let Err(error) = schema::validate_runner(ctx, &action.target)
                {
                    errors.push(format!("Target {}: {error}.", action.target));
                }
            }
        }
    }
}

fn validate_tool_definitions(
    ctx: &RepoContext,
    catalog: Option<&crate::repository::RepositoryCatalog>,
    errors: &mut Vec<String>,
) {
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
        validate_tool_definition(ctx, catalog, tool, errors);
    }
}

fn validate_tool_definition(
    ctx: &RepoContext,
    catalog: Option<&crate::repository::RepositoryCatalog>,
    tool: &jig_contract::ManifestTool,
    errors: &mut Vec<String>,
) {
    let alias_action = catalog.and_then(|catalog| catalog.action_for_alias(&tool.name));
    if ctx.contract_version() >= 6 && catalog.is_some() && alias_action.is_none() {
        errors.push(format!(
            "Contract-v6 tool {} is not mapped to a repository action through legacy_aliases.",
            tool.name
        ));
    }
    let native_operation = alias_action.and_then(|action| match &action.runner {
        jig_contract::ActionRunner::Native { operation, .. } => Some(operation.as_str()),
        jig_contract::ActionRunner::Command { .. } => None,
    });
    let admission_name = native_operation.unwrap_or(&tool.name);
    if let Some(error) = jig_features::tool_admission_error(ctx, admission_name) {
        errors.push(error);
    }
    match tool.kind.as_str() {
        kind::NATIVE => validate_native_tool(tool, alias_action, native_operation, errors),
        kind::COMMAND => validate_command_tool(ctx, tool, alias_action, errors),
        other => errors.push(format!("Unsupported tool kind for {}: {other}.", tool.name)),
    }
}

fn validate_native_tool(
    tool: &jig_contract::ManifestTool,
    alias_action: Option<&jig_contract::ActionSpec>,
    native_operation: Option<&str>,
    errors: &mut Vec<String>,
) {
    if matches!(
        alias_action.map(|action| &action.runner),
        Some(jig_contract::ActionRunner::Command { .. })
    ) {
        errors.push(format!(
            "Native tool {} aliases a command-backed action.",
            tool.name
        ));
        return;
    }
    let operation = native_operation.unwrap_or(&tool.name);
    if jig_features::is_supported_native_tool(operation) {
        return;
    }
    if operation == tool.name {
        errors.push(format!("Unsupported native tool: {}.", tool.name));
    } else {
        errors.push(format!(
            "Unsupported native operation {operation} for tool {}.",
            tool.name
        ));
    }
}

fn validate_command_tool(
    ctx: &RepoContext,
    tool: &jig_contract::ManifestTool,
    alias_action: Option<&jig_contract::ActionSpec>,
    errors: &mut Vec<String>,
) {
    if let Some(jig_contract::ActionRunner::Native { operation, .. }) =
        alias_action.map(|action| &action.runner)
    {
        errors.push(format!(
            "Command-backed tool {} aliases native operation {operation}.",
            tool.name
        ));
        return;
    }
    let Some(command_key) = tool.command.as_deref().filter(|key| !key.is_empty()) else {
        errors.push(format!(
            "Command-backed tool {} is missing command.",
            tool.name
        ));
        return;
    };
    if let Some(jig_contract::ActionRunner::Command { command, .. }) =
        alias_action.map(|action| &action.runner)
        && command != command_key
    {
        errors.push(format!(
            "Command-backed tool {} projects command {command_key}, but its owning action uses {command}.",
            tool.name
        ));
    }
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

fn validate_work_tools(
    ctx: &RepoContext,
    catalog: Option<&crate::repository::RepositoryCatalog>,
    errors: &mut Vec<String>,
) {
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
        if !tool_defs::is_execution_tool(tool) {
            errors.push(format!(
                "Configured work check or gate tool is not an execution tool: {name}."
            ));
            continue;
        }
        let native_operation = catalog
            .and_then(|catalog| catalog.action_for_alias(&name))
            .and_then(|action| match &action.runner {
                jig_contract::ActionRunner::Native { operation, .. } => Some(operation.as_str()),
                jig_contract::ActionRunner::Command { .. } => None,
            });
        if tool_defs::execution_tool_requires_name_for_native_operation(tool, native_operation) {
            errors.push(format!(
                "Configured work check or gate tool requires an argument: {name}."
            ));
        }
    }
}

mod migration_add;
pub(crate) use migration_add::migration_add;

#[cfg(test)]
pub(crate) fn schema_check(ctx: &RepoContext) -> Result<NativeToolOutput> {
    schema_check_with_observer_and_timeout(
        ctx,
        None,
        ctx.command_timeout().duration(),
        &mut NoopExecutionObserver,
    )
    .map_err(ExecutionCommandError::into_anyhow)
}

pub(crate) fn schema_check_with_observer_and_timeout(
    ctx: &RepoContext,
    schema_check_target: Option<&jig_contract::TargetId>,
    timeout: Duration,
    observer: &mut dyn ExecutionControl,
) -> std::result::Result<NativeToolOutput, ExecutionCommandError> {
    if observer.cancelled() {
        return Err(ExecutionCommandError::CancelledBeforeStart);
    }
    schema::check_with_control(ctx, schema_check_target, timeout, &|| observer.cancelled())
        .map_err(|error| schema_execution_error(error, timeout.as_secs()))
}

pub(crate) fn schema_check_with_control(
    ctx: &RepoContext,
    schema_check_target: Option<&jig_contract::TargetId>,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<NativeToolOutput> {
    schema::check_with_control(ctx, schema_check_target, timeout, cancelled)
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

fn git_list_files(root: &Path, roots: &[String]) -> Result<Vec<String>> {
    let mut args = vec!["ls-files", "-z", "--"];
    args.extend(roots.iter().map(String::as_str));
    Ok(split_nul(&git_output(root, &args)?))
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

fn utc_timestamp_at(now: time::OffsetDateTime) -> String {
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
