use std::ffi::OsString;
use std::io::Write;
use std::process;

use anyhow::Result;
use clap::{
    Parser,
    error::{ContextKind, ContextValue, ErrorKind},
};

use super::bootstrap_run::{
    run_adopt_command, run_init_command, run_presets_command, run_update_command,
};
use super::output::{HumanOutput, emit, print_json};
use super::prompt_run::run_prompt_command;
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
    let result = run_command(cli);
    if report_json_errors {
        return report_json_command_error(result);
    }
    result
}

const fn should_report_json_command_errors(json_output: bool, command: &CommandKind) -> bool {
    // MCP owns stdout as a framed protocol stream. A CLI error envelope is not
    // a valid MCP message, so its failures continue to use stderr.
    json_output && !matches!(command, CommandKind::Mcp)
}

fn run_command(cli: Cli) -> Result<()> {
    let json_output = cli.json;
    match cli.command {
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
        CommandKind::Info => {
            let output = info::run()?;
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
                    "`scripts/jig dev` requires an adopted Jig repo with `.jig.toml`. Run it from a Jig repo, or use `scripts/jig proxy run <name> -- <command>` for an ad-hoc command."
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
        CommandKind::SchemaDump(opts) => dispatch_runtime_command(
            crate::command::RuntimeCommand::SchemaDump(opts.into()),
            false,
            json_output,
            HumanOutput::ToolExecution,
        ),
        CommandKind::MigrationAdd(opts) => dispatch_runtime_command(
            crate::command::RuntimeCommand::MigrationAdd(opts.into()),
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
