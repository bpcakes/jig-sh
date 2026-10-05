//! `jig dev` and `jig proxy` dispatch, including the private dev-worker
//! handoff whose worker owns the dev lifecycle and its output.

use std::process;

use anyhow::Result;

use super::finish_after_json_output;
use crate::cli::output::{self, emit, print_json};
use crate::cli::structured_error::{
    json_error_payload, json_reported_error, require_foreground_status,
};
use crate::cli::{DevOpts, ProxyCommand};
use crate::command::RuntimeCommand;
use crate::context::RepoContext;
use crate::dev_proxy::commands::{can_run_without_context, dev_contextless, proxy_without_context};
use crate::{root_commands, runtime};

pub(super) fn run_dev_command(opts: DevOpts, json_output: bool) -> Result<()> {
    let render = opts.renderer();
    if opts.is_contextless() {
        let output = dev_contextless(opts.into())?;
        emit(json_output, render, &output)?;
        return finish_after_json_output(require_foreground_status(&output), json_output);
    }
    let Some(ctx) = RepoContext::load_optional()? else {
        anyhow::bail!(
            "`scripts/jig dev` requires an adopted Jig repo with `.jig.toml`. Run it from a Jig repo, or preview adoption with `scripts/jig adopt .` and apply it with `scripts/jig adopt . --write`."
        );
    };
    if let Some(identity_present) = dev_launch_identity_present(&opts) {
        ensure_dev_process_identity(&ctx, identity_present);
    }
    #[cfg(unix)]
    let _launcher_watch = if opts.command.is_none() {
        match opts.launch.jig_worker_fd {
            Some(fd) => {
                // SAFETY: the private CLI worker protocol transfers its
                // inherited descriptor with no other Rust owner.
                Some(unsafe { jig_dev_proxy::DevLauncherWatch::from_inherited_fd(fd) }?)
            }
            None => return run_dev_worker(json_output),
        }
    } else {
        None
    };
    let output = runtime::dispatch(&ctx, RuntimeCommand::Dev(opts.into()))?;
    emit(json_output, render, &output)?;
    finish_after_json_output(require_foreground_status(&output), json_output)
}

pub(super) fn run_proxy_command(command: ProxyCommand, json_output: bool) -> Result<()> {
    let runtime_command: crate::command::ProxyCommand = command.into();
    let output = if can_run_without_context(&runtime_command) {
        if let Some(ctx) = RepoContext::load_optional()? {
            runtime::dispatch(&ctx, RuntimeCommand::Proxy(runtime_command))?
        } else {
            proxy_without_context(runtime_command)?
        }
    } else {
        let ctx = RepoContext::load()?;
        runtime::dispatch(&ctx, RuntimeCommand::Proxy(runtime_command))?
    };
    emit(json_output, output::format_proxy_summary, &output)?;
    finish_after_json_output(require_foreground_status(&output), json_output)
}

/// Whether a dev launch already carries its project identity; `None` for
/// management actions, which never re-exec.
pub(super) fn dev_launch_identity_present(opts: &DevOpts) -> Option<bool> {
    opts.command
        .is_none()
        .then_some(opts.launch.jig_project.is_some())
}

#[cfg(unix)]
fn run_dev_worker(json_output: bool) -> Result<()> {
    use std::os::unix::process::{CommandExt, ExitStatusExt};

    let mut command = process::Command::new(std::env::current_exe()?);
    command.arg0("jig").args(std::env::args_os().skip(1));
    let status = jig_dev_proxy::launch_dev_worker(command)?;
    let exit_status = status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(1));
    if let Some(signal) = status.signal() {
        let message = format!(
            "The owning Jig dev supervisor terminated with signal {signal}; app cleanup could not be confirmed. Inspect `jig dev status --json` before retrying."
        );
        if json_output {
            print_json(&json_error_payload(
                "dev_supervisor_lost",
                &message,
                exit_status,
            ))?;
        } else {
            eprintln!("{message}");
        }
        return Err(json_reported_error(exit_status));
    }
    if exit_status == 0 {
        Ok(())
    } else {
        // The worker owns stdout/stderr and has already emitted its result.
        Err(json_reported_error(exit_status))
    }
}

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

#[cfg(unix)]
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
        .position(|arg| arg == OsStr::new(root_commands::DEV.name))
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
