//! The generated launcher's handoff to this runtime: which commands the
//! launcher may hand over with a validated repository root, and whether this
//! binary can serve that repository's contract and profile.

use anyhow::{Context, Result, bail};
use jig_context::RepoContext;

use crate::cli::{Cli, CommandKind, RuntimeCompatibilityProfile, RuntimeCompatibleOpts};
use crate::root_commands::{self, LauncherCommand, LauncherScope};

pub(super) fn validate_launcher_repository_scope(cli: &Cli) -> Result<()> {
    if matches!(&cli.command, CommandKind::Dev(opts) if opts.is_contextless()) {
        return Ok(());
    }
    let LauncherHandoff::Repository(request) = LauncherHandoff::from_cli(cli)? else {
        return Ok(());
    };
    let command = cli.command.launcher_command();
    if command.scope == LauncherScope::CapabilityOnly {
        bail!(
            "The generated launcher and this Jig runtime disagree about whether `{}` is repository-scoped. Repair the launcher/runtime pair with a current external Jig binary (`jig update <repo> --launcher-only --force`) before retrying `{}`.",
            command.name,
            command.name,
        );
    }
    let ctx = validate_repository_runtime_compatibility(request).with_context(|| {
        format!(
            "The repository contract did not validate under Jig profile {}. Run scripts/jig check contract or scripts/jig doctor for repair guidance.",
            request.profile.as_str()
        )
    })?;
    if let Some(configured_root) = std::env::var_os(jig_context::JIG_REPO_ROOT_ENV) {
        let configured_root = std::path::PathBuf::from(configured_root);
        if !configured_root.as_os_str().is_empty()
            && std::fs::canonicalize(&configured_root).ok().as_deref() != Some(ctx.root())
        {
            eprintln!(
                "jig ignored {}={} because the generated launcher root {} is authoritative",
                jig_context::JIG_REPO_ROOT_ENV,
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
        std::env::set_var(jig_context::JIG_REPO_ROOT_ENV, authoritative_root);
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
pub(super) fn launcher_capability_only_command(command: &CommandKind) -> bool {
    command.launcher_command().scope == LauncherScope::CapabilityOnly
}

impl CommandKind {
    /// Ties every top-level command to its registry entry. Keep this match
    /// exhaustive: a new command cannot dispatch until the registry declares
    /// its generated-launcher scope.
    fn launcher_command(&self) -> LauncherCommand {
        match self {
            Self::Init(_) => root_commands::INIT.launcher(),
            Self::Presets => root_commands::PRESETS.launcher(),
            Self::Adopt(_) => root_commands::ADOPT.launcher(),
            Self::Update(_) => root_commands::UPDATE.launcher(),
            Self::Bootstrap => root_commands::BOOTSTRAP.launcher(),
            Self::Setup => root_commands::SETUP.launcher(),
            Self::Doctor => root_commands::DOCTOR.launcher(),
            Self::Info(_) => root_commands::INFO.launcher(),
            Self::Dev(_) => root_commands::DEV.launcher(),
            Self::Check(opts) if opts.is_contract_only() => {
                root_commands::CHECK.launcher().capability_only()
            }
            Self::Check(_) => root_commands::CHECK.launcher(),
            Self::Run(_) => root_commands::RUN.launcher(),
            Self::FileBudget(_) => root_commands::FILE_BUDGET.launcher(),
            Self::Status(_) => root_commands::STATUS.launcher(),
            Self::Ui(_) => root_commands::UI.launcher(),
            Self::Loop(_) => root_commands::LOOP.launcher(),
            Self::Migration(_) => root_commands::MIGRATION.launcher(),
            Self::Sqlx(_) => root_commands::SQLX.launcher(),
            Self::MigrationAdd(_) => root_commands::MIGRATION_ADD,
            Self::SchemaDump => root_commands::SCHEMA_DUMP,
            Self::Vault(_) => root_commands::VAULT.launcher(),
            Self::GenerateSqlxUncheckedQueriesTodo(_) => {
                root_commands::GENERATE_SQLX_UNCHECKED_QUERIES_TODO
            }
            Self::Proxy(_) => root_commands::PROXY.launcher(),
            Self::Agent(_) => root_commands::AGENT.launcher(),
            Self::Claude(_) => root_commands::CLAUDE.launcher(),
            Self::Codex(_) => root_commands::CODEX.launcher(),
            Self::AgentMap(_) => root_commands::AGENT_MAP.launcher(),
            Self::State(_) => root_commands::STATE.launcher(),
            Self::RuntimeCompatible(_) => root_commands::RUNTIME_COMPATIBLE,
        }
    }
}

pub(super) fn run_runtime_compatible(opts: RuntimeCompatibleOpts) -> Result<()> {
    RuntimeCompatibilityProbe::from_opts(&opts).validate()
}

#[derive(Clone, Copy, Debug)]
struct RuntimeCompatibilityRequest<'a> {
    repo_root: &'a std::path::Path,
    contract_version: Option<u32>,
    profile: RuntimeCompatibilityProfile,
}

impl RuntimeCompatibilityRequest<'_> {
    fn validate_active_contract_version(self, contract_version: u32) -> Result<()> {
        if !jig_context::is_active_contract_version(contract_version) {
            bail!(
                "Inactive Jig contract version {contract_version}; this runtime cache supports active versions {}",
                jig_context::active_contract_versions_label()
            );
        }
        Ok(())
    }

    fn canonical_repo_root(self) -> Result<std::path::PathBuf> {
        let repo_root = std::fs::canonicalize(self.repo_root).with_context(|| {
            format!(
                "Failed to resolve Jig repository root {}",
                self.repo_root.display()
            )
        })?;
        if let Some(contract_version) = self.contract_version {
            self.validate_active_contract_version(contract_version)?;
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
        let contract_version = RepoContext::supported_contract_version_from_root(&repo_root)?;
        request.validate_active_contract_version(contract_version)?;
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
    request.validate_active_contract_version(ctx.contract_version())?;
    crate::policy::validate_contract(&ctx)?;
    request.validate_profile()?;
    Ok(ctx)
}

#[cfg(test)]
mod tests;
