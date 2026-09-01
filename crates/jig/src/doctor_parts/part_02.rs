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
    if let Some(node_runtime) = checks.node_runtime.as_mut()
        && node_runtime.ok
    {
        node_runtime.ok = false;
        node_runtime.status = "unverified".to_string();
        node_runtime.detail.push_str(
                "; Node runtime verification is incomplete because the process-wide doctor signal session could not retire safely",
            );
        node_runtime.fix =
            Some("Run `scripts/jig doctor` again before starting frontend work.".into());
    }
    for (runtime, label) in [
        (checks.rust_runtime.as_mut(), "Rust runtime"),
        (checks.go_runtime.as_mut(), "Go runtime"),
        (checks.sqlx_cli.as_mut(), "SQLx CLI"),
    ] {
        if let Some(runtime) = runtime
            && runtime.ok
        {
            runtime.ok = false;
            runtime.status = "unverified".to_string();
            runtime.detail.push_str(&format!(
                    "; {label} verification is incomplete because the process-wide doctor signal session could not retire safely"
                ));
            runtime.fix =
                Some("Run `scripts/jig doctor` again before starting database work.".into());
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

fn resolve_required_program(
    root: &Path,
    program: &RequiredProgram,
    captured_search_path: Option<&std::ffi::OsStr>,
) -> ProgramPresence {
    let search_path = match &program.path_lookup {
        ProgramPathLookup::Explicit | ProgramPathLookup::Captured => captured_search_path,
        ProgramPathLookup::CommandLocal(search_path) => Some(search_path.as_os_str()),
        ProgramPathLookup::CapturedAfterCwdChange
            if search_path_is_cwd_independent(captured_search_path) =>
        {
            captured_search_path
        }
        ProgramPathLookup::CapturedAfterCwdChange | ProgramPathLookup::Unverifiable => {
            return ProgramPresence::Unverified;
        }
    };
    resolve_program(root, &program.program, search_path)
        .map_or(ProgramPresence::Missing, ProgramPresence::Present)
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
                    let presence = resolve_required_program(
                        ctx.root(),
                        program_spec,
                        environment.search_path.as_deref(),
                    );
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
        if command_key == "sqlx_check_command"
            && !sqlx_resolution_recorded
            && let SqlxDriverResolution::Indeterminate(reason) = sqlx_driver
        {
            let detail = format!(
                "{command_key}: could not determine the required SQLx driver ({reason}); run `scripts/jig check sqlx`"
            );
            indeterminate.push(detail);
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

mod version_checks;
use version_checks::*;
