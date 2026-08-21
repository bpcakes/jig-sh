
#[cfg(unix)]
fn mark_doctor_signal_retirement_failure(ctx: &RepoContext, checks: &mut DoctorContextChecks) {
    if ctx.sqlx_enabled()
        && ctx
            .required_commands()
            .iter()
            .any(|command| command == "sqlx_check_command")
    {
        if checks.required_tools.status == "present" {
            checks.required_tools.status = "present_unverified".to_string();
        }
        checks.required_tools.detail.push_str(
            "; SQLx capability verification is incomplete because the process-wide doctor signal session could not retire safely",
        );
    }
    if let Some(node_runtime) = checks.node_runtime.as_mut() {
        if node_runtime.ok {
            node_runtime.ok = false;
            node_runtime.status = "unverified".to_string();
            node_runtime.detail.push_str(
                "; Node runtime verification is incomplete because the process-wide doctor signal session could not retire safely",
            );
            node_runtime.fix =
                Some("Run `scripts/jig doctor` again before starting frontend work.".into());
        }
    }
    for (runtime, label) in [
        (checks.rust_runtime.as_mut(), "Rust runtime"),
        (checks.go_runtime.as_mut(), "Go runtime"),
        (checks.sqlx_cli.as_mut(), "SQLx CLI"),
    ] {
        if let Some(runtime) = runtime {
            if runtime.ok {
                runtime.ok = false;
                runtime.status = "unverified".to_string();
                runtime.detail.push_str(&format!(
                    "; {label} verification is incomplete because the process-wide doctor signal session could not retire safely"
                ));
                runtime.fix =
                    Some("Run `scripts/jig doctor` again before starting database work.".into());
            }
        }
    }
    if !ctx.codex_marketplaces().is_empty() {
        checks.agent.ok = false;
        checks.agent.status = "error".to_string();
        checks.agent.detail.push_str(
            "; Codex marketplace verification is incomplete because the process-wide doctor signal session could not retire safely",
        );
        checks.agent.fix = Some("Run `scripts/jig agent doctor` for agent tooling details.".into());
    }
    if proxy_configured(ctx) {
        checks.proxy.ok = false;
        checks.proxy.status = "error".to_string();
        checks.proxy.detail.push_str(
            "; proxy diagnostics are incomplete because the process-wide doctor signal session could not retire safely",
        );
        checks.proxy.fix = Some("Run `scripts/jig proxy list` for proxy diagnostics.".into());
    }
}

#[cfg(test)]
fn required_tools_check_with_environment(
    ctx: &RepoContext,
    environment: &DoctorEnvironment,
) -> DoctorCheck {
    required_tools_check_with_environment_and_process_control(
        ctx,
        environment,
        DoctorProcessControl::allowed_without_signal_session(),
    )
}

