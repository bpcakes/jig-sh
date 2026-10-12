//! Rust, Go, and Node.js runtime checks and their version probes.

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use jig_context::RepoContext;
use jig_owned_process::{OwnedProcessTreeError, run_owned_process_tree_with_output};
use serde_json::json;

use super::check::{DoctorCheck, check};
use super::environment::{DoctorEnvironment, DoctorProcessControl};
use super::programs::resolve_program;
use super::sqlx_driver_probe::sanitized_probe_search_path;
use super::version_authority::{
    NumericVersion, cargo_rust_version_authority, numeric_version_authority, parse_numeric_version,
    select_go_module_version_requirement,
};

const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) fn rust_runtime_check(
    ctx: &RepoContext,
    environment: &DoctorEnvironment,
    process_control: DoctorProcessControl<'_>,
) -> Option<DoctorCheck> {
    let authority_path = ctx.root().join("Cargo.toml");
    let required = match cargo_rust_version_authority(&authority_path) {
        Ok(Some(required)) => required,
        Ok(None) => return None,
        Err(reason) => {
            return Some(
                check(
                    "rust_runtime",
                    "Rust runtime",
                    true,
                    false,
                    "invalid authority",
                    reason,
                )
                .with_fix(
                    "Correct the root Cargo.toml rust-version authority, then run `scripts/jig doctor`.",
                )
                .with_data(json!({ "authority": authority_path.display().to_string() })),
            );
        }
    };
    let Some(resolution) = resolve_program(ctx.root(), "rustc", environment.search_path.as_deref())
    else {
        return Some(
            check(
                "rust_runtime",
                "Rust runtime",
                true,
                false,
                "missing",
                format!("Rust {required} or newer is required, but rustc was not found on PATH"),
            )
            .with_fix(&rust_runtime_fix(required))
            .with_data(json!({
                "authority": authority_path.display().to_string(),
                "required": required.to_string(),
                "actual": null,
            })),
        );
    };
    if let Some(reason) = process_control.unavailable_reason {
        return Some(
            check(
                "rust_runtime",
                "Rust runtime",
                true,
                false,
                "unverified",
                format!("Could not verify Rust {required} or newer ({reason})"),
            )
            .with_fix("Run `scripts/jig doctor` again before starting Rust work.")
            .with_data(json!({
                "authority": authority_path.display().to_string(),
                "required": required.to_string(),
                "actual": null,
            })),
        );
    }
    let actual = match probe_rust_version(
        &resolution.path,
        ctx.root(),
        environment,
        process_control.cancellation,
    ) {
        Ok(actual) => actual,
        Err(reason) => {
            return Some(
                check(
                    "rust_runtime",
                    "Rust runtime",
                    true,
                    false,
                    "unverified",
                    format!("Could not verify Rust {required} or newer ({reason})"),
                )
                .with_fix(
                    "Run `rustc --version`, correct the active toolchain, then rerun `scripts/jig doctor`.",
                )
                .with_data(json!({
                    "authority": authority_path.display().to_string(),
                    "required": required.to_string(),
                    "actual": null,
                })),
            );
        }
    };
    let compatible = actual >= required;
    let detail = if compatible {
        format!("Rust {actual} satisfies the required version {required}")
    } else {
        format!("Rust {actual} is active, but this repository requires {required} or newer")
    };
    let check = check(
        "rust_runtime",
        "Rust runtime",
        true,
        compatible,
        if compatible {
            "compatible"
        } else {
            "incompatible"
        },
        detail,
    )
    .with_data(json!({
        "authority": authority_path.display().to_string(),
        "required": required.to_string(),
        "actual": actual.to_string(),
    }));
    Some(if compatible {
        check
    } else {
        check.with_fix(&rust_runtime_fix(required))
    })
}

