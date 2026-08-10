use std::ffi::OsString;
use std::io::Write;
use std::process;

use anyhow::{Context, Result, bail};
use clap::{
    Parser,
    error::{ContextKind, ContextValue, ErrorKind},
};

use super::bootstrap_run::{
    run_adopt_command, run_init_command, run_presets_command, run_update_command,
};
use super::codex_run::run_codex_command;
use super::output::{HumanOutput, emit, print_json};
use super::prompt_run::run_prompt_command;
use super::setup_run::run_setup_command;
use super::structured_error::{
    is_json_output_already_emitted, json_error_payload, json_output_already_emitted,
    json_reported_error, require_foreground_status, require_json_ok,
};
pub(crate) use super::structured_error::{is_structured_json_failure, structured_error_exit_code};
use super::vault_run::run_vault_command;
use super::*;

pub(crate) fn run() -> Result<()> {
    let cli = parse_cli();
    let json_output = cli.json;
    let report_json_errors = should_report_json_command_errors(json_output, &cli.command);
    let result = validate_launcher_repository_scope(&cli).and_then(|()| run_command(cli));
    if report_json_errors {
        return report_json_command_error(result);
    }
    result
}

fn validate_launcher_repository_scope(cli: &Cli) -> Result<()> {
    let (Some(contract_version), Some(profile), Some(repo_root)) = (
        cli.launcher_contract_version,
        cli.launcher_profile,
        cli.launcher_repo_root.as_deref(),
    ) else {
        if cli.launcher_contract_version.is_none()
            && cli.launcher_profile.is_none()
            && cli.launcher_repo_root.is_none()
        {
            return Ok(());
        }
        bail!("Incomplete generated-launcher repository validation handoff");
    };
    if launcher_capability_only_command(&cli.command) {
        bail!(
            "The generated launcher and this Jig runtime disagree about whether `{}` is repository-scoped. Repair the launcher/runtime pair with a current external Jig binary (`jig update <repo> --launcher-only --force`) before retrying `{}`.",
            top_level_command_name(&cli.command),
            top_level_command_name(&cli.command),
        );
    }
    let ctx = validate_runtime_compatibility(repo_root, Some(contract_version), profile, false)
    .with_context(|| {
        format!(
            "The repository contract did not validate under Jig profile {}. Run scripts/jig check contract or scripts/jig doctor for repair guidance.",
            profile.as_str()
        )
    })?
    .context("Strict launcher validation did not produce a repository context")?;
    if let Some(configured_root) = std::env::var_os(crate::context::JIG_REPO_ROOT_ENV) {
        let configured_root = std::path::PathBuf::from(configured_root);
        if !configured_root.as_os_str().is_empty()
            && std::fs::canonicalize(&configured_root).ok().as_deref() != Some(ctx.root())
        {
            eprintln!(
                "jig ignored {}={} because the generated launcher root {} is authoritative",
                crate::context::JIG_REPO_ROOT_ENV,
                configured_root.display(),
                ctx.root().display()
            );
        }
    }
    let authoritative_root = ctx.root().to_path_buf();
    ctx.remember_prevalidated_launcher_context()?;
    // SAFETY: launcher validation runs at CLI startup, before command dispatch
    // can create worker threads. Descendants must inherit the same canonical
    // root that this process has already validated as launcher-authoritative.
    unsafe {
        std::env::set_var(crate::context::JIG_REPO_ROOT_ENV, authoritative_root);
    }
    Ok(())
}