fn required_tools_check_with_environment_and_process_control(
    ctx: &RepoContext,
    environment: &DoctorEnvironment,
    process_control: DoctorProcessControl<'_>,
) -> DoctorCheck {
    let mut tools = Vec::new();
    let mut missing = Vec::new();
    let mut incompatible = Vec::new();
    let mut indeterminate = Vec::new();
    let mut remediation_driver = None;
    let mut executable_reference_count = 0;
    for command_key in ctx.required_commands() {
        let command = match ctx.command_for_key(command_key) {
            Ok(command) => command,
            Err(error) => {
                missing.push(format!("{command_key}: {error}"));
                tools.push(json!({
                    "command_key": command_key,
                    "command": null,
                    "program": null,
                    "present": false,
                    "detail": error.to_string(),
                }));
                continue;
            }
        };
        let sqlx_driver = if command_key == "sqlx_check_command" && ctx.sqlx_enabled() {
            configured_sqlx_driver(ctx.root(), command, environment.database_url.as_deref())
        } else {
            SqlxDriverResolution::Absent
        };
        let program_discovery = required_command_programs(ctx.root(), command);
        let programs = &program_discovery.programs;
        let inherited_shell_issue = environment.shell_environment_issue;
        let cargo_sqlx_dispatch_issue = command_uses_cargo_sqlx(command)
            .then(|| cargo_sqlx_dispatch_issue(ctx.root(), command, environment));
        executable_reference_count += programs.len();
        let mut sqlx_probes = HashSet::new();
        let mut sqlx_resolution_recorded = false;
        let mut probed_programs = if programs.is_empty()
            && program_discovery.ambiguity.is_none()
            && inherited_shell_issue.is_none()
        {
            vec![json!({
                "program": null,
                "present": true,
                "detail": "No external executable required.",
            })]
        } else {
            programs
                .iter()
                .map(|program_spec| {
                    let program = &program_spec.program;
                    let presence = match &program_spec.path_lookup {
                        ProgramPathLookup::Explicit | ProgramPathLookup::Captured => {
                            match resolve_program(
                                ctx.root(),
                                program,
                                environment.search_path.as_deref(),
                                environment.path_extensions.as_deref(),
                            ) {
                                Some(resolution) => ProgramPresence::Present(resolution),
                                None => ProgramPresence::Missing,
                            }
                        }
                        ProgramPathLookup::CommandLocal(search_path) => {
                            match resolve_program(
                                ctx.root(),
                                program,
                                Some(search_path.as_os_str()),
                                environment.path_extensions.as_deref(),
                            ) {
                                Some(resolution) => ProgramPresence::Present(resolution),
                                None => ProgramPresence::Missing,
                            }
                        }
                        ProgramPathLookup::CapturedAfterCwdChange
                            if search_path_is_cwd_independent(
                                environment.search_path.as_deref(),
                            ) =>
                        {
                            match resolve_program(
                                ctx.root(),
                                program,
                                environment.search_path.as_deref(),
                                environment.path_extensions.as_deref(),
                            ) {
                                Some(resolution) => ProgramPresence::Present(resolution),
                                None => ProgramPresence::Missing,
                            }
                        }
                        ProgramPathLookup::CapturedAfterCwdChange
                        | ProgramPathLookup::Unverifiable => ProgramPresence::Unverified,
                    };
                    let resolved = match &presence {
                        ProgramPresence::Present(resolution) => Some(resolution),
                        ProgramPresence::Missing | ProgramPresence::Unverified => None,
                    };
                    let (reported_program, redact_program) =
                        reported_program(command_key, program);
                    let (present, detail) = match &presence {
                        ProgramPresence::Present(_) if redact_program => (
                            Some(true),
                            "A redacted command executable is present.".to_string(),
                        ),
                        ProgramPresence::Missing if redact_program => (
                            Some(false),
                            "A redacted command executable was not found.".to_string(),
                        ),
                        ProgramPresence::Present(resolution) => {
                            let (present, detail) = program_presence(
                                ctx.root(),
                                &reported_program,
                                Some(resolution.path.as_path()),
                            );
                            (Some(present), detail)
                        }
                        ProgramPresence::Missing => {
                            let (present, detail) =
                                program_presence(ctx.root(), &reported_program, None);
                            (Some(present), detail)
                        }
                        ProgramPresence::Unverified => (
                            None,
                            if redact_program {
                                "A redacted command executable could not be resolved safely because the configured Bash command may change the executable lookup context before this invocation."
                                    .to_string()
                            } else {
                                format!(
                                    "{reported_program} could not be resolved safely because the configured Bash command may change the executable lookup context before this invocation"
                                )
                            },
                        ),
                    };
                    if present == Some(false) {
                        missing.push(format!("{command_key}: {reported_program}"));
                    }
                    let mut report = json!({
                        "program": reported_program,
                        "present": present,
                        "detail": detail,
                    });

                    let probe_style = sqlx_probe_style(program);
                    if matches!(presence, ProgramPresence::Unverified) {
                        let detail = format!(
                            "{command_key}: {reported_program} executable lookup could not be verified because the configured Bash command may change the executable lookup context before this invocation"
                        );
                        if command_key == "sqlx_check_command"
                            && (program_spec.cargo_sqlx_dispatch || probe_style.is_some())
                        {
                            sqlx_resolution_recorded = true;
                            let probe_detail = format!(
                                "{detail}; SQLx CLI capability probing was skipped; run `scripts/jig check sqlx`"
                            );
                            indeterminate.push(probe_detail.clone());
                            report["driver_probe"] = match sqlx_driver {
                                SqlxDriverResolution::Known(requirement) => json!({
                                    "driver": requirement.driver.key(),
                                    "source": requirement.source.key(),
                                    "status": "unverified",
                                    "compatible": null,
                                    "detail": probe_detail,
                                }),
                                _ => json!({
                                    "driver": null,
                                    "source": null,
                                    "status": "unverified",
                                    "compatible": null,
                                    "detail": probe_detail,
                                }),
                            };
                        } else {
                            indeterminate.push(detail);
                        }
                        return report;
                    }
                    let sqlx_inherited_shell_issue = (command_key == "sqlx_check_command"
                        && present == Some(true)
                        && (program_spec.cargo_sqlx_dispatch || probe_style.is_some()))
                    .then_some(environment.shell_environment_issue)
                    .flatten();
                    if let Some(issue) = sqlx_inherited_shell_issue {
                        let detail = format!(
                            "{command_key}: SQLx CLI capability probing was skipped because inherited shell state ({}) can alter the configured Bash command; run `scripts/jig check sqlx`",
                            issue.description(),
                        );
                        sqlx_resolution_recorded = true;
                        report["driver_probe"] = json!({
                            "driver": null,
                            "source": null,
                            "status": "unverified",
                            "compatible": null,
                            "detail": detail,
                        });
                        return report;
                    }

                    if program_spec.cargo_sqlx_dispatch {
                        let reason = cargo_sqlx_dispatch_issue.unwrap_or(
                            "Cargo subcommand dispatch cannot be verified without executing cargo",
                        );
                        let detail = format!(
                            "{command_key}: cargo sqlx dispatch could not be verified ({reason}); run `scripts/jig check sqlx`"
                        );
                        if !sqlx_resolution_recorded {
                            indeterminate.push(detail.clone());
                        }
                        sqlx_resolution_recorded = true;
                        report["driver_probe"] = match sqlx_driver {
                            SqlxDriverResolution::Known(requirement) => json!({
                                "driver": requirement.driver.key(),
                                "source": requirement.source.key(),
                                "status": "unverified",
                                "compatible": null,
                                "detail": detail,
                            }),
                            _ => json!({
                                "driver": null,
                                "source": null,
                                "status": "unverified",
                                "compatible": null,
                                "detail": detail,
                            }),
                        };
                        return report;
                    }

                    if command_key == "sqlx_check_command" && present == Some(true) {
                        match (probe_style, sqlx_driver) {
                            (Some(probe_style), SqlxDriverResolution::Known(requirement)) => {
                                let trusted_executable = resolved.as_ref().and_then(|resolution| {
                                    trusted_sqlx_probe_executable(
                                        ctx.root(),
                                        program,
                                        resolution,
                                    )
                                });
                                let Some(executable) = trusted_executable else {
                                    if !sqlx_resolution_recorded {
                                        sqlx_resolution_recorded = true;
                                        let detail = format!(
                                            "{command_key}: SQLx CLI capability probing was skipped because the configured executable is not a trusted bare PATH command; run `scripts/jig check sqlx`"
                                        );
                                        indeterminate.push(detail.clone());
                                        report["driver_probe"] = json!({
                                            "driver": requirement.driver.key(),
                                            "source": requirement.source.key(),
                                            "status": "unverified",
                                            "compatible": null,
                                            "detail": detail,
                                        });
                                    }
                                    return report;
                                };
                                if let Some(reason) = process_control.unavailable_reason {
                                    let detail = format!(
                                        "{command_key}: could not verify the {} driver in the SQLx CLI ({reason}); run `scripts/jig check sqlx`",
                                        requirement.driver.label()
                                    );
                                    indeterminate.push(detail.clone());
                                    sqlx_resolution_recorded = true;
                                    report["driver_probe"] = json!({
                                        "driver": requirement.driver.key(),
                                        "source": requirement.source.key(),
                                        "status": "unverified",
                                        "compatible": null,
                                        "detail": detail,
                                    });
                                    return report;
                                }
                                let probe_key = (executable.clone(), probe_style);
                                if !sqlx_probes.insert(probe_key) {
                                    return report;
                                }
                                let probe = probe_sqlx_driver(
                                    &executable,
                                    probe_style,
                                    requirement.driver,
                                    ctx.root(),
                                    environment,
                                    process_control.cancellation,
                                );
                                let (status, compatible, probe_detail) = match &probe {
                                    SqlxDriverProbe::Compatible => (
                                        "compatible",
                                        Some(true),
                                        format!(
                                            "SQLx CLI supports the {}",
                                            requirement.description()
                                        ),
                                    ),
                                    SqlxDriverProbe::Incompatible => {
                                        remediation_driver = Some(requirement.driver);
                                        let detail = format!(
                                            "{command_key}: SQLx CLI is installed but lacks the {}",
                                            requirement.description()
                                        );
                                        incompatible.push(detail.clone());
                                        ("missing_driver", Some(false), detail)
                                    }
                                    SqlxDriverProbe::Indeterminate(reason) => {
                                        let detail = format!(
                                            "{command_key}: could not verify the {} driver in the SQLx CLI ({reason}); run `scripts/jig check sqlx`",
                                            requirement.driver.label()
                                        );
                                        indeterminate.push(detail.clone());
                                        ("unverified", None, detail)
                                    }
                                };
                                report["driver_probe"] = json!({
                                    "driver": requirement.driver.key(),
                                    "source": requirement.source.key(),
                                    "status": status,
                                    "compatible": compatible,
                                    "detail": probe_detail,
                                });
                            }
                            (Some(_), SqlxDriverResolution::Indeterminate(reason))
                                if !sqlx_resolution_recorded =>
                            {
                                sqlx_resolution_recorded = true;
                                let detail = format!(
                                    "{command_key}: could not determine the required SQLx driver ({reason}); run `scripts/jig check sqlx`"
                                );
                                indeterminate.push(detail.clone());
                                report["driver_probe"] = json!({
                                    "driver": null,
                                    "source": null,
                                    "status": "unverified",
                                    "compatible": null,
                                    "detail": detail,
                                });
                            }
                            _ => {}
                        }
                    }

                    report
                })
                .collect()
        };
        if let Some(ambiguity) = program_discovery.ambiguity {
            let detail = format!(
                "{command_key}: executable discovery is incomplete because {}; the configured command must be run to verify its tools",
                ambiguity.description(),
            );
            indeterminate.push(detail.clone());
            probed_programs.push(json!({
                "program": null,
                "present": null,
                "detail": detail,
            }));
        }
        if let Some(issue) = inherited_shell_issue {
            let detail = format!(
                "{command_key}: command execution is unverified because inherited shell state ({}) can alter the configured Bash command",
                issue.description(),
            );
            indeterminate.push(detail.clone());
            probed_programs.push(json!({
                "program": null,
                "present": null,
                "detail": detail,
            }));
        }
        if command_key == "sqlx_check_command" && !sqlx_resolution_recorded {
            if let SqlxDriverResolution::Indeterminate(reason) = sqlx_driver {
                let detail = format!(
                    "{command_key}: could not determine the required SQLx driver ({reason}); run `scripts/jig check sqlx`"
                );
                indeterminate.push(detail);
            }
        }
        let any_missing = probed_programs
            .iter()
            .any(|program| program["present"].as_bool() == Some(false));
        let any_unverified = probed_programs
            .iter()
            .any(|program| program["present"].is_null());
        let command_present = if any_missing {
            Some(false)
        } else if any_unverified {
            None
        } else {
            Some(true)
        };
        let reported_command = format!("<redacted: {command_key}>");
        tools.push(json!({
            "command_key": command_key,
            "command": reported_command,
            "command_redacted": true,
            "programs": probed_programs,
            "present": command_present,
        }));
    }

    let ok = missing.is_empty() && incompatible.is_empty();
    let status = match (missing.is_empty(), incompatible.is_empty()) {
        (true, true) if indeterminate.is_empty() => "present",
        (true, true) => "present_unverified",
        (false, true) => "missing",
        (true, false) => "incompatible",
        (false, false) => "unavailable",
    };
    let checked_detail = format!(
        "{} required command(s) checked; {} external executable reference(s) inspected",
        tools.len(),
        executable_reference_count
    );
    let detail = if ok && indeterminate.is_empty() {
        checked_detail
    } else if ok {
        format!(
            "{checked_detail}; Present but unverified: {}",
            indeterminate.join(", ")
        )
    } else {
        let mut details = Vec::new();
        if !missing.is_empty() {
            details.push(format!(
                "Missing command executable(s): {}",
                missing.join(", ")
            ));
        }
        if !incompatible.is_empty() {
            details.push(format!(
                "Incompatible command executable(s): {}",
                incompatible.join(", ")
            ));
        }
        if !indeterminate.is_empty() {
            details.push(format!(
                "Unverified command executable(s): {}",
                indeterminate.join(", ")
            ));
        }
        details.join("; ")
    };
    let fix = required_tools_fix(
        !missing.is_empty(),
        !incompatible.is_empty(),
        remediation_driver,
    );

    check("required_tools", "Required tools", true, ok, status, detail)
        .with_optional_fix(fix.as_deref())
        .with_data(json!({ "tools": tools }))
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct NumericVersion {
    major: u64,
    minor: u64,
    patch: u64,
}

impl std::fmt::Display for NumericVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

fn parse_numeric_version(
    value: &str,
    allow_v_prefix: bool,
    allow_missing_patch: bool,
) -> Option<NumericVersion> {
    let value = if allow_v_prefix {
        value.strip_prefix('v').unwrap_or(value)
    } else {
        value
    };
    let mut components = value.split('.');
    let major = parse_numeric_version_component(components.next()?)?;
    let minor = parse_numeric_version_component(components.next()?)?;
    let patch = match components.next() {
        Some(patch) => parse_numeric_version_component(patch)?,
        None if allow_missing_patch => 0,
        None => return None,
    };
    components.next().is_none().then_some(NumericVersion {
        major,
        minor,
        patch,
    })
}

fn parse_numeric_version_component(value: &str) -> Option<u64> {
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return None;
    }
    value.parse().ok()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AuthorityReadError {
    Inspect,
    NotRegular,
    EmptyOrOversized,
    Read,
    InvalidUtf8,
}

fn read_bounded_authority(
    path: &Path,
    max_bytes: u64,
) -> std::result::Result<Option<String>, AuthorityReadError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(AuthorityReadError::Inspect),
    };
    if !metadata.file_type().is_file() {
        return Err(AuthorityReadError::NotRegular);
    }
    if metadata.len() == 0 || metadata.len() > max_bytes {
        return Err(AuthorityReadError::EmptyOrOversized);
    }
    let file = fs::File::open(path).map_err(|_| AuthorityReadError::Read)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| AuthorityReadError::Read)?;
    if bytes.is_empty() || bytes.len() as u64 > max_bytes {
        return Err(AuthorityReadError::EmptyOrOversized);
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| AuthorityReadError::InvalidUtf8)
}