pub(super) fn go_runtime_check(
    ctx: &RepoContext,
    environment: &DoctorEnvironment,
    process_control: DoctorProcessControl<'_>,
) -> Option<DoctorCheck> {
    let authority_paths = match ctx.go_module_authority_paths() {
        Ok(paths) => paths,
        Err(error) => {
            return Some(
                check(
                    "go_runtime",
                    "Go runtime",
                    true,
                    false,
                    "invalid authority",
                    format!("Could not resolve Go module authority: {error}"),
                )
                .with_fix(
                    "Correct the repository component roots, then rerun `scripts/jig doctor`.",
                )
                .with_data(json!({
                    "authority": "",
                    "authorities": [],
                })),
            );
        }
    };
    if authority_paths.is_empty() {
        return None;
    }
    let authorities = authority_paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    let (authority_path, requirement) = match select_go_module_version_requirement(&authority_paths)
    {
        Ok(Some(selected)) => selected,
        Ok(None) => return None,
        Err(error) => {
            return Some(
                check(
                    "go_runtime",
                    "Go runtime",
                    true,
                    false,
                    "invalid authority",
                    error.reason,
                )
                .with_fix(&format!(
                    "Correct or restore {}, then rerun scripts/jig doctor.",
                    error.path.display()
                ))
                .with_data(json!({
                    "authority": error.path.display().to_string(),
                    "authorities": authorities,
                })),
            );
        }
    };
    let required = requirement.numeric;
    let Some(resolution) = resolve_program(ctx.root(), "go", environment.search_path.as_deref())
    else {
        return Some(
            check(
                "go_runtime",
                "Go runtime",
                true,
                false,
                "missing",
                format!("Go {required} or newer is required, but go was not found on PATH"),
            )
            .with_fix(&go_runtime_fix(required)),
        );
    };
    if let Some(reason) = process_control.unavailable_reason {
        return Some(check(
            "go_runtime",
            "Go runtime",
            true,
            false,
            "unverified",
            format!("Could not verify Go {required} or newer ({reason})"),
        ));
    }
    let actual = match probe_go_version(
        &resolution.path,
        ctx.root(),
        environment,
        process_control.cancellation,
    ) {
        Ok(actual) => actual,
        Err(reason) => {
            return Some(
                check(
                    "go_runtime",
                    "Go runtime",
                    true,
                    false,
                    "unverified",
                    format!("Could not verify Go {required} or newer ({reason})"),
                )
                .with_fix(
                    "Run go version, correct the active toolchain, then rerun scripts/jig doctor.",
                ),
            );
        }
    };
    let compatible = actual >= required;
    let result = check(
        "go_runtime",
        "Go runtime",
        true,
        compatible,
        if compatible {
            "compatible"
        } else {
            "incompatible"
        },
        format!("Go {actual} is active; this repository requires {required} or newer"),
    )
    .with_data(json!({
        "authority": authority_path.display().to_string(),
        "authorities": authorities,
        "required": required.to_string(),
        "actual": actual.to_string(),
    }));
    Some(if compatible {
        result
    } else {
        result.with_fix(&go_runtime_fix(required))
    })
}

fn rust_runtime_fix(required: NumericVersion) -> String {
    format!("Activate Rust {required} or newer with rustup, then run `scripts/jig doctor`.")
}

fn go_runtime_fix(required: NumericVersion) -> String {
    format!("Install or activate Go {required} or newer, then run `scripts/jig doctor`.")
}

pub(super) fn node_runtime_check(
    ctx: &RepoContext,
    environment: &DoctorEnvironment,
    process_control: DoctorProcessControl<'_>,
) -> Option<DoctorCheck> {
    if ctx.frontend_apps().is_empty() {
        return None;
    }
    let authority_path = ctx.root().join(".node-version");
    let required = match numeric_version_authority(&authority_path, "Node", false, "24.19.0") {
        Ok(Some(required)) => required,
        Ok(None) => return None,
        Err(reason) => {
            return Some(
                check(
                    "node_runtime",
                    "Node runtime",
                    true,
                    false,
                    "invalid authority",
                    reason,
                )
                .with_fix(
                    "Replace `.node-version` with one exact numeric version, then run `scripts/jig doctor`.",
                )
                .with_data(json!({ "authority": authority_path.display().to_string() })),
            );
        }
    };
    let Some(resolution) = resolve_program(ctx.root(), "node", environment.search_path.as_deref())
    else {
        return Some(
            check(
                "node_runtime",
                "Node runtime",
                true,
                false,
                "missing",
                format!("Node {required} or newer is required, but node was not found on PATH"),
            )
            .with_fix(&node_runtime_fix(required))
            .with_data(json!({
                "authority": authority_path.display().to_string(),
                "required": required.to_string(),
                "actual": null,
            })),
        );
    };
    if let Some(reason) = process_control.unavailable_reason {
        return Some(
            check(
                "node_runtime",
                "Node runtime",
                true,
                false,
                "unverified",
                format!("Could not verify Node {required} or newer ({reason})"),
            )
            .with_fix("Run `scripts/jig doctor` again before starting frontend work.")
            .with_data(json!({
                "authority": authority_path.display().to_string(),
                "required": required.to_string(),
                "actual": null,
            })),
        );
    }
    let actual = match probe_node_version(
        &resolution.path,
        ctx.root(),
        environment,
        process_control.cancellation,
    ) {
        Ok(actual) => actual,
        Err(reason) => {
            return Some(
                check(
                    "node_runtime",
                    "Node runtime",
                    true,
                    false,
                    "unverified",
                    format!("Could not verify Node {required} or newer ({reason})"),
                )
                .with_fix("Run `node --version`, correct the active runtime, then rerun `scripts/jig doctor`.")
                .with_data(json!({
                    "authority": authority_path.display().to_string(),
                    "required": required.to_string(),
                    "actual": null,
                })),
            );
        }
    };
    let compatible = actual >= required;
    let status = if compatible {
        "compatible"
    } else {
        "incompatible"
    };
    let detail = if compatible {
        format!("Node {actual} satisfies the required version {required}")
    } else {
        format!("Node {actual} is active, but this repository requires {required} or newer")
    };
    let check = check(
        "node_runtime",
        "Node runtime",
        true,
        compatible,
        status,
        detail,
    )
    .with_data(json!({
        "authority": authority_path.display().to_string(),
        "required": required.to_string(),
        "actual": actual.to_string(),
    }));
    Some(if compatible {
        check
    } else {
        check.with_fix(&node_runtime_fix(required))
    })
}

