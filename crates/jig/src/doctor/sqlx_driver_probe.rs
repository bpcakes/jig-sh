//! Probing the installed SQLx CLI for its compiled-in drivers.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use std::{env, fs};

use jig_owned_process::{OwnedProcessTreeError, run_owned_process_tree_with_output};

use super::cargo_sqlx::path_has_symlink_or_reparse_component;
use super::environment::DoctorEnvironment;
use super::programs::{ProgramOrigin, ProgramResolution, program_has_explicit_path};
use super::shell_analysis::executable_basename;
use super::sqlx_driver::SqlxDriver;

const SQLX_DRIVER_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum SqlxDriverProbe {
    Compatible,
    Incompatible,
    Indeterminate(String),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum SqlxProbeStyle {
    CargoSubcommand,
    Direct,
}

pub(super) fn probe_sqlx_driver(
    executable: &Path,
    style: SqlxProbeStyle,
    driver: SqlxDriver,
    root: &Path,
    environment: &DoctorEnvironment,
    cancellation: Option<&dyn Fn() -> bool>,
) -> SqlxDriverProbe {
    probe_sqlx_driver_with_timeout_and_environment_and_cancellation(
        executable,
        style,
        driver,
        SQLX_DRIVER_PROBE_TIMEOUT,
        root,
        environment,
        cancellation,
    )
}

#[cfg(all(test, unix))]
pub(super) fn probe_sqlx_driver_with_timeout(
    executable: &Path,
    style: SqlxProbeStyle,
    driver: SqlxDriver,
    timeout: Duration,
) -> SqlxDriverProbe {
    probe_sqlx_driver_with_timeout_and_environment(
        executable,
        style,
        driver,
        timeout,
        Path::new("/"),
        &DoctorEnvironment::default(),
    )
}

#[cfg(all(test, unix))]
pub(super) fn probe_sqlx_driver_with_timeout_and_environment(
    executable: &Path,
    style: SqlxProbeStyle,
    driver: SqlxDriver,
    timeout: Duration,
    root: &Path,
    environment: &DoctorEnvironment,
) -> SqlxDriverProbe {
    probe_sqlx_driver_with_timeout_and_environment_and_cancellation(
        executable,
        style,
        driver,
        timeout,
        root,
        environment,
        None,
    )
}

pub(super) fn probe_sqlx_driver_with_timeout_and_environment_and_cancellation(
    executable: &Path,
    style: SqlxProbeStyle,
    driver: SqlxDriver,
    timeout: Duration,
    root: &Path,
    environment: &DoctorEnvironment,
    cancellation: Option<&dyn Fn() -> bool>,
) -> SqlxDriverProbe {
    let Ok(temp) = tempfile::tempdir() else {
        return SqlxDriverProbe::Indeterminate(
            "could not create an isolated probe directory".into(),
        );
    };
    let migrations = temp.path().join("migrations");
    if fs::create_dir(&migrations).is_err() {
        return SqlxDriverProbe::Indeterminate(
            "could not create an isolated migration source".into(),
        );
    }
    let mut command = Command::new(executable);
    if style == SqlxProbeStyle::CargoSubcommand {
        // Cargo subcommand shims receive the subcommand name as argv[1].
        command.arg("sqlx");
    }
    command
        .args(["migrate", "info", "--source"])
        .arg(&migrations)
        .arg("--no-dotenv")
        .args(["--database-url", driver.probe_url()])
        .current_dir(temp.path())
        .env_clear()
        .env("NO_COLOR", "1")
        .env("LC_ALL", "C")
        .env("HOME", temp.path())
        .env("TMPDIR", temp.path())
        .env("TMP", temp.path())
        .env("TEMP", temp.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = sanitized_probe_search_path(root, executable) {
        command.env("PATH", path);
    }
    for (key, value) in &environment.probe_environment {
        command.env(key, value);
    }

    let output_result = run_owned_process_tree_with_output(&mut command, timeout, || {
        cancellation.is_some_and(|cancelled| cancelled())
    });

    let output = match output_result {
        Ok(output) => output,
        Err(error) => {
            return SqlxDriverProbe::Indeterminate(driver_probe_reason(&error).into());
        }
    };
    let Some(stdout) = output.stdout.as_ref() else {
        return SqlxDriverProbe::Indeterminate("the driver probe output was not captured".into());
    };
    let Some(stderr) = output.stderr.as_ref() else {
        return SqlxDriverProbe::Indeterminate("the driver probe output was not captured".into());
    };
    if !stdout.complete || !stderr.complete {
        return SqlxDriverProbe::Indeterminate(
            "the driver probe output capture did not complete".into(),
        );
    }
    if stdout.truncated || stderr.truncated {
        return SqlxDriverProbe::Indeterminate(
            "the driver probe output exceeded the diagnostic capture limit".into(),
        );
    }
    classify_sqlx_driver_probe(
        driver,
        output.status.success(),
        &stdout.to_string_lossy(),
        &stderr.to_string_lossy(),
    )
}

const fn driver_probe_reason(error: &OwnedProcessTreeError) -> &'static str {
    match error {
        OwnedProcessTreeError::Start(_) => "the driver probe could not start",
        OwnedProcessTreeError::TimedOut => "the driver probe timed out",
        OwnedProcessTreeError::CancelledBeforeStart => "the driver probe was cancelled",
        OwnedProcessTreeError::Cancelled => "the driver probe was cancelled",
        OwnedProcessTreeError::OutputLimitExceeded(_) => {
            "the driver probe output exceeded the diagnostic capture limit"
        }
        OwnedProcessTreeError::Await => "the driver probe could not be awaited",
        OwnedProcessTreeError::Cleanup => {
            "the driver probe process tree could not be cleaned up safely"
        }
    }
}

pub(super) fn classify_sqlx_driver_probe(
    driver: SqlxDriver,
    success: bool,
    stdout: &str,
    stderr: &str,
) -> SqlxDriverProbe {
    let output = format!("{stdout}\n{stderr}").to_ascii_lowercase();
    if output.contains("no driver found for url scheme") {
        return SqlxDriverProbe::Incompatible;
    }
    let postgres_driver_rejected_sslmode = driver == SqlxDriver::Postgres
        && output.lines().any(|line| {
            line.contains("jig-doctor-invalid")
                && (line.contains("sslmode") || line.contains("ssl_mode"))
                && (line.contains("unknown value") || line.contains("invalid value"))
        });
    if success || postgres_driver_rejected_sslmode {
        return SqlxDriverProbe::Compatible;
    }
    SqlxDriverProbe::Indeterminate("cargo-sqlx returned an unexpected result".into())
}

pub(super) fn sqlx_probe_style(program: &str) -> Option<SqlxProbeStyle> {
    let basename = executable_basename(program)?;
    if basename.eq_ignore_ascii_case("cargo-sqlx") {
        Some(SqlxProbeStyle::CargoSubcommand)
    } else if basename.eq_ignore_ascii_case("sqlx") {
        Some(SqlxProbeStyle::Direct)
    } else {
        None
    }
}

pub(super) fn trusted_sqlx_probe_executable(
    root: &Path,
    program: &str,
    resolution: &ProgramResolution,
) -> Option<PathBuf> {
    if !bare_sqlx_probe_program(program) {
        return None;
    }
    trusted_bare_path_executable(root, resolution)
}

fn trusted_bare_path_executable(root: &Path, resolution: &ProgramResolution) -> Option<PathBuf> {
    let ProgramOrigin::SearchPath { entry } = &resolution.origin else {
        return None;
    };
    if entry.as_os_str().is_empty() || !entry.is_absolute() {
        return None;
    }
    let root = fs::canonicalize(root).ok()?;
    if entry.starts_with(&root) || resolution.path.starts_with(&root) {
        return None;
    }
    if path_has_symlink_or_reparse_component(&resolution.path)? {
        return None;
    }
    let entry = fs::canonicalize(entry).ok()?;
    let executable = fs::canonicalize(&resolution.path).ok()?;
    if entry.starts_with(&root) || executable.starts_with(&root) {
        return None;
    }
    Some(executable)
}

fn bare_sqlx_probe_program(program: &str) -> bool {
    if program_has_explicit_path(program) {
        return false;
    }
    matches!(program, "sqlx" | "cargo-sqlx")
}

pub(super) fn sanitized_probe_search_path(root: &Path, executable: &Path) -> Option<OsString> {
    let root = fs::canonicalize(root).ok()?;
    let executable = fs::canonicalize(executable).ok()?;
    let directory = executable.parent()?;
    if !directory.is_absolute() || directory.starts_with(root) {
        return None;
    }
    env::join_paths([directory]).ok()
}
