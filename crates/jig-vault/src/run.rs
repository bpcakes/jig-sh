#[cfg(test)]
use std::io;
#[cfg(all(test, target_os = "macos"))]
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};
#[cfg(test)]
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result as AnyResult, anyhow, bail};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::SecretBytes;
use crate::env_policy::is_preserved_env_var_name;
use crate::redact::Redactor;
use crate::types::{EnvVarName, SecretName};

mod output;
mod process;
#[cfg(any(target_os = "linux", test))]
mod process_linux;
#[cfg(any(target_os = "linux", target_os = "macos", test))]
mod process_unix;
mod secret_files;

use process::{BrokeredProcess, wait_for_capped_output};
use secret_files::BrokeredSecretFiles;
#[cfg(all(test, unix))]
use secret_files::wipe_secret_file;

// Keep this cap aligned with redaction cost: redaction scans the captured text
// once per raw/encoded secret needle.
pub const MAX_CAPTURED_STREAM_BYTES: usize = 1024 * 1024;
const BROKERED_RUN_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const BROKERED_PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);
const BROKERED_PROCESS_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
const BROKERED_OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_millis(250);
// Output progress warrants a faster retry than idle process polling. A 1 ms
// floor keeps deadline and capture-limit enforcement responsive without
// sustaining thousands of wakeups per second for continuously chatty children.
const ACTIVE_OUTPUT_POLL_INTERVAL: Duration = Duration::from_millis(1);
// A shell can issue thousands of tiny writes. Keep each poll bounded while
// draining enough syscalls to avoid throttling finite output behind the poll
// interval and turning a capture-limit error into a process timeout.
const MAX_STREAM_READS_PER_POLL: usize = 1024;
// Also bound byte work so a normally buffered writer cannot postpone the next
// process/deadline observation for an entire capture-sized burst.
const MAX_STREAM_BYTES_PER_POLL: usize = 64 * 1024;

fn checked_deadline(label: &str, timeout: Duration) -> AnyResult<Instant> {
    Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| anyhow!("{label} deadline overflowed"))
}

#[derive(Debug)]
pub(crate) struct ResolvedBrokeredEnv {
    pub(crate) var: EnvVarName,
    pub(crate) secret_name: SecretName,
    pub(crate) value: SecretBytes,
}

#[derive(Debug)]
pub(crate) struct ResolvedBrokeredFile {
    pub(crate) var: EnvVarName,
    pub(crate) secret_name: SecretName,
    pub(crate) value: SecretBytes,
}

#[derive(Debug)]
pub(crate) struct ResolvedBrokeredRun {
    pub(crate) command: Vec<String>,
    pub(crate) env: Vec<ResolvedBrokeredEnv>,
    pub(crate) files: Vec<ResolvedBrokeredFile>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RunOutput {
    pub exit_status: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_signal: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

pub(crate) fn run_brokered(request: ResolvedBrokeredRun) -> AnyResult<RunOutput> {
    run_brokered_with_timeout(request, BROKERED_RUN_TIMEOUT)
}

fn run_brokered_with_timeout(
    request: ResolvedBrokeredRun,
    timeout: Duration,
) -> AnyResult<RunOutput> {
    // Keep this guard for direct crate callers; clap enforces it for the CLI.
    if request.command.is_empty() {
        bail!("vault run requires a command after --");
    }
    let redactor = Redactor::from_secret_slices(
        request
            .env
            .iter()
            .map(|mapping| mapping.value.as_slice())
            .chain(request.files.iter().map(|mapping| mapping.value.as_slice())),
    );
    let file_env = BrokeredSecretFiles::create(&request.files)?;
    let mut env_values = Vec::<(String, Zeroizing<String>)>::new();
    for mapping in request.env {
        let env_value = match mapping.value.into_zeroizing_string() {
            Ok(value) => value,
            Err(_value) => {
                bail!(
                    "vault secret '{}' cannot be injected as env var {} because it is not valid UTF-8",
                    mapping.secret_name.as_str(),
                    mapping.var.as_str()
                );
            }
        };
        env_values.push((mapping.var.as_str().to_string(), env_value));
    }

    let mut command = Command::new(&request.command[0]);
    command.args(&request.command[1..]).env_clear();
    preserve_minimal_environment(&mut command);
    for (name, value) in &env_values {
        // std::process::Command copies env values into OsString storage; keep
        // our source copy zeroized, but the std-owned copy is dropped normally.
        command.env(name, value.as_str());
    }
    if let Some(file_env) = &file_env {
        for (name, path) in file_env.env() {
            command.env(name, path);
        }
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let process = BrokeredProcess::spawn(&mut command)
        .with_context(|| format!("failed to run brokered command '{}'", request.command[0]))?;
    let (status, stdout, stderr) = wait_for_capped_output(process, &request.command[0], timeout)?;
    Ok(RunOutput {
        exit_status: status.exit_status,
        exit_signal: status.exit_signal,
        stdout: redactor.redact_bytes_lossy(stdout.as_slice()),
        stderr: redactor.redact_bytes_lossy(stderr.as_slice()),
    })
}

fn preserve_minimal_environment(command: &mut Command) {
    // Env forwarding is allowlist-only. Loader/interpreter hooks such as
    // LD_PRELOAD, DYLD_*, PYTHONPATH, NODE_OPTIONS, SSH_AUTH_SOCK, XDG_*,
    // and TZ stay out unless deliberately added to the exact list below.
    for (name, value) in std::env::vars() {
        if is_preserved_env_var_name(&name) {
            command.env(name, value);
        }
    }
}

#[cfg(test)]
mod tests;
