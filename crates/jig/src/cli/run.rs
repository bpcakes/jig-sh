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
use super::output::{HumanOutput, print_json, print_output};
use super::prompt_run::run_prompt_command;
use super::structured_error::require_json_ok;
pub(crate) use super::structured_error::{is_structured_json_failure, structured_error_exit_code};
use super::vault_run::run_vault_command;
use super::*;

pub(crate) fn run() -> Result<()> {
    let cli = parse_cli();
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
        CommandKind::Doctor(opts) => {
            let output = doctor::run()?;
            print_output(opts.summary.then_some(HumanOutput::DoctorSummary), &output)?;
            require_json_ok(true, &output)
        }
        CommandKind::Info(opts) => {
            let output = info::run()?;
            print_output(opts.summary.then_some(HumanOutput::InfoSummary), &output)?;
            require_json_ok(true, &output)
        }
        #[cfg(not(feature = "dev-proxy"))]
        CommandKind::Dev(opts) => {
            let output = crate::dev_proxy::commands::dev_without_context(opts.into())?;
            print_json(&output)?;
            require_json_ok(true, &output)
        }
        #[cfg(feature = "dev-proxy")]
        CommandKind::Dev(opts) => {
            let Some(ctx) = RepoContext::load_optional()? else {
                anyhow::bail!(
                    "`scripts/jig dev` requires an adopted Jig repo with `.jig.toml` dev app configuration. Run it from a Jig repo, or use `scripts/jig proxy run <name> -- <command>` for an ad-hoc command."
                );
            };
            let output = runtime::dispatch(&ctx, crate::command::RuntimeCommand::Dev(opts.into()))?;
            print_json(&output)?;
            require_json_ok(true, &output)
        }
        #[cfg(not(feature = "dev-proxy"))]
        CommandKind::Proxy(command) => {
            let output = crate::dev_proxy::commands::proxy_without_context(command.into())?;
            print_json(&output)?;
            require_json_ok(true, &output)
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
            print_json(&output)?;
            require_json_ok(true, &output)
        }
        CommandKind::Bootstrap(opts) => dispatch_runtime_command(
            crate::command::RuntimeCommand::Bootstrap(opts.into()),
            false,
            None,
        ),
        CommandKind::Check(command) => {
            let require_ok = check_command_reports_failure_with_ok(&command);
            dispatch_runtime_command(
                crate::command::RuntimeCommand::Check(command.into()),
                require_ok,
                None,
            )
        }
        CommandKind::SchemaDump(opts) => dispatch_runtime_command(
            crate::command::RuntimeCommand::SchemaDump(opts.into()),
            false,
            None,
        ),
        CommandKind::MigrationAdd(opts) => dispatch_runtime_command(
            crate::command::RuntimeCommand::MigrationAdd(opts.into()),
            false,
            None,
        ),
        CommandKind::AgentMap(command) => dispatch_runtime_command(
            crate::command::RuntimeCommand::AgentMap(command.into()),
            false,
            None,
        ),
        CommandKind::GenerateSqlxUncheckedQueriesTodo(opts) => dispatch_runtime_command(
            crate::command::RuntimeCommand::GenerateSqlxUncheckedQueriesTodo(opts.into()),
            false,
            None,
        ),
        CommandKind::Vault(command) => run_vault_command(command),
        CommandKind::Prompt(command) => run_prompt_command(command, json_output),
        CommandKind::Agent(command) => {
            let require_ok = agent_command_reports_failure_with_ok(&command);
            let human_output = agent_human_output_requested(&command);
            dispatch_runtime_command(
                crate::command::RuntimeCommand::Agent(command.into()),
                require_ok,
                human_output,
            )
        }
        CommandKind::Work(command) => {
            let human_output = work_human_output_requested(&command);
            dispatch_runtime_command(
                crate::command::RuntimeCommand::Work(command.into()),
                false,
                human_output,
            )
        }
        CommandKind::Loop(command) => dispatch_runtime_command(
            crate::command::RuntimeCommand::Loop(command.into()),
            false,
            None,
        ),
        CommandKind::State(command) => dispatch_runtime_command(
            crate::command::RuntimeCommand::State(command.into()),
            false,
            None,
        ),
    }
}

#[cfg(test)]
pub(super) fn test_command_reports_failure_with_ok(command: &CommandKind) -> bool {
    // Proxy commands expose host-cleanup/status operations that can complete
    // with `ok: false` in their JSON payload. Multi-app `jig dev` also uses
    // `ok: false` when the first child exits unsuccessfully. Agent doctor is a
    // readiness report and returns `ok: false` when required local tooling is
    // missing or unregistered.
    match command {
        CommandKind::Doctor(_) | CommandKind::Dev(_) | CommandKind::Proxy(_) => true,
        CommandKind::Vault(command) => matches!(command, VaultCommand::Run(_)),
        CommandKind::Agent(command) => agent_command_reports_failure_with_ok(command),
        CommandKind::Check(command) => check_command_reports_failure_with_ok(command),
        _ => false,
    }
}

fn agent_command_reports_failure_with_ok(command: &AgentCommand) -> bool {
    matches!(command, AgentCommand::Doctor(_))
}

fn check_command_reports_failure_with_ok(command: &CheckCommand) -> bool {
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

fn agent_human_output_requested(command: &AgentCommand) -> Option<HumanOutput> {
    match command {
        AgentCommand::Doctor(opts) if opts.summary => Some(HumanOutput::AgentDoctorSummary),
        _ => None,
    }
}

fn work_human_output_requested(command: &WorkCommand) -> Option<HumanOutput> {
    match command {
        WorkCommand::Start(opts) if opts.print_plan_id => Some(HumanOutput::WorkStartPlanId),
        WorkCommand::Check(opts) if opts.summary => Some(HumanOutput::WorkCheckSummary),
        WorkCommand::Gates(opts) if opts.summary => Some(HumanOutput::WorkGatesSummary),
        WorkCommand::Evidence(opts) if opts.summary => Some(HumanOutput::WorkEvidenceSummary),
        WorkCommand::Review(opts) if opts.summary => Some(HumanOutput::WorkReviewSummary),
        WorkCommand::Refine(opts) if opts.summary => Some(HumanOutput::WorkRefineSummary),
        WorkCommand::Receipts(opts) if opts.summary => Some(HumanOutput::WorkReceiptsSummary),
        WorkCommand::Status(opts) if opts.summary => Some(HumanOutput::WorkStatusSummary),
        _ => None,
    }
}

fn dispatch_runtime_command(
    command: crate::command::RuntimeCommand,
    require_ok: bool,
    human_output: Option<HumanOutput>,
) -> Result<()> {
    let ctx = RepoContext::load()?;
    let output = runtime::dispatch(&ctx, command)?;
    print_output(human_output, &output)?;
    require_json_ok(require_ok, &output)
}

fn parse_cli() -> Cli {
    match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => exit_with_cli_error(error),
    }
}

fn exit_with_cli_error(error: clap::Error) -> ! {
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
  jig init /path/to/new-repo --repo-name new-repo --sqlx-enabled false
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
