use std::path::Path;
use std::process::{Command, Stdio};

use super::{
    TrackerError, TrackerOperation, TrackerOutputStream, TrackerProcessPolicy, profile_0_5_7,
};
use jig_owned_process::{
    OwnedProcessObserver, OwnedProcessOutputStream, OwnedProcessTreeError,
    ProcessOutputOverflowPolicy, run_owned_process_tree_with_output_policy_and_observer,
};

mod budget;
mod executable;
mod store;

pub(super) use budget::OperationBudget;
pub(super) use executable::{ResolvedExecutable, resolve_br};
pub(super) use store::RetainedStore;

#[cfg(test)]
pub(crate) use executable::{MAX_EXECUTABLE_BYTES, TestBrOverride};
#[cfg(test)]
pub(crate) use store::MAX_STORE_SNAPSHOT_BYTES;
#[cfg(test)]
pub(crate) use store::TestStoreSnapshotHook;

const PROVIDER_IDENTITY_ENVIRONMENT: &[&str] = &[
    "BD_ACTOR",
    "BEADS_ACTOR",
    "BR_AGENT_NAME",
    "BR_HARNESS",
    "BR_MODEL",
    "BR_SESSION",
];

#[cfg(target_os = "linux")]
pub(super) fn make_descriptor_inheritable(descriptor: std::os::fd::RawFd) -> std::io::Result<()> {
    // SAFETY: `descriptor` belongs to the prepared executable and is live until
    // the parent finishes the supervised spawn.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFD) };
    if flags == -1 {
        return Err(std::io::Error::last_os_error());
    }
    if flags & libc::FD_CLOEXEC != 0 {
        // SAFETY: the same live descriptor and flags returned by `F_GETFD` are
        // passed back to the async-signal-safe `fcntl` system call.
        if unsafe { libc::fcntl(descriptor, libc::F_SETFD, flags & !libc::FD_CLOEXEC) } == -1 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn descriptor_path(descriptor: std::os::fd::RawFd) -> String {
    format!("/proc/self/fd/{descriptor}")
}

fn private_temp_directory(
    root: &Path,
    prefix: &str,
    operation: TrackerOperation,
) -> Result<tempfile::TempDir, TrackerError> {
    let base = std::env::temp_dir();
    let base = if base.is_absolute() {
        base
    } else {
        std::env::current_dir()
            .map_err(|_| prestart_failure(operation))?
            .join(base)
    };
    let base = base
        .canonicalize()
        .map_err(|_| prestart_failure(operation))?;
    let root = root
        .canonicalize()
        .map_err(|_| prestart_failure(operation))?;
    if base == root || base.starts_with(&root) {
        return Err(TrackerError::UnsafeTemporaryDirectory { operation });
    }
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(base)
        .map_err(|_| prestart_failure(operation))
}

fn remove_loader_injection_environment(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;

        // Loader controls are open-ended platform namespaces. Strip the whole
        // namespaces instead of maintaining an incomplete list of known keys.
        for (name, _) in std::env::vars_os() {
            let bytes = name.as_os_str().as_bytes();
            if bytes.starts_with(b"LD_") || bytes.starts_with(b"DYLD_") {
                command.env_remove(name);
            }
        }
    }
}

fn remove_provider_configuration_environment(command: &mut Command) {
    for (name, _) in std::env::vars_os() {
        if is_provider_configuration_environment(&name)
            && !PROVIDER_IDENTITY_ENVIRONMENT
                .iter()
                .any(|allowed| name == *allowed)
        {
            command.env_remove(name);
        }
    }
}

#[cfg(unix)]
fn is_provider_configuration_environment(name: &std::ffi::OsStr) -> bool {
    use std::os::unix::ffi::OsStrExt;

    [b"BD_".as_slice(), b"BR_", b"BEADS_", b"TOON_"]
        .iter()
        .any(|prefix| name.as_bytes().starts_with(prefix))
}

#[cfg(not(unix))]
fn is_provider_configuration_environment(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(|name| {
        ["BD_", "BR_", "BEADS_", "TOON_"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
    })
}

pub(super) struct ProcessRunner<'a> {
    root: &'a Path,
    executable: &'a ResolvedExecutable,
    store: Option<&'a RetainedStore>,
    policy: TrackerProcessPolicy,
}

impl<'a> ProcessRunner<'a> {
    pub(super) const fn new(
        root: &'a Path,
        executable: &'a ResolvedExecutable,
        store: Option<&'a RetainedStore>,
        policy: TrackerProcessPolicy,
    ) -> Self {
        Self {
            root,
            executable,
            store,
            policy,
        }
    }

    pub(super) fn run_json_with_budget(
        &self,
        operation: TrackerOperation,
        args: &[&str],
        budget: OperationBudget,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<serde_json::Value, TrackerError> {
        self.run_json_with_budget_and_hook(operation, args, budget, cancelled, || {})
    }

    #[cfg(test)]
    pub(super) fn run_json_after_prepare(
        &self,
        operation: TrackerOperation,
        args: &[&str],
        cancelled: &mut dyn FnMut() -> bool,
        after_prepare: impl FnOnce(),
    ) -> Result<serde_json::Value, TrackerError> {
        let budget = OperationBudget::new(self.timeout(operation));
        self.run_json_with_budget_and_hook(operation, args, budget, cancelled, after_prepare)
    }

    fn run_json_with_budget_and_hook(
        &self,
        operation: TrackerOperation,
        args: &[&str],
        budget: OperationBudget,
        cancelled: &mut dyn FnMut() -> bool,
        after_prepare: impl FnOnce(),
    ) -> Result<serde_json::Value, TrackerError> {
        let prepared_executable = self
            .executable
            .prepare(self.root, budget, operation, cancelled)?;
        let prepared_store = self
            .store
            .map(|store| store.prepare(self.root, budget, operation, cancelled))
            .transpose()?;
        after_prepare();
        if let Some(store) = &prepared_store {
            store.verify_before_spawn(operation)?;
        }
        let mut command = prepared_executable.command();
        command
            .current_dir(self.root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .args([
                "--no-auto-import",
                "--no-auto-flush",
                "--no-color",
                "--json",
            ]);
        remove_provider_configuration_environment(&mut command);
        remove_loader_injection_environment(&mut command);
        crate::shell::sanitize_bash_environment(&mut command);
        // The 0.5.7 config layer gives `BD_*` values precedence over project
        // and user configuration. Discovery is observational from the first
        // provider invocation; only store-backed operations may enable SQLite.
        command.env(
            "BD_NO_DB",
            if operation.uses_database() {
                "false"
            } else {
                "true"
            },
        );
        command.env("BR_STARTUP_CACHE", "0");
        if let Some(store) = &prepared_store {
            store.configure(&mut command, operation)?;
        }
        command.args(args);

        let timeout = budget.remaining_before_spawn(operation, cancelled)?;
        let mut observer = CancellationObserver(cancelled);
        let output = run_owned_process_tree_with_output_policy_and_observer(
            &mut command,
            timeout,
            self.policy.output_limits,
            ProcessOutputOverflowPolicy::Error,
            &mut observer,
        );
        drop(prepared_store);
        drop(prepared_executable);
        let output = output.map_err(|error| map_process_error(operation, error))?;

        let stdout = output.stdout.ok_or_else(|| response_error(operation))?;
        let stderr = output.stderr.ok_or_else(|| response_error(operation))?;
        if !stdout.complete || stdout.truncated || !stderr.complete || stderr.truncated {
            return Err(response_error(operation));
        }
        if output.status.success() {
            if operation.is_mutation() && !stderr.bytes.iter().all(u8::is_ascii_whitespace) {
                return Err(response_error(operation));
            }
            return crate::strict_json::from_slice(&stdout.bytes)
                .map_err(|_| response_error(operation));
        }

        if let Some(error) = profile_0_5_7::classify_provider_error(
            &stdout.bytes,
            &stderr.bytes,
            operation,
            args.last().copied(),
        ) {
            return Err(mutation_aware_response_error(error, operation));
        }
        if operation == TrackerOperation::CloseIssue
            && let Some(issue_id) = args.last()
            && let Some(error) =
                profile_0_5_7::classify_close_noop(&stdout.bytes, &stderr.bytes, issue_id)
        {
            return Err(error);
        }
        Err(if operation.is_mutation() {
            TrackerError::IndeterminateWrite { operation }
        } else {
            TrackerError::ProcessFailure {
                operation,
                exit_code: output.status.code(),
            }
        })
    }

    #[cfg(test)]
    fn timeout(&self, operation: TrackerOperation) -> std::time::Duration {
        match operation {
            TrackerOperation::Version | TrackerOperation::Info => self.policy.discovery_timeout,
            operation if operation.is_mutation() => self.policy.mutation_timeout,
            _ => self.policy.read_timeout,
        }
    }
}

fn mutation_aware_response_error(error: TrackerError, operation: TrackerOperation) -> TrackerError {
    if operation.is_mutation() && matches!(error, TrackerError::UnsupportedResponse { .. }) {
        TrackerError::IndeterminateWrite { operation }
    } else {
        error
    }
}

struct CancellationObserver<'a>(&'a mut dyn FnMut() -> bool);

impl OwnedProcessObserver for CancellationObserver<'_> {
    fn cancelled(&mut self) -> bool {
        (self.0)()
    }
}

fn response_error(operation: TrackerOperation) -> TrackerError {
    if operation.is_mutation() {
        TrackerError::IndeterminateWrite { operation }
    } else {
        TrackerError::UnsupportedResponse { operation }
    }
}

const fn prestart_failure(operation: TrackerOperation) -> TrackerError {
    TrackerError::ProcessFailure {
        operation,
        exit_code: None,
    }
}

fn map_process_error(operation: TrackerOperation, error: OwnedProcessTreeError) -> TrackerError {
    match error {
        OwnedProcessTreeError::Start(_) => TrackerError::ProcessFailure {
            operation,
            exit_code: None,
        },
        OwnedProcessTreeError::CancelledBeforeStart => {
            TrackerError::CancelledBeforeStart { operation }
        }
        OwnedProcessTreeError::TimedOut if operation.is_mutation() => {
            TrackerError::IndeterminateWrite { operation }
        }
        OwnedProcessTreeError::TimedOut => TrackerError::TimedOut { operation },
        OwnedProcessTreeError::Cancelled if operation.is_mutation() => {
            TrackerError::IndeterminateWrite { operation }
        }
        OwnedProcessTreeError::Cancelled => TrackerError::Cancelled { operation },
        OwnedProcessTreeError::OutputLimitExceeded(_) if operation.is_mutation() => {
            TrackerError::IndeterminateWrite { operation }
        }
        OwnedProcessTreeError::OutputLimitExceeded(stream) => TrackerError::OutputLimit {
            operation,
            stream: match stream {
                OwnedProcessOutputStream::Stdout => TrackerOutputStream::Stdout,
                OwnedProcessOutputStream::Stderr => TrackerOutputStream::Stderr,
            },
        },
        OwnedProcessTreeError::Await | OwnedProcessTreeError::Cleanup
            if operation.is_mutation() =>
        {
            TrackerError::IndeterminateWrite { operation }
        }
        OwnedProcessTreeError::Await | OwnedProcessTreeError::Cleanup => {
            TrackerError::ProcessFailure {
                operation,
                exit_code: None,
            }
        }
    }
}
