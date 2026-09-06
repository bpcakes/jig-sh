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
    let name_ui_errors = json_output && matches!(cli.command, CommandKind::Ui(_));
    let result = validate_launcher_repository_scope(&cli)
        .map_err(|error| {
            if name_ui_errors {
                super::json_command_error("ui", error)
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

fn validate_launcher_repository_scope(cli: &Cli) -> Result<()> {
    let LauncherHandoff::Repository(request) = LauncherHandoff::from_cli(cli)? else {
        return Ok(());
    };
    let descriptor = cli.command.launcher_descriptor();
    if descriptor.scope == LauncherCommandScope::CapabilityOnly {
        bail!(
            "The generated launcher and this Jig runtime disagree about whether `{}` is repository-scoped. Repair the launcher/runtime pair with a current external Jig binary (`jig update <repo> --launcher-only --force`) before retrying `{}`.",
            descriptor.name,
            descriptor.name,
        );
    }
    let ctx = validate_repository_runtime_compatibility(request).with_context(|| {
        format!(
            "The repository contract did not validate under Jig profile {}. Run scripts/jig check contract or scripts/jig doctor for repair guidance.",
            request.profile.as_str()
        )
    })?;
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

#[derive(Clone, Copy, Debug)]
enum LauncherHandoff<'a> {
    Direct,
    Repository(RuntimeCompatibilityRequest<'a>),
}

impl<'a> LauncherHandoff<'a> {
    fn from_cli(cli: &'a Cli) -> Result<Self> {
        match (
            cli.launcher_contract_version,
            cli.launcher_profile,
            cli.launcher_repo_root.as_deref(),
        ) {
            (None, None, None) => Ok(Self::Direct),
            (Some(contract_version), Some(profile), Some(repo_root)) => {
                Ok(Self::Repository(RuntimeCompatibilityRequest {
                    repo_root,
                    contract_version: Some(contract_version),
                    profile,
                }))
            }
            _ => bail!("Incomplete generated-launcher repository validation handoff"),
        }
    }
}

#[cfg(test)]
fn launcher_capability_only_command(command: &CommandKind) -> bool {
    command.launcher_descriptor().scope == LauncherCommandScope::CapabilityOnly
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LauncherCommandScope {
    CapabilityOnly,
    Repository,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LauncherCommandDescriptor {
    name: &'static str,
    scope: LauncherCommandScope,
}

impl LauncherCommandDescriptor {
    const fn new(name: &'static str, scope: LauncherCommandScope) -> Self {
        Self { name, scope }
    }
}

impl CommandKind {
    fn launcher_descriptor(&self) -> LauncherCommandDescriptor {
        use LauncherCommandScope::{CapabilityOnly, Repository};

        let (name, scope) = match self {
            Self::Init(_) => (tool_defs::cli_command::INIT, CapabilityOnly),
            Self::Presets => (tool_defs::cli_command::PRESETS, CapabilityOnly),
            Self::Adopt(_) => (tool_defs::cli_command::ADOPT, CapabilityOnly),
            Self::Update(_) => (tool_defs::cli_command::UPDATE, CapabilityOnly),
            Self::Bootstrap(_) => (tool_defs::cli_command::BOOTSTRAP, Repository),
            Self::Setup => (tool_defs::cli_command::SETUP, Repository),
            Self::Doctor => (tool_defs::cli_command::DOCTOR, CapabilityOnly),
            Self::Info(_) => (tool_defs::cli_command::INFO, Repository),
            Self::Dev(_) => (tool_defs::cli_command::DEV, Repository),
            Self::Check(opts) if opts.is_contract_only() => {
                (tool_defs::cli_command::CHECK, CapabilityOnly)
            }
            Self::Check(_) => (tool_defs::cli_command::CHECK, Repository),
            Self::FileBudget(_) => (tool_defs::cli_command::FILE_BUDGET, Repository),
            Self::Status(_) => (tool_defs::cli_command::STATUS, Repository),
            Self::Ui(_) => (tool_defs::cli_command::UI, Repository),
            Self::Work(_) => (tool_defs::cli_command::WORK, Repository),
            Self::Loop(_) => (tool_defs::cli_command::LOOP, Repository),
            Self::Migration(_) => (root_commands::MIGRATION.name, Repository),
            Self::Sqlx(_) => (root_commands::SQLX.name, Repository),
            Self::MigrationAdd(_) => (tool_defs::cli_command::MIGRATION_ADD, Repository),
            Self::SchemaDump(_) => (tool_defs::cli_command::SCHEMA_DUMP, Repository),
            Self::Vault(_) => (tool_defs::cli_command::VAULT, Repository),
            Self::GenerateSqlxUncheckedQueriesTodo(_) => (
                tool_defs::cli_command::GENERATE_SQLX_UNCHECKED_QUERIES_TODO,
                Repository,
            ),
            Self::Proxy(_) => (tool_defs::cli_command::PROXY, Repository),
            Self::Prompt(_) => ("prompt", Repository),
            Self::Agent(_) => (tool_defs::cli_command::AGENT, Repository),
            Self::Codex(_) => (tool_defs::cli_command::CODEX, CapabilityOnly),
            Self::AgentMap(_) => (tool_defs::cli_command::AGENT_MAP, Repository),
            Self::State(_) => (tool_defs::cli_command::STATE, Repository),
            Self::Mcp => (tool_defs::cli_command::MCP, Repository),
            Self::RuntimeCompatible(_) => ("__runtime-compatible", CapabilityOnly),
        };

        // Repository-scoped commands must operate exclusively on the
        // generated launcher's validated root. A command that accepts a
        // caller-relative repository target belongs in CapabilityOnly instead;
        // keeping this match exhaustive forces every new top-level command to
        // make that choice explicitly.
        LauncherCommandDescriptor::new(name, scope)
    }
}

#[cfg(test)]
fn launcher_capability_only_top_level_name(name: &str) -> bool {
    LAUNCHER_CAPABILITY_ONLY_SUBCOMMANDS
        .split(',')
        .any(|capability| capability == name)
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
            let ctx = RepoContext::load().map_err(|error| {
                if json_output {
                    super::json_command_error("ui", error)
                } else {
                    error
                }
            })?;
            ui::run(ctx, opts, json_output)
        }
        CommandKind::Doctor => {
            let output = doctor::run()?;
            emit(json_output, HumanOutput::Doctor, &output)?;
            finish_after_json_output(require_json_ok(true, &output), json_output)
        }
        CommandKind::Info(opts) => {
            if matches!(opts.subject.as_ref(), Some(super::InfoCommand::GoVersion)) {
                if opts.commands {
                    bail!("--commands cannot be combined with an info subject");
                }
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
                return Ok(());
            }
            let request = opts.subject.map(|subject| match subject {
                super::InfoCommand::GoVersion => unreachable!("handled above"),
                super::InfoCommand::Workspace => crate::repository::InspectRequest::Workspace,
                super::InfoCommand::Components => crate::repository::InspectRequest::Components,
                super::InfoCommand::Component { id } => {
                    crate::repository::InspectRequest::Component(id)
                }
                super::InfoCommand::Targets => crate::repository::InspectRequest::Targets,
                super::InfoCommand::Target { id } => crate::repository::InspectRequest::Target(id),
                super::InfoCommand::Profiles => crate::repository::InspectRequest::Profiles,
                super::InfoCommand::Profile { id } => {
                    crate::repository::InspectRequest::Profile(id)
                }
            });
            let output = info::run(opts.commands, json_output, request)?;
            emit(json_output, HumanOutput::Info, &output)?;
            finish_after_json_output(require_json_ok(true, &output), json_output)
        }
        CommandKind::Status(opts) => {
            let ctx = RepoContext::load()?;
            if let Some(StatusCommand::Run { run_id }) = &opts.command {
                let output = status_run_output(&ctx, run_id)?;
                return emit(json_output, HumanOutput::RunStatus, &output);
            }
            if opts.tui {
                return ui::run_status(
                    ctx,
                    std::time::Duration::from_secs(opts.effective_refresh_seconds()),
                );
            }
            #[cfg(all(unix, not(test)))]
            let signal_session = crate::doctor::DoctorSignalSession::start().map_err(|_| {
                anyhow::anyhow!("Status was not started because signal supervision is unavailable")
            })?;
            #[cfg(all(unix, not(test)))]
            let cancellation = signal_session.cancellation();
            #[cfg(all(unix, not(test)))]
            let outcome = status::snapshot_with_cancellation(&ctx, &|| cancellation.cancelled());
            #[cfg(all(unix, not(test)))]
            let outcome = crate::codex::finish_signal_supervised(
                outcome,
                signal_session.finish(),
                "Status signal supervision could not retire safely",
            );
            #[cfg(any(not(unix), test))]
            let outcome = status::snapshot(&ctx);
            let output = outcome?;
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
            let command: crate::command::CheckCommand = command.try_into()?;
            dispatch_runtime_command(
                crate::command::RuntimeCommand::Check(command),
                true,
                json_output,
                HumanOutput::Check,
            )
        }
        CommandKind::FileBudget(command) => {
            super::file_budget::run_file_budget_command(command, json_output)
        }
        CommandKind::Migration(MigrationCommand::Add(opts)) => run_migration_add(opts, json_output),
        CommandKind::Sqlx(command) => run_sqlx_command(command, json_output),
        CommandKind::SchemaDump(opts) => dispatch_runtime_command(
            crate::command::RuntimeCommand::Sqlx(crate::command::SqlxCommand::SchemaDump(
                opts.into(),
            )),
            false,
            json_output,
            HumanOutput::ToolExecution,
        ),
        CommandKind::MigrationAdd(opts) => run_migration_add(opts, json_output),
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
            let require_ok = loop_command_reports_failure_with_ok(&command);
            let human_output = loop_human_output(&command);
            dispatch_runtime_command(
                crate::command::RuntimeCommand::Loop(command.into()),
                require_ok,
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

fn status_run_output(ctx: &RepoContext, run_id: &str) -> Result<serde_json::Value> {
    let run = crate::state::reconcile_run_for_inspection(ctx, run_id)?;
    let mut output = serde_json::to_value(run)?;
    output["ok"] = serde_json::json!(true);
    output["command"] = serde_json::json!("status run");
    Ok(output)
}

fn run_sqlx_command(command: SqlxCommand, json_output: bool) -> Result<()> {
    match command {
        SqlxCommand::Migration(SqlxMigrationCommand::Add(opts)) => {
            run_migration_add(opts, json_output)
        }
        SqlxCommand::Schema(SqlxSchemaCommand::Dump(opts)) => dispatch_runtime_command(
            crate::command::RuntimeCommand::Sqlx(crate::command::SqlxCommand::SchemaDump(
                opts.into(),
            )),
            false,
            json_output,
            HumanOutput::ToolExecution,
        ),
    }
}

fn run_migration_add(opts: MigrationAddOpts, json_output: bool) -> Result<()> {
    dispatch_runtime_command(
        crate::command::RuntimeCommand::MigrationAdd(opts.into()),
        false,
        json_output,
        HumanOutput::MigrationAdd,
    )
}

fn run_runtime_compatible(opts: RuntimeCompatibleOpts) -> Result<()> {
    RuntimeCompatibilityProbe::from_opts(&opts).validate()
}

#[derive(Clone, Copy, Debug)]
struct RuntimeCompatibilityRequest<'a> {
    repo_root: &'a std::path::Path,
    contract_version: Option<u32>,
    profile: RuntimeCompatibilityProfile,
}

impl RuntimeCompatibilityRequest<'_> {
    fn canonical_repo_root(self) -> Result<std::path::PathBuf> {
        let repo_root = std::fs::canonicalize(self.repo_root).with_context(|| {
            format!(
                "Failed to resolve Jig repository root {}",
                self.repo_root.display()
            )
        })?;
        if let Some(contract_version) = self.contract_version
            && !crate::context::is_supported_contract_version(contract_version)
        {
            bail!(
                "Unsupported Jig contract version {contract_version}; this runtime supports versions {} through {}",
                crate::context::MIN_SUPPORTED_CONTRACT_VERSION,
                crate::context::CURRENT_CONTRACT_VERSION
            );
        }
        Ok(repo_root)
    }

    fn validate_profile(self) -> Result<()> {
        if self.profile == RuntimeCompatibilityProfile::Default && !cfg!(feature = "dev-proxy") {
            bail!(
                "This Jig binary is incompatible with the default runtime profile because it was built without the dev-proxy feature"
            );
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
enum RuntimeCompatibilityProbe<'a> {
    Capability(RuntimeCompatibilityRequest<'a>),
    Repository(RuntimeCompatibilityRequest<'a>),
}

impl<'a> RuntimeCompatibilityProbe<'a> {
    fn from_opts(opts: &'a RuntimeCompatibleOpts) -> Self {
        let request = RuntimeCompatibilityRequest {
            repo_root: &opts.repo_root,
            contract_version: opts.contract_version,
            profile: opts.profile,
        };
        if opts.capability_only {
            Self::Capability(request)
        } else {
            Self::Repository(request)
        }
    }

    fn validate(self) -> Result<()> {
        match self {
            Self::Capability(request) => validate_capability_runtime_compatibility(request),
            Self::Repository(request) => {
                validate_repository_runtime_compatibility(request).map(|_| ())
            }
        }
    }
}

fn validate_capability_runtime_compatibility(
    request: RuntimeCompatibilityRequest<'_>,
) -> Result<()> {
    let repo_root = request.canonical_repo_root()?;
    if request.contract_version.is_none() {
        // Keep direct/manual uses of the private probe useful. Generated
        // launchers and installers pass their rendered epoch explicitly so
        // repair paths do not depend on a readable manifest.
        RepoContext::supported_contract_version_from_root(&repo_root)?;
    }
    request.validate_profile()
}

fn validate_repository_runtime_compatibility(
    request: RuntimeCompatibilityRequest<'_>,
) -> Result<RepoContext> {
    let repo_root = request.canonical_repo_root()?;
    if let Some(launcher_contract_version) = request.contract_version {
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
    request.validate_profile()?;
    Ok(ctx)
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
        CommandKind::Loop(command) => loop_command_reports_failure_with_ok(command),
        CommandKind::Check(_) => true,
        _ => false,
    }
}

const fn agent_command_reports_failure_with_ok(command: &AgentCommand) -> bool {
    matches!(command, AgentCommand::Doctor)
}

const fn loop_command_reports_failure_with_ok(command: &LoopCommand) -> bool {
    matches!(
        command,
        LoopCommand::Tick(_) | LoopCommand::Dispatch(_) | LoopCommand::Run(_)
    )
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
        LoopCommand::Dispatch(_) => HumanOutput::LoopDispatch,
        LoopCommand::Status(_) => HumanOutput::LoopStatus,
        LoopCommand::Run(_) => HumanOutput::LoopRun,
        LoopCommand::ClearAttempt(_) => HumanOutput::LoopClearAttempt,
        LoopCommand::AcknowledgeOccurrence(_) => HumanOutput::LoopAcknowledgeOccurrence,
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
    #[cfg(all(unix, not(test)))]
    if command.signal_policy() == crate::command::RuntimeSignalPolicy::Native {
        let output = runtime::dispatch(&ctx, command)?;
        emit(json_output, human_output, &output)?;
        return finish_after_json_output(require_json_ok(require_ok, &output), json_output);
    }
    #[cfg(all(unix, not(test)))]
    let signal_session = crate::doctor::DoctorSignalSession::start().map_err(|_| {
        anyhow::anyhow!("Command was not started because signal supervision is unavailable")
    })?;
    #[cfg(all(unix, not(test)))]
    let cancellation = signal_session.cancellation();
    #[cfg(all(unix, not(test)))]
    let mut observer =
        crate::progress::CliExecutionObserver::with_cancellation(json_output, move || {
            cancellation.cancelled()
        });
    #[cfg(any(not(unix), test))]
    let mut observer = crate::progress::CliExecutionObserver::for_human_output(json_output);
    let outcome = runtime::dispatch_with_observer(&ctx, command, &mut observer);
    let outcome = observer.finish_with(outcome);
    #[cfg(all(unix, not(test)))]
    let outcome = crate::codex::finish_signal_supervised(
        outcome,
        signal_session.finish(),
        "Command signal supervision could not retire safely",
    );
    let output = outcome?;
    emit(json_output, human_output, &output)?;
    finish_after_json_output(require_json_ok(require_ok, &output), json_output)
}

mod argument_parsing;
pub(super) use argument_parsing::*;

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