fn launcher_capability_only_command(command: &CommandKind) -> bool {
    launcher_command_scope(command) == LauncherCommandScope::CapabilityOnly
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LauncherCommandScope {
    CapabilityOnly,
    Repository,
}

const fn launcher_command_scope(command: &CommandKind) -> LauncherCommandScope {
    use LauncherCommandScope::{CapabilityOnly, Repository};

    match command {
        CommandKind::Init(_)
        | CommandKind::Presets
        | CommandKind::Adopt(_)
        | CommandKind::Update(_)
        | CommandKind::Doctor
        | CommandKind::Codex(_)
        | CommandKind::Check(CheckCommand::Contract(_))
        | CommandKind::RuntimeCompatible(_) => CapabilityOnly,

        // Repository-scoped commands must operate exclusively on the
        // generated launcher's validated root. A command that accepts a
        // caller-relative repository target belongs in CapabilityOnly instead;
        // keeping this match exhaustive forces every new top-level command to
        // make that choice explicitly.
        CommandKind::Bootstrap(_)
        | CommandKind::Setup
        | CommandKind::Info(_)
        | CommandKind::Dev(_)
        | CommandKind::Check(_)
        | CommandKind::Status(_)
        | CommandKind::Ui(_)
        | CommandKind::Work(_)
        | CommandKind::Loop(_)
        | CommandKind::Sqlx(_)
        | CommandKind::MigrationAdd(_)
        | CommandKind::SchemaDump(_)
        | CommandKind::Vault(_)
        | CommandKind::GenerateSqlxUncheckedQueriesTodo(_)
        | CommandKind::Proxy(_)
        | CommandKind::Prompt(_)
        | CommandKind::Agent(_)
        | CommandKind::AgentMap(_)
        | CommandKind::State(_)
        | CommandKind::Mcp => Repository,
    }
}

#[cfg(test)]
fn launcher_capability_only_top_level_name(name: &str) -> bool {
    LAUNCHER_CAPABILITY_ONLY_SUBCOMMANDS
        .split(',')
        .any(|capability| capability == name)
}

const fn top_level_command_name(command: &CommandKind) -> &'static str {
    match command {
        CommandKind::Init(_) => tool_defs::cli_command::INIT,
        CommandKind::Presets => tool_defs::cli_command::PRESETS,
        CommandKind::Adopt(_) => tool_defs::cli_command::ADOPT,
        CommandKind::Update(_) => tool_defs::cli_command::UPDATE,
        CommandKind::Bootstrap(_) => tool_defs::cli_command::BOOTSTRAP,
        CommandKind::Setup => tool_defs::cli_command::SETUP,
        CommandKind::Doctor => tool_defs::cli_command::DOCTOR,
        CommandKind::Info(_) => tool_defs::cli_command::INFO,
        CommandKind::Dev(_) => tool_defs::cli_command::DEV,
        CommandKind::Check(_) => tool_defs::cli_command::CHECK,
        CommandKind::Status(_) => tool_defs::cli_command::STATUS,
        CommandKind::Ui(_) => tool_defs::cli_command::UI,
        CommandKind::Work(_) => tool_defs::cli_command::WORK,
        CommandKind::Loop(_) => tool_defs::cli_command::LOOP,
        CommandKind::Sqlx(_) => root_commands::SQLX.name,
        CommandKind::MigrationAdd(_) => tool_defs::cli_command::MIGRATION_ADD,
        CommandKind::SchemaDump(_) => tool_defs::cli_command::SCHEMA_DUMP,
        CommandKind::Vault(_) => tool_defs::cli_command::VAULT,
        CommandKind::GenerateSqlxUncheckedQueriesTodo(_) => {
            tool_defs::cli_command::GENERATE_SQLX_UNCHECKED_QUERIES_TODO
        }
        CommandKind::Proxy(_) => tool_defs::cli_command::PROXY,
        CommandKind::Prompt(_) => "prompt",
        CommandKind::Agent(_) => tool_defs::cli_command::AGENT,
        CommandKind::Codex(_) => tool_defs::cli_command::CODEX,
        CommandKind::AgentMap(_) => tool_defs::cli_command::AGENT_MAP,
        CommandKind::State(_) => tool_defs::cli_command::STATE,
        CommandKind::Mcp => tool_defs::cli_command::MCP,
        CommandKind::RuntimeCompatible(_) => "__runtime-compatible",
    }
}