fn numeric_version_authority(
    path: &Path,
    product: &str,
    allow_missing_patch: bool,
    example: &str,
) -> std::result::Result<Option<NumericVersion>, String> {
    let contents = read_bounded_authority(path, VERSION_AUTHORITY_MAX_BYTES)
        .map_err(|error| match error {
            AuthorityReadError::Inspect => format!(
                "Could not inspect the {product} version authority at {}",
                path.display()
            ),
            AuthorityReadError::NotRegular => format!(
                "{product} version authority {} must be a real regular file",
                path.display()
            ),
            AuthorityReadError::EmptyOrOversized => format!(
                "{product} version authority {} must contain exactly one bounded version token",
                path.display()
            ),
            AuthorityReadError::Read => format!(
                "Could not read the {product} version authority at {}",
                path.display()
            ),
            AuthorityReadError::InvalidUtf8 => format!(
                "{product} version authority {} must contain valid UTF-8",
                path.display()
            ),
        })?;
    let Some(contents) = contents else {
        return Ok(None);
    };
    let mut tokens = contents.split_ascii_whitespace();
    let Some(token) = tokens.next() else {
        return Err(format!(
            "{product} version authority {} is empty",
            path.display()
        ));
    };
    if tokens.next().is_some() {
        return Err(format!(
            "{product} version authority {} must contain exactly one version token",
            path.display()
        ));
    }
    parse_numeric_version(token, false, allow_missing_patch)
        .map(Some)
        .ok_or_else(|| {
            format!(
                "{product} version authority {} must contain an exact numeric version such as {example}",
                path.display()
            )
        })
}

