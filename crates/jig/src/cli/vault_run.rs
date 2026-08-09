use std::io::IsTerminal;
use std::path::Path;

#[cfg(windows)]
use anyhow::Context;
use anyhow::{Result, bail};

use super::output::{HumanOutput, emit};
use super::run::finish_after_json_output;
use super::structured_error::{require_json_ok, require_vault_child_status_ok};
use super::vault::VaultCommand;
use crate::{context::RepoContext, runtime};

pub(super) fn run_vault_command(command: VaultCommand, json_output: bool) -> Result<()> {
    run_vault_command_with_stdout_terminal(command, json_output, std::io::stdout().is_terminal())
}

fn run_vault_command_with_stdout_terminal(
    command: VaultCommand,
    json_output: bool,
    stdout_is_terminal: bool,
) -> Result<()> {
    let human_output = vault_human_output(&command);
    let mut runtime_command: crate::command::VaultCommand = command.into();
    let is_raw = vault_command_uses_raw_output(&runtime_command);
    validate_raw_vault_command(&runtime_command, json_output, stdout_is_terminal)?;
    if is_raw {
        runtime::prepare_vault_raw_input(&mut runtime_command)?;
    }
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
    if is_raw {
        return runtime::dispatch_vault_raw(runtime_command);
    }
    let output = runtime::dispatch_vault(runtime_command)?;
    emit(json_output, human_output, &output)?;
    if is_run {
        // `vault run` mirrors the child process status. Its JSON `ok` field is
        // derived from that same status, so avoid reporting a second generic
        // ok=false error for the same child failure.
        return finish_after_json_output(require_vault_child_status_ok(&output), json_output);
    }
    finish_after_json_output(require_json_ok(true, &output), json_output)
}

const fn vault_command_uses_raw_output(command: &crate::command::VaultCommand) -> bool {
    matches!(
        command,
        crate::command::VaultCommand::Inject(_) | crate::command::VaultCommand::Read(_)
    )
}

fn validate_raw_vault_command(
    command: &crate::command::VaultCommand,
    json_output: bool,
    stdout_is_terminal: bool,
) -> Result<()> {
    let (name, reveal, out_file, overwrite) = match command {
        crate::command::VaultCommand::Read(request) => (
            "vault read",
            request.reveal,
            request.out_file.as_deref(),
            request.overwrite,
        ),
        crate::command::VaultCommand::Inject(request) => (
            "vault inject",
            request.reveal,
            request.out_file.as_deref(),
            request.overwrite,
        ),
        _ => return Ok(()),
    };

    if json_output {
        bail!("--json is not supported by {name}; choose an exact byte output sink instead");
    }
    if reveal && out_file.is_some() {
        bail!("{name} cannot combine --reveal with --out-file");
    }
    if overwrite && out_file.is_none() {
        bail!("{name} requires --out-file when --overwrite is used");
    }
    if out_file.is_none() && stdout_is_terminal && !reveal {
        bail!("{name} refuses to reveal bytes to a terminal without --reveal");
    }

    if let crate::command::VaultCommand::Inject(request) = command {
        if let Some(output) = &request.out_file {
            if !request.overwrite && input_and_output_are_same_file(&request.input, output)? {
                bail!(
                    "vault inject input and output refer to the same file; pass --overwrite to request atomic replacement"
                );
            }
        }
    }

    Ok(())
}

fn input_and_output_are_same_file(input: &Path, output: &Path) -> Result<bool> {
    if input == Path::new("-") {
        return Ok(false);
    }
    if normalize_absolute_path(input)? == normalize_absolute_path(output)? {
        return Ok(true);
    }

    let (Ok(input_metadata), Ok(output_metadata)) =
        (std::fs::metadata(input), std::fs::metadata(output))
    else {
        return Ok(false);
    };

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        Ok(input_metadata.dev() == output_metadata.dev()
            && input_metadata.ino() == output_metadata.ino())
    }

    #[cfg(windows)]
    {
        let _ = (input_metadata, output_metadata);
        windows_file_identity(input).and_then(|input_identity| {
            windows_file_identity(output).map(|output_identity| input_identity == output_identity)
        })
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = (input_metadata, output_metadata);
        Ok(std::fs::canonicalize(input).ok() == std::fs::canonicalize(output).ok())
    }
}

#[cfg(windows)]
fn windows_file_identity(path: &Path) -> Result<(u32, u32, u32)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let file = std::fs::File::open(path).with_context(|| {
        format!(
            "failed to open {} for file identity validation",
            path.display()
        )
    })?;
    let mut identity = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `identity` remains writable for the call and `file` owns a valid
    // handle for the complete call duration.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as HANDLE, &mut identity) } == 0 {
        return Err(std::io::Error::last_os_error()).with_context(|| {
            format!(
                "failed to identify {} during sink validation",
                path.display()
            )
        });
    }
    Ok((
        identity.dwVolumeSerialNumber,
        identity.nFileIndexHigh,
        identity.nFileIndexLow,
    ))
}

fn normalize_absolute_path(path: &Path) -> Result<std::path::PathBuf> {
    use std::path::Component;

    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    Ok(normalized)
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
        crate::command::VaultCommand::Migrate(request) => &mut request.vault,
        crate::command::VaultCommand::Field(command) => match command {
            crate::command::VaultFieldCommand::List(request) => &mut request.vault,
            crate::command::VaultFieldCommand::Set(request) => &mut request.vault,
            crate::command::VaultFieldCommand::Remove(request) => &mut request.vault,
        },
        crate::command::VaultCommand::Inject(request) => &mut request.vault,
        crate::command::VaultCommand::Read(request) => &mut request.vault,
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