const fn should_report_json_command_errors(json_output: bool, command: &CommandKind) -> bool {
    // MCP owns stdout as a framed protocol stream, while the hidden runtime
    // probe is itself a machine protocol. CLI JSON envelopes are invalid for
    // both, so their failures continue to use stderr.
    json_output
        && !matches!(
            command,
            CommandKind::Mcp | CommandKind::RuntimeCompatible(_)
        )
}

fn run_command(cli: Cli) -> Result<()> {
    let json_output = cli.json;
    match cli.command {
        CommandKind::RuntimeCompatible(opts) => run_runtime_compatible(opts),
        CommandKind::Init(opts) => run_init_command(opts, json_output),
        CommandKind::Presets => run_presets_command(json_output),
        CommandKind::Adopt(opts) => run_adopt_command(opts, json_output),
        CommandKind::Update(opts) => run_update_command(opts, json_output),
        CommandKind::Mcp => {
            let ctx = RepoContext::load()?;
            mcp::serve(&ctx)
        }
        CommandKind::Ui(opts) => {
            let ctx = RepoContext::load()?;
            ui::serve(&ctx, opts, json_output)
        }
        CommandKind::Doctor => {
            let output = doctor::run()?;
            emit(json_output, HumanOutput::Doctor, &output)?;
            finish_after_json_output(require_json_ok(true, &output), json_output)
        }
        CommandKind::Info(opts) => {
            let output = info::run(opts.commands, json_output)?;
            emit(json_output, HumanOutput::Info, &output)?;
            finish_after_json_output(require_json_ok(true, &output), json_output)
        }
        CommandKind::Status(opts) => {
            let ctx = RepoContext::load()?;
            if opts.tui {
                return status::tui::run(
                    ctx,
                    std::time::Duration::from_secs(opts.effective_refresh_seconds()),
                );
            }
            let output = status::snapshot(&ctx)?;
            emit(json_output, HumanOutput::Status, &output)
        }
        #[cfg(not(feature = "dev-proxy"))]
        CommandKind::Dev(opts) => {
            let human_output = dev_human_output(&opts);
            let output = crate::dev_proxy::commands::dev_without_context(opts.into())?;
            emit(json_output, human_output, &output)?;
            finish_after_json_output(require_foreground_status(&output), json_output)
        }
        #[cfg(feature = "dev-proxy")]
        CommandKind::Dev(opts) => {
            let human_output = dev_human_output(&opts);
            let Some(ctx) = RepoContext::load_optional()? else {
                anyhow::bail!(
                    "`scripts/jig dev` requires an adopted Jig repo with `.jig.toml`. Run it from a Jig repo, or preview adoption with `scripts/jig adopt .` and apply it with `scripts/jig adopt . --write`."
                );
            };
            if let Some(identity_present) = dev_launch_identity_present(&opts) {
                ensure_dev_process_identity(&ctx, identity_present);
            }
            let output = runtime::dispatch(&ctx, crate::command::RuntimeCommand::Dev(opts.into()))?;
            emit(json_output, human_output, &output)?;
            finish_after_json_output(require_foreground_status(&output), json_output)
        }
        #[cfg(not(feature = "dev-proxy"))]
        CommandKind::Proxy(command) => {
            let output = crate::dev_proxy::commands::proxy_without_context(command.into())?;
            emit(json_output, HumanOutput::Proxy, &output)?;
            finish_after_json_output(require_foreground_status(&output), json_output)
        }
        #[cfg(feature = "dev-proxy")]
        CommandKind::Proxy(command) => {
            let runtime_command: crate::command::ProxyCommand = command.into();
            let output = if crate::dev_proxy::commands::can_run_without_context(&runtime_command) {
                if let Some(ctx) = RepoContext::load_optional()? {
                    runtime::dispatch(&ctx, crate::command::RuntimeCommand::Proxy(runtime_command))?
                } else {
                    crate::dev_proxy::commands::proxy_without_context(runtime_command)?
                }
            } else {
                let ctx = RepoContext::load()?;
                runtime::dispatch(&ctx, crate::command::RuntimeCommand::Proxy(runtime_command))?
            };
            emit(json_output, HumanOutput::Proxy, &output)?;
            finish_after_json_output(require_foreground_status(&output), json_output)
        }
        CommandKind::Bootstrap(opts) => dispatch_runtime_command(
            crate::command::RuntimeCommand::Bootstrap(opts.into()),
            false,
            json_output,
            HumanOutput::ToolExecution,
        ),
        CommandKind::Setup => run_setup_command(json_output),
        CommandKind::Check(command) => {
            let require_ok = check_command_reports_failure_with_ok(&command);
            let human_output = check_human_output(&command);
            dispatch_runtime_command(
                crate::command::RuntimeCommand::Check(command.into()),
                require_ok,
                json_output,
                human_output,
            )
        }
        CommandKind::Sqlx(command) => run_sqlx_command(command, json_output),
        CommandKind::SchemaDump(opts) => dispatch_runtime_command(
            crate::command::RuntimeCommand::Sqlx(crate::command::SqlxCommand::SchemaDump(
                opts.into(),
            )),
            false,
            json_output,
            HumanOutput::ToolExecution,
        ),
        CommandKind::MigrationAdd(opts) => dispatch_runtime_command(
            crate::command::RuntimeCommand::Sqlx(crate::command::SqlxCommand::MigrationAdd(
                opts.into(),
            )),
            false,
            json_output,
            HumanOutput::MigrationAdd,
        ),
        CommandKind::AgentMap(command) => dispatch_runtime_command(
            crate::command::RuntimeCommand::AgentMap(command.into()),
            false,
            json_output,
            HumanOutput::AgentMapGenerate,
        ),
        CommandKind::GenerateSqlxUncheckedQueriesTodo(opts) => dispatch_runtime_command(
            crate::command::RuntimeCommand::GenerateSqlxUncheckedQueriesTodo(opts.into()),
            false,
            json_output,
            HumanOutput::ToolExecution,
        ),
        CommandKind::Vault(command) => run_vault_command(command, json_output),
        CommandKind::Prompt(command) => run_prompt_command(command, json_output),
        CommandKind::Agent(command) => {
            let require_ok = agent_command_reports_failure_with_ok(&command);
            let human_output = agent_human_output(&command);
            dispatch_runtime_command(
                crate::command::RuntimeCommand::Agent(command.into()),
                require_ok,
                json_output,
                human_output,
            )
        }
        CommandKind::Codex(command) => run_codex_command(command, json_output),
        CommandKind::Work(command) => {
            let human_output = work_human_output(&command);
            dispatch_runtime_command(
                crate::command::RuntimeCommand::Work(command.into()),
                false,
                json_output,
                human_output,
            )
        }
        CommandKind::Loop(command) => {
            let human_output = loop_human_output(&command);
            dispatch_runtime_command(
                crate::command::RuntimeCommand::Loop(command.into()),
                false,
                json_output,
                human_output,
            )
        }
        CommandKind::State(command) => {
            let human_output = state_human_output(&command);
            dispatch_runtime_command(
                crate::command::RuntimeCommand::State(command.into()),
                false,
                json_output,
                human_output,
            )
        }
    }
}

