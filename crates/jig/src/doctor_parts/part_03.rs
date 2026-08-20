
fn sqlx_cli_version_check(
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
    let Some(resolution) = resolve_program(
        ctx.root(),
        &program,
        environment.search_path.as_deref(),
        environment.path_extensions.as_deref(),
    ) else {
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

fn root_cargo_manifest(path: &Path) -> std::result::Result<Option<toml::Value>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(format!("Could not inspect {}", path.display())),
    };
    if !metadata.file_type().is_file() {
        return Err(format!("{} must be a real regular file", path.display()));
    }
    if metadata.len() == 0 || metadata.len() > CARGO_MANIFEST_AUTHORITY_MAX_BYTES {
        return Err(format!(
            "{} must be a non-empty bounded Cargo manifest",
            path.display()
        ));
    }
    let contents = fs::read_to_string(path)
        .map_err(|_| format!("{} must contain valid UTF-8", path.display()))?;
    toml::from_str::<toml::Value>(&contents)
        .map(Some)
        .map_err(|error| format!("Could not parse {}: {error}", path.display()))
}

fn cargo_rust_version_authority(
    path: &Path,
) -> std::result::Result<Option<NumericVersion>, String> {
    let Some(manifest) = root_cargo_manifest(path)? else {
        return Ok(None);
    };
    let value = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("rust-version"))
        .or_else(|| {
            manifest
                .get("package")
                .and_then(|package| package.get("rust-version"))
        });
    let Some(value) = value else {
        return Ok(None);
    };
    let version = value.as_str().ok_or_else(|| {
        format!(
            "Rust version authority in {} must be a string",
            path.display()
        )
    })?;
    parse_numeric_version(version, false, true)
        .map(Some)
        .ok_or_else(|| {
            format!(
                "Rust version authority in {} must be numeric, such as 1.94",
                path.display()
            )
        })
}

fn cargo_sqlx_version_authority(path: &Path) -> std::result::Result<Option<VersionSeries>, String> {
    let Some(manifest) = root_cargo_manifest(path)? else {
        return Ok(None);
    };
    let value = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(|dependencies| dependencies.get("sqlx"))
        .or_else(|| {
            manifest
                .get("dependencies")
                .and_then(|dependencies| dependencies.get("sqlx"))
        });
    let Some(value) = value else {
        return Ok(None);
    };
    let requirement = value
        .as_str()
        .or_else(|| value.get("version").and_then(toml::Value::as_str))
        .ok_or_else(|| {
            format!(
                "SQLx dependency authority in {} must declare a version",
                path.display()
            )
        })?;
    let requirement = requirement
        .strip_prefix('=')
        .or_else(|| requirement.strip_prefix('^'))
        .unwrap_or(requirement);
    let version = parse_numeric_version(requirement, false, true).ok_or_else(|| {
        format!(
            "SQLx dependency authority in {} must use one numeric minor line, such as 0.9",
            path.display()
        )
    })?;
    Ok(Some(VersionSeries {
        major: version.major,
        minor: version.minor,
    }))
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

fn rust_runtime_fix(required: NumericVersion) -> String {
    format!("Activate Rust {required} or newer with rustup, then run `scripts/jig doctor`.")
}

fn go_runtime_fix(required: NumericVersion) -> String {
    format!("Install or activate Go {required} or newer, then run `scripts/jig doctor`.")
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

fn node_runtime_check(
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
    let Some(resolution) = resolve_program(
        ctx.root(),
        "node",
        environment.search_path.as_deref(),
        environment.path_extensions.as_deref(),
    ) else {
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
    if tokens.next() != Some("sqlx-cli") {
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

fn version_probe_stdout(
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
        .env("USERPROFILE", home)
        .env("TMPDIR", temp.path())
        .env("TMP", temp.path())
        .env("TEMP", temp.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = sanitized_probe_search_path(root, executable) {
        command.env("PATH", path);
    }
    #[cfg(windows)]
    if let Some(path_extensions) = sanitized_windows_pathext(environment.path_extensions.as_deref())
    {
        command.env("PATHEXT", path_extensions);
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
        OwnedProcessTreeError::Cancelled => "the version probe was cancelled",
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

fn required_tools_fix(
    has_missing: bool,
    has_incompatible: bool,
    driver: Option<SqlxDriver>,
) -> Option<String> {
    let mut steps = Vec::new();
    if has_missing {
        steps
            .push("Install the missing executable or restore the missing repo script.".to_string());
    }
    if let Some(driver) = driver {
        let action = if has_incompatible {
            "Reinstall"
        } else {
            "Install"
        };
        steps.push(format!(
            "{action} SQLx CLI with {} support (for example, `{}`).",
            driver.label(),
            driver.install_command()
        ));
    }
    if steps.is_empty() {
        return None;
    }
    steps.push("Then run `scripts/jig doctor`.".to_string());
    Some(steps.join(" "))
}
