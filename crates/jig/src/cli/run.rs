use anyhow::Result;

use super::bootstrap_run::{
    run_adopt_command, run_init_command, run_presets_command, run_update_command,
};
use super::codex_run::run_codex_command;
use super::output::{self, emit, print_json};
use super::setup_run::run_setup_command;
use super::structured_error::{
    is_json_output_already_emitted, json_error_payload, json_output_already_emitted,
    json_reported_error, require_json_ok,
};
pub(crate) use super::structured_error::{is_structured_json_failure, structured_error_exit_code};
use super::ui_run::{name_ui_error, run_ui_command};
use super::vault_run::run_vault_command;
use super::{Cli, CommandKind};
use crate::cli::runtime_dispatch::{RuntimeDispatch, dispatch_runtime};
use crate::command::RuntimeCommand;
use crate::doctor;

pub(crate) fn run() -> Result<()> {
    let cli = parse_cli();
    // Invariant: before launcher validation, repository loading, worker
    // threads, or any child process.
    enforce_vault_passphrase_startup_boundary(&cli.command);
    let json_output = cli.json;
    let report_json_errors = should_report_json_command_errors(json_output, &cli.command);
    let name_ui_errors = json_output && matches!(cli.command, CommandKind::Ui(_));
    let result = validate_launcher_repository_scope(&cli)
        .map_err(|error| {
            if name_ui_errors {
                name_ui_error(error)
            } else {
                error
            }
        })
        .and_then(|()| run_command(cli));
    if report_json_errors {
        return report_json_command_error(result);
    }
    result
}

const fn should_report_json_command_errors(json_output: bool, command: &CommandKind) -> bool {
    // The hidden runtime probe is a machine protocol, so its failures use stderr.
    json_output && !matches!(command, CommandKind::RuntimeCompatible(_))
}

fn run_command(cli: Cli) -> Result<()> {
    let json_output = cli.json;
    match cli.command {
        CommandKind::RuntimeCompatible(opts) => run_runtime_compatible(opts),
        CommandKind::Init(opts) => run_init_command(opts, json_output),
        CommandKind::Presets => run_presets_command(json_output),
        CommandKind::Adopt(opts) => run_adopt_command(opts, json_output),
        CommandKind::Update(opts) => run_update_command(opts, json_output),
        CommandKind::Ui(opts) => run_ui_command(opts, json_output),
        CommandKind::Doctor => {
            let output = doctor::run()?;
            emit(json_output, output::format_doctor_summary, &output)?;
            finish_after_json_output(require_json_ok(true, &output), json_output)
        }
        CommandKind::Info(opts) => super::info_run::run_info_command(opts, json_output),
        CommandKind::Status(opts) => super::status_run::run_status_command(opts, json_output),
        CommandKind::Dev(opts) => run_dev_command(opts, json_output),
        CommandKind::Proxy(command) => run_proxy_command(command, json_output),
        CommandKind::Bootstrap => dispatch_runtime(
            RuntimeDispatch::tool(RuntimeCommand::Bootstrap),
            json_output,
        ),
        CommandKind::Setup => run_setup_command(json_output),
        CommandKind::Check(opts) => dispatch_runtime(opts.into_dispatch()?, json_output),
        CommandKind::Run(opts) => dispatch_runtime(opts.into_dispatch()?, json_output),
        CommandKind::FileBudget(command) => {
            super::file_budget::run_file_budget_command(command, json_output)
        }
        CommandKind::Migration(command) => dispatch_runtime(command.into_dispatch(), json_output),
        CommandKind::MigrationAdd(opts) => dispatch_runtime(opts.into_dispatch(), json_output),
        CommandKind::Sqlx(command) => dispatch_runtime(command.into_dispatch(), json_output),
        CommandKind::SchemaDump => {
            dispatch_runtime(super::sqlx::schema_dump_dispatch(), json_output)
        }
        CommandKind::AgentMap(command) => dispatch_runtime(command.into_dispatch(), json_output),
        CommandKind::GenerateSqlxUncheckedQueriesTodo(opts) => {
            dispatch_runtime(opts.into_dispatch(), json_output)
        }
        CommandKind::Vault(command) => run_vault_command(command, json_output),
        CommandKind::Agent(command) => dispatch_runtime(command.into_dispatch(), json_output),
        CommandKind::Claude(command) => super::claude_run::run_claude_command(command, json_output),
        CommandKind::Codex(command) => run_codex_command(command, json_output),
        // Argument parsing rejects the retired namespace before dispatch.
        CommandKind::Loop(command) => dispatch_runtime(command.into_dispatch(), json_output),
        CommandKind::State(command) => dispatch_runtime(command.into_dispatch(), json_output),
    }
}

fn report_json_command_error(result: Result<()>) -> Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(error) if is_structured_json_failure(&error) => Err(error),
        Err(error) if is_json_output_already_emitted(&error) => Err(error),
        Err(error) if error.is::<super::structured_error::JsonCommandError>() => {
            let named = error
                .downcast_ref::<super::structured_error::JsonCommandError>()
                .expect("checked named JSON command error");
            let mut payload = json_error_payload("command_failed", &named.to_string(), 1);
            payload["command"] = serde_json::json!(named.command);
            print_json(&payload)?;
            Err(json_reported_error(1))
        }
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

mod argument_parsing;
mod launcher_handoff;
mod vault_environment;
mod workflow_recovery;
use argument_parsing::parse_cli;
#[cfg(test)]
pub(super) use argument_parsing::post_parse_usage_error;
use launcher_handoff::{run_runtime_compatible, validate_launcher_repository_scope};
use vault_environment::enforce_vault_passphrase_startup_boundary;
// `jig dev` and `jig proxy` have one implementation per build: the real one,
// or a stub reporting that the `dev-proxy` feature is not built.
#[cfg(feature = "dev-proxy")]
mod dev_launch;
#[cfg(feature = "dev-proxy")]
use dev_launch::{run_dev_command, run_proxy_command};
#[cfg(not(feature = "dev-proxy"))]
mod dev_unavailable;
#[cfg(not(feature = "dev-proxy"))]
use dev_unavailable::{run_dev_command, run_proxy_command};

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
