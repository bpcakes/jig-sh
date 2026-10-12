//! SQLx CLI version check.

use std::path::Path;

use jig_context::RepoContext;
use serde_json::json;

use super::cargo_sqlx::command_uses_cargo_sqlx;
use super::check::{DoctorCheck, check};
use super::environment::{DoctorEnvironment, DoctorProcessControl};
use super::programs::{required_command_programs, resolve_program};
use super::sqlx_driver::{SqlxDriver, SqlxDriverResolution, configured_sqlx_driver};
use super::sqlx_driver_probe::{SqlxProbeStyle, sqlx_probe_style, trusted_sqlx_probe_executable};
use super::toolchains::version_probe_stdout;
use super::version_authority::{
    NumericVersion, VersionSeries, cargo_sqlx_version_authority, parse_numeric_version,
};

pub(super) fn sqlx_cli_version_check(
    ctx: &RepoContext,
    environment: &DoctorEnvironment,
    process_control: DoctorProcessControl<'_>,
) -> Option<DoctorCheck> {
    if !ctx.sqlx_enabled() {
        return None;
    }
    let authority_path = ctx.root().join("Cargo.toml");
    let required = match cargo_sqlx_version_authority(&authority_path) {
        Ok(Some(required)) => required,
        Ok(None) => return None,
        Err(reason) => {
            return Some(
                check(
                    "sqlx_cli",
                    "SQLx CLI",
                    true,
                    false,
                    "invalid authority",
                    reason,
                )
                .with_fix(
                    "Use one numeric SQLx dependency line in the root Cargo.toml, then run `scripts/jig doctor`.",
                )
                .with_data(json!({ "authority": authority_path.display().to_string() })),
            );
        }
    };
    let command = ctx.command_for_key("sqlx_check_command").ok()?;
    let (program, style) = sqlx_cli_version_program(ctx.root(), command)?;
    let Some(resolution) =
        resolve_program(ctx.root(), &program, environment.search_path.as_deref())
    else {
        return Some(
            check(
                "sqlx_cli",
                "SQLx CLI",
                true,
                false,
                "missing",
                format!("SQLx CLI {required}.x is required, but {program} was not found on PATH"),
            )
            .with_fix(&sqlx_cli_version_fix(ctx, environment, required))
            .with_data(json!({
                "authority": authority_path.display().to_string(),
                "required": required.to_string(),
                "actual": null,
            })),
        );
    };
    let Some(executable) = trusted_sqlx_probe_executable(ctx.root(), &program, &resolution) else {
        return Some(
            check(
                "sqlx_cli",
                "SQLx CLI",
                true,
                false,
                "unverified",
                "Could not verify the SQLx CLI version because the configured executable is not a trusted bare PATH command",
            )
            .with_fix("Use a bare `sqlx` or `cargo sqlx` command, then rerun `scripts/jig doctor`.")
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
                "sqlx_cli",
                "SQLx CLI",
                true,
                false,
                "unverified",
                format!("Could not verify SQLx CLI {required}.x ({reason})"),
            )
            .with_fix("Run `scripts/jig doctor` again before starting database work.")
            .with_data(json!({
                "authority": authority_path.display().to_string(),
                "required": required.to_string(),
                "actual": null,
            })),
        );
    }
    let actual = match probe_sqlx_cli_version(
        &executable,
        style,
        ctx.root(),
        environment,
        process_control.cancellation,
    ) {
        Ok(actual) => actual,
        Err(reason) => {
            return Some(
                check(
                    "sqlx_cli",
                    "SQLx CLI",
                    true,
                    false,
                    "unverified",
                    format!("Could not verify SQLx CLI {required}.x ({reason})"),
                )
                .with_fix("Run `sqlx --version`, correct the installed CLI, then rerun `scripts/jig doctor`.")
                .with_data(json!({
                    "authority": authority_path.display().to_string(),
                    "required": required.to_string(),
                    "actual": null,
                })),
            );
        }
    };
    let compatible = required.contains(actual);
    let detail = if compatible {
        format!("SQLx CLI {actual} matches the required {required}.x line")
    } else {
        format!("SQLx CLI {actual} is installed, but this repository requires {required}.x")
    };
    let check = check(
        "sqlx_cli",
        "SQLx CLI",
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
        check.with_fix(&sqlx_cli_version_fix(ctx, environment, required))
    })
}

fn sqlx_cli_version_program(root: &Path, command: &str) -> Option<(String, SqlxProbeStyle)> {
    if command_uses_cargo_sqlx(command) {
        return Some(("cargo-sqlx".into(), SqlxProbeStyle::CargoSubcommand));
    }
    required_command_programs(root, command)
        .programs
        .into_iter()
        .find_map(|program| {
            sqlx_probe_style(&program.program).map(|style| (program.program, style))
        })
}

fn sqlx_cli_version_fix(
    ctx: &RepoContext,
    environment: &DoctorEnvironment,
    required: VersionSeries,
) -> String {
    let driver = ctx
        .command_for_key("sqlx_check_command")
        .ok()
        .map(|command| {
            configured_sqlx_driver(ctx.root(), command, environment.database_url.as_deref())
        })
        .and_then(|resolution| match resolution {
            SqlxDriverResolution::Known(requirement) => Some(requirement.driver),
            SqlxDriverResolution::Absent | SqlxDriverResolution::Indeterminate(_) => None,
        });
    match driver {
        Some(driver) => format!(
            "Install SQLx CLI {required}.x with {} support (`cargo install sqlx-cli --version ^{required} --force --no-default-features --features {}`), then run `scripts/jig doctor`.",
            driver.label(),
            match driver {
                SqlxDriver::Postgres => "rustls,postgres",
                SqlxDriver::Sqlite => "sqlite",
            }
        ),
        None => format!(
            "Install SQLx CLI {required}.x for the configured database driver, then run `scripts/jig doctor`."
        ),
    }
}

fn probe_sqlx_cli_version(
    executable: &Path,
    style: SqlxProbeStyle,
    root: &Path,
    environment: &DoctorEnvironment,
    cancellation: Option<&dyn Fn() -> bool>,
) -> std::result::Result<NumericVersion, String> {
    let arguments = match style {
        SqlxProbeStyle::CargoSubcommand => &["sqlx", "--version"][..],
        SqlxProbeStyle::Direct => &["--version"][..],
    };
    let stdout = version_probe_stdout(
        executable,
        arguments,
        "sqlx --version",
        root,
        environment,
        None,
        cancellation,
    )?;
    let mut tokens = stdout.split_ascii_whitespace();
    let product = tokens.next();
    // The Cargo entrypoint can report its own product name. Retain sqlx-cli
    // for installations that use the common name for both entrypoints.
    if product != Some("sqlx-cli")
        && !(style == SqlxProbeStyle::CargoSubcommand && product == Some("sqlx-cli-sqlx"))
    {
        return Err("sqlx --version returned an invalid product name".into());
    }
    let version = tokens
        .next()
        .and_then(|token| parse_numeric_version(token, false, false))
        .ok_or_else(|| "sqlx --version returned an invalid version".to_string())?;
    if tokens.next().is_some() {
        return Err("sqlx --version returned unexpected output".into());
    }
    Ok(version)
}
