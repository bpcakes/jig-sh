use super::*;

#[cfg(all(feature = "dev-proxy", unix))]
pub(super) fn run_dev_worker(json_output: bool) -> Result<()> {
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

#[cfg(feature = "dev-proxy")]
pub(super) fn ensure_dev_process_identity(ctx: &RepoContext, identity_present: bool) {
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
