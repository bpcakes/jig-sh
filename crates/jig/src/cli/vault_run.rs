use anyhow::Result;

use super::output::{HumanOutput, emit};
use super::structured_error::{require_json_ok, require_vault_child_status_ok};
use super::vault::VaultCommand;
use crate::{context::RepoContext, runtime};

pub(super) fn run_vault_command(command: VaultCommand, json_output: bool) -> Result<()> {
    let human_output = vault_human_output(&command);
    let mut runtime_command: crate::command::VaultCommand = command.into();
    apply_repo_vault_scope(&mut runtime_command)?;
    let is_run = matches!(runtime_command, crate::command::VaultCommand::Run(_));
    if vault_command_requires_passphrase(&runtime_command) {
        // Invariant: capture and clear the process environment copy before vault
        // runtime code can start background threads.
        if matches!(runtime_command, crate::command::VaultCommand::Init(_)) {
            runtime::capture_new_vault_passphrase()?;
        } else {
            runtime::capture_vault_passphrase()?;
        }
    }
    let output = runtime::dispatch_vault(runtime_command)?;
    emit(json_output, human_output, &output)?;
    if is_run {
        // `vault run` mirrors the child process status. Its JSON `ok` field is
        // derived from that same status, so avoid reporting a second generic
        // ok=false error for the same child failure.
        return require_vault_child_status_ok(&output);
    }
    require_json_ok(true, &output)
}

const fn vault_human_output(command: &VaultCommand) -> HumanOutput {
    match command {
        VaultCommand::Run(_) => HumanOutput::VaultRun,
        _ => HumanOutput::VaultGeneric,
    }
}

const fn vault_command_requires_passphrase(command: &crate::command::VaultCommand) -> bool {
    !matches!(command, crate::command::VaultCommand::Status(_))
}

pub(super) fn apply_repo_vault_scope(command: &mut crate::command::VaultCommand) -> Result<()> {
    let options = vault_options_mut(command);
    if options.home.is_some() {
        return Ok(());
    }

    let Some(ctx) = RepoContext::load_optional()? else {
        return Ok(());
    };
    let vault = ctx.vault_config();
    apply_repo_vault_scope_to_options(
        options,
        runtime::repo_vault_options_for_context(&ctx),
        vault.allow_global(),
    )
}

pub(super) const fn vault_options_mut(
    command: &mut crate::command::VaultCommand,
) -> &mut crate::command::VaultRuntimeOptions {
    match command {
        crate::command::VaultCommand::Audit(command) => match command {
            crate::command::VaultAuditCommand::Verify(request) => &mut request.vault,
        },
        crate::command::VaultCommand::Init(request) => &mut request.vault,
        crate::command::VaultCommand::Status(request) => &mut request.vault,
        crate::command::VaultCommand::Secret(command) => match command {
            crate::command::VaultSecretCommand::List(request) => &mut request.vault,
            crate::command::VaultSecretCommand::Set(request) => &mut request.vault,
            crate::command::VaultSecretCommand::Remove(request) => &mut request.vault,
        },
        crate::command::VaultCommand::Run(request) => &mut request.vault,
    }
}

pub(super) fn apply_repo_vault_scope_to_options(
    options: &mut crate::command::VaultRuntimeOptions,
    repo_options: Option<crate::command::VaultRuntimeOptions>,
    allow_global: bool,
) -> Result<()> {
    if options.home.is_some() {
        return Ok(());
    }
    let has_repo_scope = repo_options.is_some();
    match &options.scope {
        crate::command::VaultScopeSelection::Auto => {
            if let Some(repo_options) = repo_options {
                *options = repo_options;
            }
            Ok(())
        }
        crate::command::VaultScopeSelection::Global if !allow_global && has_repo_scope => {
            anyhow::bail!(
                "This repo is configured for repo-scoped vault access and [vault].allow_global is false; remove --global or set allow_global = true after reviewing the risk."
            )
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
#[path = "vault_run_tests.rs"]
mod tests;