fn go_module_version_authority(
    path: &Path,
) -> std::result::Result<Option<NumericVersion>, String> {
    let contents = read_bounded_authority(path, GO_MODULE_AUTHORITY_MAX_BYTES)
        .map_err(|error| match error {
            AuthorityReadError::Inspect => {
                format!("Could not inspect Go module {}", path.display())
            }
            AuthorityReadError::NotRegular => format!(
                "Go module authority {} must be a real regular file",
                path.display()
            ),
            AuthorityReadError::EmptyOrOversized => format!(
                "Go module authority {} must be a non-empty bounded go.mod file",
                path.display()
            ),
            AuthorityReadError::Read => {
                format!("Could not read Go module authority at {}", path.display())
            }
            AuthorityReadError::InvalidUtf8 => format!(
                "Go module authority {} must contain valid UTF-8",
                path.display()
            ),
        })?;
    let Some(contents) = contents else {
        return Ok(None);
    };

    let mut go_version = None;
    let mut toolchain_version = None;
    let mut toolchain_seen = false;
    for raw_line in contents.lines() {
        let line = raw_line.split_once("//").map_or(raw_line, |(code, _)| code);
        let mut tokens = line.split_ascii_whitespace();
        let Some(directive) = tokens.next() else {
            continue;
        };
        if !matches!(directive, "go" | "toolchain") {
            continue;
        }
        let token = tokens.next().ok_or_else(|| {
            format!(
                "Go module authority {} has an empty {directive} directive",
                path.display()
            )
        })?;
        if tokens.next().is_some() {
            return Err(format!(
                "Go module authority {} has an invalid {directive} directive",
                path.display()
            ));
        }
        let token = unquote_go_module_token(token).ok_or_else(|| {
            format!(
                "Go module authority {} has an invalid {directive} version token",
                path.display()
            )
        })?;
        match directive {
            "go" => {
                if go_version.is_some() {
                    return Err(format!(
                        "Go module authority {} declares the go version more than once",
                        path.display()
                    ));
                }
                go_version = Some(parse_go_module_version(token, "go", path)?);
            }
            "toolchain" => {
                if toolchain_seen {
                    return Err(format!(
                        "Go module authority {} declares the toolchain more than once",
                        path.display()
                    ));
                }
                toolchain_seen = true;
                toolchain_version = if token == "default" {
                    None
                } else {
                    let version = token.strip_prefix("go").ok_or_else(|| {
                        format!(
                            "Go module authority {} toolchain must be default or an exact go version such as go1.26.0",
                            path.display()
                        )
                    })?;
                    Some(parse_go_module_version(version, "toolchain", path)?)
                };
            }
            _ => unreachable!(),
        }
    }

    let go_version = go_version.ok_or_else(|| {
        format!(
            "Go module authority {} must declare one numeric go version such as go 1.26.0",
            path.display()
        )
    })?;
    if let Some(toolchain_version) = toolchain_version {
        if toolchain_version < go_version {
            return Err(format!(
                "Go module authority {} declares toolchain {toolchain_version} below required go version {go_version}",
                path.display()
            ));
        }
        Ok(Some(toolchain_version))
    } else {
        Ok(Some(go_version))
    }
}