fn run_sqlx_command(command: SqlxCommand, json_output: bool) -> Result<()> {
    let human_output = match &command {
        SqlxCommand::Migration(SqlxMigrationCommand::Add(_)) => HumanOutput::MigrationAdd,
        SqlxCommand::Schema(SqlxSchemaCommand::Dump(_)) => HumanOutput::ToolExecution,
    };
    dispatch_runtime_command(
        crate::command::RuntimeCommand::Sqlx(command.into()),
        false,
        json_output,
        human_output,
    )
}

fn run_runtime_compatible(opts: RuntimeCompatibleOpts) -> Result<()> {
    validate_runtime_compatibility(
        &opts.repo_root,
        opts.contract_version,
        opts.profile,
        opts.capability_only,
    )?;
    Ok(())
}

fn validate_runtime_compatibility(
    repo_root: &std::path::Path,
    contract_version: Option<u32>,
    profile: RuntimeCompatibilityProfile,
    capability_only: bool,
) -> Result<Option<RepoContext>> {
    let repo_root = std::fs::canonicalize(repo_root).with_context(|| {
        format!(
            "Failed to resolve Jig repository root {}",
            repo_root.display()
        )
    })?;
    if let Some(contract_version) = contract_version
        && !crate::context::is_supported_contract_version(contract_version)
    {
        bail!(
            "Unsupported Jig contract version {contract_version}; this runtime supports versions {} through {}",
            crate::context::MIN_SUPPORTED_CONTRACT_VERSION,
            crate::context::CURRENT_CONTRACT_VERSION
        );
    }
    let ctx = if capability_only {
        if contract_version.is_none() {
            // Keep direct/manual uses of the private probe useful. Generated
            // launchers and installers pass their rendered epoch explicitly so
            // repair paths do not depend on a readable manifest.
            RepoContext::supported_contract_version_from_root(&repo_root)?;
        }
        None
    } else {
        if let Some(launcher_contract_version) = contract_version {
            let repository_contract_version =
                RepoContext::declared_contract_version_from_root(&repo_root)?;
            if launcher_contract_version != repository_contract_version {
                bail!(
                    "Launcher contract version {launcher_contract_version} does not match repository contract version {repository_contract_version}"
                );
            }
        }
        let ctx = RepoContext::load_from_root(repo_root)?;
        crate::policy::validate_contract(&ctx)?;
        Some(ctx)
    };
    if profile == RuntimeCompatibilityProfile::Default && !cfg!(feature = "dev-proxy") {
        bail!(
            "This Jig binary is incompatible with the default runtime profile because it was built without the dev-proxy feature"
        );
    }
    Ok(ctx)
}