fn probe_node_version(
    executable: &Path,
    root: &Path,
    environment: &DoctorEnvironment,
    cancellation: Option<&dyn Fn() -> bool>,
) -> std::result::Result<NumericVersion, String> {
    let stdout = version_probe_stdout(
        executable,
        &["--version"],
        "node --version",
        root,
        environment,
        None,
        cancellation,
    )?;
    let mut tokens = stdout.split_ascii_whitespace();
    tokens
        .next()
        .and_then(|token| parse_numeric_version(token, true, false))
        .filter(|_| tokens.next().is_none())
        .ok_or_else(|| "node --version returned an invalid version".to_string())
}

fn probe_rust_version(
    executable: &Path,
    root: &Path,
    environment: &DoctorEnvironment,
    cancellation: Option<&dyn Fn() -> bool>,
) -> std::result::Result<NumericVersion, String> {
    let stdout = version_probe_stdout(
        executable,
        &["--version"],
        "rustc --version",
        root,
        environment,
        environment.home.as_deref(),
        cancellation,
    )?;
    let mut tokens = stdout.split_ascii_whitespace();
    if tokens.next() != Some("rustc") {
        return Err("rustc --version returned an invalid product name".into());
    }
    tokens
        .next()
        .and_then(|token| parse_numeric_version(token, false, false))
        .ok_or_else(|| "rustc --version returned an invalid version".to_string())
}

fn probe_go_version(
    executable: &Path,
    root: &Path,
    environment: &DoctorEnvironment,
    cancellation: Option<&dyn Fn() -> bool>,
) -> std::result::Result<NumericVersion, String> {
    let stdout = version_probe_stdout(
        executable,
        &["version"],
        "go version",
        root,
        environment,
        environment.home.as_deref(),
        cancellation,
    )?;
    let mut tokens = stdout.split_ascii_whitespace();
    if tokens.next() != Some("go") || tokens.next() != Some("version") {
        return Err("go version returned an invalid product name".into());
    }
    tokens
        .next()
        .and_then(|token| token.strip_prefix("go"))
        .and_then(|token| parse_numeric_version(token, false, false))
        .ok_or_else(|| "go version returned an invalid version".to_string())
}

pub(super) fn version_probe_stdout(
    executable: &Path,
    arguments: &[&str],
    invocation: &str,
    root: &Path,
    environment: &DoctorEnvironment,
    captured_home: Option<&OsStr>,
    cancellation: Option<&dyn Fn() -> bool>,
) -> std::result::Result<String, String> {
    let temp = tempfile::tempdir()
        .map_err(|_| "could not create an isolated probe directory".to_string())?;
    let home = captured_home.unwrap_or_else(|| temp.path().as_os_str());
    let mut command = Command::new(executable);
    command
        .args(arguments)
        .current_dir(root)
        .env_clear()
        .env("NO_COLOR", "1")
        .env("LC_ALL", "C")
        .env("HOME", home)
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

    let output = run_owned_process_tree_with_output(&mut command, VERSION_PROBE_TIMEOUT, || {
        cancellation.is_some_and(|cancelled| cancelled())
    })
    .map_err(|error| version_probe_reason(&error).to_string())?;
    let stdout = output
        .stdout
        .ok_or_else(|| "the version probe output was not captured".to_string())?;
    let stderr = output
        .stderr
        .ok_or_else(|| "the version probe output was not captured".to_string())?;
    if !stdout.complete || !stderr.complete {
        return Err("the version probe output capture did not complete".into());
    }
    if stdout.truncated || stderr.truncated {
        return Err("the version probe output exceeded the diagnostic capture limit".into());
    }
    if !output.status.success() {
        return Err(format!("{invocation} exited with status {}", output.status));
    }
    Ok(stdout.to_string_lossy().trim().to_string())
}

const fn version_probe_reason(error: &OwnedProcessTreeError) -> &'static str {
    match error {
        OwnedProcessTreeError::Start(_) => "the version probe could not start",
        OwnedProcessTreeError::TimedOut => "the version probe timed out",
        OwnedProcessTreeError::CancelledBeforeStart => "the version probe was cancelled",
        OwnedProcessTreeError::Cancelled => "the version probe was cancelled",
        OwnedProcessTreeError::OutputLimitExceeded(_) => {
            "the version probe output exceeded the diagnostic capture limit"
        }
        OwnedProcessTreeError::Await => "the version probe could not be awaited",
        OwnedProcessTreeError::Cleanup => {
            "the version probe process tree could not be cleaned up safely"
        }
    }
}

fn node_runtime_fix(required: NumericVersion) -> String {
    format!(
        "Activate Node {required} or newer with your version manager, then run `scripts/jig doctor`."
    )
}