fn unquote_go_module_token(token: &str) -> Option<&str> {
    if token.starts_with('"') || token.ends_with('"') {
        token
            .strip_prefix('"')
            .and_then(|token| token.strip_suffix('"'))
            .filter(|token| !token.is_empty())
    } else {
        Some(token)
    }
}

fn parse_go_module_version(
    token: &str,
    directive: &str,
    path: &Path,
) -> std::result::Result<NumericVersion, String> {
    parse_numeric_version(token, false, true).ok_or_else(|| {
        format!(
            "Go module authority {} {directive} directive must use an exact numeric version such as 1.26.0",
            path.display()
        )
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VersionSeries {
    major: u64,
    minor: u64,
}

impl VersionSeries {
    const fn contains(self, version: NumericVersion) -> bool {
        self.major == version.major && self.minor == version.minor
    }
}

impl std::fmt::Display for VersionSeries {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}", self.major, self.minor)
    }
}

fn rust_runtime_check(
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
    let Some(resolution) = resolve_program(
        ctx.root(),
        "rustc",
        environment.search_path.as_deref(),
        environment.path_extensions.as_deref(),
    ) else {
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

fn go_runtime_check(
    ctx: &RepoContext,
    environment: &DoctorEnvironment,
    process_control: DoctorProcessControl<'_>,
) -> Option<DoctorCheck> {
    if !ctx.is_go_backend() {
        return None;
    }
    let authority_path = ctx.root().join(GO_TOOLCHAIN_AUTHORITY_PATH);
    let required = match go_module_version_authority(&authority_path) {
        Ok(Some(required)) => required,
        Ok(None) => {
            return Some(
                check(
                    "go_runtime",
                    "Go runtime",
                    true,
                    false,
                    "invalid authority",
                    format!(
                        "Go version authority {} is missing",
                        authority_path.display()
                    ),
                )
                .with_fix("Restore the root go.mod with a numeric Go directive such as `go 1.26.0`.")
                .with_data(json!({ "authority": authority_path.display().to_string() })),
            );
        }
        Err(reason) => {
            return Some(
                check(
                    "go_runtime",
                    "Go runtime",
                    true,
                    false,
                    "invalid authority",
                    reason,
                )
                .with_fix("Correct the root go.mod Go/toolchain directives, then rerun scripts/jig doctor.")
                .with_data(json!({ "authority": authority_path.display().to_string() })),
            );
        }
    };
    let Some(resolution) = resolve_program(
        ctx.root(),
        "go",
        environment.search_path.as_deref(),
        environment.path_extensions.as_deref(),
    ) else {
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
        "required": required.to_string(),
        "actual": actual.to_string(),
    }));
    Some(if compatible {
        result
    } else {
        result.with_fix(&go_runtime_fix(required))
    })
}