fn report_json_command_error(result: Result<()>) -> Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(error) if is_structured_json_failure(&error) => Err(error),
        Err(error) if is_json_output_already_emitted(&error) => Err(error),
        Err(error) => {
            print_json(&json_error_payload(
                "command_failed",
                &format!("{error:#}"),
                1,
            ))?;
            Err(json_reported_error(1))
        }
    }
}

pub(super) fn finish_after_json_output(result: Result<()>, json_output: bool) -> Result<()> {
    match result {
        Err(error) if json_output && !is_structured_json_failure(&error) => {
            Err(json_output_already_emitted(error))
        }
        result => result,
    }
}

const fn dev_human_output(opts: &DevOpts) -> HumanOutput {
    match &opts.command {
        None => HumanOutput::Dev,
        Some(DevSubcommand::Status(_)) => HumanOutput::DevStatus,
        Some(DevSubcommand::Stop(_)) => HumanOutput::DevStop,
    }
}

#[cfg_attr(not(feature = "dev-proxy"), allow(dead_code))]
fn dev_launch_identity_present(opts: &DevOpts) -> Option<bool> {
    opts.command
        .is_none()
        .then_some(opts.launch.jig_project.is_some())
}

#[cfg(feature = "dev-proxy")]
fn ensure_dev_process_identity(ctx: &RepoContext, identity_present: bool) {
    if identity_present {
        return;
    }

    #[cfg(unix)]
    {
        let error = exec_dev_with_process_identity(ctx);
        eprintln!("jig warning: could not add the project identity to this dev process: {error}");
    }

    #[cfg(not(unix))]
    let _ = ctx;
}

#[cfg(all(feature = "dev-proxy", unix))]
fn exec_dev_with_process_identity(ctx: &RepoContext) -> std::io::Error {
    use std::ffi::{OsStr, OsString};
    use std::os::unix::process::CommandExt;

    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(error) => return error,
    };
    let mut args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let Some(dev_index) = args
        .iter()
        .position(|arg| arg == OsStr::new(tool_defs::cli_command::DEV))
    else {
        return std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "parsed dev command was missing from the process arguments",
        );
    };
    let mut identity = OsString::from("--jig-project=");
    identity.push(ctx.repo_name());
    identity.push("@");
    identity.push(ctx.root());
    args.insert(dev_index + 1, identity);

    let mut command = process::Command::new(executable);
    command.arg0("jig").args(args);
    command.exec()
}

#[cfg(test)]
pub(super) const fn test_command_reports_failure_with_ok(command: &CommandKind) -> bool {
    // Proxy commands expose host-cleanup/status operations that can complete
    // with `ok: false` in their JSON payload. Multi-app `jig dev` also uses
    // `ok: false` when the first child exits unsuccessfully. Agent doctor is a
    // readiness report and returns `ok: false` when required local tooling is
    // missing or unregistered.
    match command {
        CommandKind::Doctor | CommandKind::Dev(_) | CommandKind::Proxy(_) => true,
        CommandKind::Vault(command) => matches!(command, VaultCommand::Run(_)),
        CommandKind::Agent(command) => agent_command_reports_failure_with_ok(command),
        CommandKind::Check(command) => check_command_reports_failure_with_ok(command),
        _ => false,
    }
}

const fn agent_command_reports_failure_with_ok(command: &AgentCommand) -> bool {
    matches!(command, AgentCommand::Doctor)
}

const fn check_command_reports_failure_with_ok(command: &CheckCommand) -> bool {
    matches!(
        command,
        CheckCommand::AgentMap(_)
            | CheckCommand::AgentGuides
            | CheckCommand::RustFileLoc(_)
            | CheckCommand::NoModRs
            | CheckCommand::MigrationImmutability(_)
            | CheckCommand::SqlxUncheckedNonTest,
    )
}

const fn check_human_output(_command: &CheckCommand) -> HumanOutput {
    HumanOutput::ToolExecution
}

const fn agent_human_output(command: &AgentCommand) -> HumanOutput {
    match command {
        AgentCommand::Doctor => HumanOutput::AgentDoctor,
        AgentCommand::Bootstrap(_) => HumanOutput::AgentBootstrap,
    }
}

const fn work_human_output(command: &WorkCommand) -> HumanOutput {
    match command {
        WorkCommand::Start(opts) if opts.print_plan_id => HumanOutput::WorkStartPlanId,
        WorkCommand::Start(_) => HumanOutput::WorkStart,
        WorkCommand::Goal(_) => HumanOutput::WorkGoal,
        WorkCommand::Append(_) => HumanOutput::WorkAppend,
        WorkCommand::Check(_) => HumanOutput::WorkCheck,
        WorkCommand::Gates(_) => HumanOutput::WorkGates,
        WorkCommand::Evidence(_) => HumanOutput::WorkEvidence,
        WorkCommand::Review(_) => HumanOutput::WorkReview,
        WorkCommand::Refine(_) => HumanOutput::WorkRefine,
        WorkCommand::Decide(_) => HumanOutput::WorkDecide,
        WorkCommand::Receipts(_) => HumanOutput::WorkReceipts,
        WorkCommand::Status => HumanOutput::WorkStatus,
        WorkCommand::Finish(_) => HumanOutput::WorkFinish,
    }
}

const fn loop_human_output(command: &LoopCommand) -> HumanOutput {
    match command {
        LoopCommand::Tick(_) => HumanOutput::LoopTick,
        LoopCommand::Status(_) => HumanOutput::LoopStatus,
        LoopCommand::Run(_) => HumanOutput::LoopRun,
        LoopCommand::ClearAttempt(_) => HumanOutput::LoopClearAttempt,
    }
}

const fn state_human_output(command: &StateCommand) -> HumanOutput {
    match command {
        StateCommand::Summary => HumanOutput::StateSummary,
        StateCommand::Diagnose(_) => HumanOutput::StateDiagnose,
        StateCommand::Compact { .. } => HumanOutput::StateCompact,
        StateCommand::Restore(_) => HumanOutput::StateRestore,
        StateCommand::Export { .. } => HumanOutput::StateExport,
        StateCommand::Archive(_) => HumanOutput::StateArchive,
    }
}

fn dispatch_runtime_command(
    command: crate::command::RuntimeCommand,
    require_ok: bool,
    json_output: bool,
    human_output: HumanOutput,
) -> Result<()> {
    let ctx = RepoContext::load()?;
    let output = runtime::dispatch(&ctx, command)?;
    emit(json_output, human_output, &output)?;
    finish_after_json_output(require_json_ok(require_ok, &output), json_output)
}

fn parse_cli() -> Cli {
    let args = std::env::args_os().collect::<Vec<_>>();
    let command_args = || args.iter().skip(1).cloned();
    let report_json_errors = args_request_json(command_args()) && !args_target_mcp(command_args());

    match Cli::try_parse_from(args) {
        Ok(cli) => {
            if let Some(error) = post_parse_usage_error(&cli) {
                exit_with_cli_error(error, report_json_errors);
            }
            cli
        }
        Err(error) => exit_with_cli_error(error, report_json_errors),
    }
}

fn post_parse_usage_error(cli: &Cli) -> Option<clap::Error> {
    let message = match &cli.command {
        CommandKind::Status(opts) if cli.json && opts.tui => {
            "`--tui` cannot be combined with `--json`"
        }
        CommandKind::Work(WorkCommand::Start(opts)) if cli.json && opts.print_plan_id => {
            "`--print-plan-id` cannot be combined with `--json`"
        }
        _ => return None,
    };
    Some(clap::Error::raw(ErrorKind::ArgumentConflict, message))
}

fn exit_with_cli_error(error: clap::Error, json_output: bool) -> ! {
    if json_output
        && !matches!(
            error.kind(),
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
        )
    {
        let exit_status = error.exit_code();
        let message = augmented_cli_error_message(&error);
        let _ = print_json(&json_error_payload("usage", &message, exit_status));
        process::exit(exit_status);
    }

    if should_add_template_hint(&error) {
        let message = error.to_string();
        // If stderr is closed, there is nowhere useful to report the parse hint.
        let _ = writeln!(std::io::stderr(), "{message}\n{TEMPLATE_ERROR_HINT}");
        process::exit(error.exit_code());
    }

    if let Some(hint) = moved_check_command_hint(&error) {
        let message = error.to_string();
        // If stderr is closed, there is nowhere useful to report the parse hint.
        let _ = writeln!(std::io::stderr(), "{message}\n{hint}");
        process::exit(error.exit_code());
    }

    if let Some(hint) = missing_init_path_hint(&error) {
        let message = error.to_string();
        // If stderr is closed, there is nowhere useful to report the parse hint.
        let _ = writeln!(std::io::stderr(), "{message}\n{hint}");
        process::exit(error.exit_code());
    }

    error.exit();
}

fn args_request_json(args: impl IntoIterator<Item = OsString>) -> bool {
    args.into_iter()
        .take_while(|arg| arg != "--")
        .any(|arg| arg == "--json")
}

fn args_target_mcp(args: impl IntoIterator<Item = OsString>) -> bool {
    args.into_iter()
        .take_while(|arg| arg != "--")
        .filter(|arg| arg != "--json")
        .find(|arg| !arg.to_string_lossy().starts_with('-'))
        .is_some_and(|arg| arg == tool_defs::cli_command::MCP)
}

fn augmented_cli_error_message(error: &clap::Error) -> String {
    let mut message = error.to_string();
    let hint = if should_add_template_hint(error) {
        Some(TEMPLATE_ERROR_HINT.to_string())
    } else if let Some(hint) = moved_check_command_hint(error) {
        Some(hint)
    } else {
        missing_init_path_hint(error).map(str::to_string)
    };
    if let Some(hint) = hint {
        message.push('\n');
        message.push_str(&hint);
    }
    message
}

fn missing_init_path_hint(error: &clap::Error) -> Option<&'static str> {
    if error.kind() != ErrorKind::MissingRequiredArgument {
        return None;
    }

    if !error.context().any(|(kind, value)| {
        kind == ContextKind::Usage && context_contains(value, "jig init <PATH>")
    }) {
        return None;
    }

    Some(
        "\
`jig init` creates a new Jig-managed repository.
Use `jig adopt .` for an existing repository.

Use one of:
  jig init /path/to/new-repo --preset harness-only --repo-name new-repo --sqlx-enabled false --no-input --no-vault
  jig init /path/to/new-repo --preset rust-react
  jig init /path/to/new-repo --preset rust-react --db postgres --frontends web,landing,admin
  jig adopt .              # preview Jig adoption for this existing repo
  jig adopt . --write      # apply Jig adoption to this existing repo
  jig presets              # list available project scaffolds",
    )
}

pub(super) fn moved_check_command_hint(error: &clap::Error) -> Option<String> {
    if error.kind() != ErrorKind::InvalidSubcommand {
        return None;
    }

    let message = error.to_string();
    let moved = [
        ("fmt-check", "jig check fmt"),
        ("clippy", "jig check clippy"),
        ("test", "jig check test"),
        ("test-locked", "jig check test-locked"),
        ("sqlx-check", "jig check sqlx"),
        ("schema-check", "jig check schema"),
        ("contract-check", "jig check contract"),
        ("check-agent-guides", "jig check agent-guides"),
        ("check-rust-file-loc", "jig check rust-file-loc"),
        ("check-no-mod-rs", "jig check no-mod-rs"),
        (
            "check-migration-immutability",
            "jig check migration-immutability",
        ),
        (
            "check-sqlx-unchecked-non-test",
            "jig check sqlx-unchecked-non-test",
        ),
    ];

    // Like the nested agent-map case below, this depends on Clap 4.6.1 formatted
    // usage text and is only a best-effort migration hint. Global options such as
    // --json make the top-level usage line include [OPTIONS]; recheck this matcher
    // on Clap upgrades or when adding more global flags.
    if message.contains("Usage: jig [OPTIONS] <COMMAND>") {
        if let Some((_, replacement)) = moved
            .iter()
            .find(|(legacy, _)| message.contains(&format!("'{legacy}'")))
        {
            return Some(moved_check_hint_for(replacement));
        }
    }

    // Clap 4.6.1 reports nested invalid subcommands through formatted usage text;
    // this hint is best-effort and may disappear if that formatting changes.
    if message.contains("unrecognized subcommand 'check'")
        && message.contains("Usage: jig agent-map [OPTIONS] <COMMAND>")
    {
        return Some(moved_check_hint_for("jig check agent-map"));
    }

    None
}

fn moved_check_hint_for(replacement: &str) -> String {
    format!("This check command moved. Use:\n  {replacement}")
}

pub(super) fn should_add_template_hint(error: &clap::Error) -> bool {
    if !matches!(
        error.kind(),
        ErrorKind::InvalidValue | ErrorKind::TooFewValues
    ) {
        return false;
    }
    error
        .context()
        .any(|(kind, value)| kind == ContextKind::InvalidArg && context_mentions_template(value))
}

fn context_contains(value: &ContextValue, needle: &str) -> bool {
    match value {
        ContextValue::String(value) => value.contains(needle),
        ContextValue::Strings(values) => values.iter().any(|value| value.contains(needle)),
        ContextValue::StyledStr(value) => value.to_string().contains(needle),
        ContextValue::StyledStrs(values) => values
            .iter()
            .any(|value| value.to_string().contains(needle)),
        _ => false,
    }
}

fn context_mentions_template(value: &ContextValue) -> bool {
    match value {
        ContextValue::String(value) => is_template_arg(value),
        ContextValue::Strings(values) => values.iter().any(|value| is_template_arg(value)),
        ContextValue::StyledStr(value) => is_template_arg(&value.to_string()),
        ContextValue::StyledStrs(values) => values
            .iter()
            .any(|value| is_template_arg(&value.to_string())),
        _ => false,
    }
}

fn is_template_arg(value: &str) -> bool {
    value
        .split_whitespace()
        .next()
        .is_some_and(|arg| arg == "--template")
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
