
use super::*;
#[cfg(windows)]
use crate::types::{AppKind, AppRunSpec, CommandSpec, ProxySettings};
use tempfile::tempdir;

fn environment(path: &Path, path_extensions: &str) -> WindowsEnvironment {
    WindowsEnvironment {
        path: path.as_os_str().to_os_string(),
        path_extensions: OsString::from(path_extensions),
        command_interpreter: Some(test_command_interpreter()),
    }
}

fn test_command_interpreter() -> OsString {
    #[cfg(windows)]
    {
        fs::canonicalize(crate::windows_system::native_system_executable("cmd.exe").unwrap())
            .unwrap()
            .into_os_string()
    }
    #[cfg(not(windows))]
    {
        OsString::from(r"C:\Windows\System32\cmd.exe")
    }
}

#[cfg(not(windows))]
#[test]
fn missing_comspec_uses_injected_native_system_command_interpreter() {
    let command_interpreter = resolve_command_interpreter_with(None, || {
        Ok(PathBuf::from(r"D:\NativeWindows\System32\cmd.exe"))
    })
    .unwrap();

    assert_eq!(
        command_interpreter,
        OsString::from(r"D:\NativeWindows\System32\cmd.exe")
    );
}

#[cfg(not(windows))]
#[test]
fn missing_comspec_ignores_injected_cwd_and_path_cmd_shadows() {
    let temp = tempdir().unwrap();
    let working_directory = temp.path().join("working");
    let path_directory = temp.path().join("path");
    fs::create_dir_all(&working_directory).unwrap();
    fs::create_dir_all(&path_directory).unwrap();
    fs::write(path_directory.join("pnpm.CMD"), "shim").unwrap();
    fs::write(working_directory.join("cmd.exe"), "cwd shadow").unwrap();
    fs::write(path_directory.join("cmd.exe"), "PATH shadow").unwrap();
    let environment = WindowsEnvironment {
        path: path_directory.into_os_string(),
        path_extensions: OsString::from(".CMD"),
        command_interpreter: None,
    };
    let native = PathBuf::from(r"D:\NativeWindows\System32\cmd.exe");

    let plan = plan_windows_command_with_interpreter(
        &["pnpm".into(), "dev".into()],
        &working_directory,
        &environment,
        |command_interpreter| {
            resolve_command_interpreter_with(command_interpreter, || Ok(native.clone()))
        },
    )
    .unwrap();

    let WindowsCommandPlan::CommandInterpreter {
        command_interpreter,
        ..
    } = plan
    else {
        panic!("command shim was not dispatched through the native interpreter");
    };
    assert_eq!(command_interpreter, native.into_os_string());
}

#[test]
fn missing_comspec_preserves_native_system_lookup_failure() {
    let error = resolve_command_interpreter_with(None, || {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "injected system-directory denial",
        ))
    })
    .unwrap_err();

    assert!(error.to_string().contains("native Windows cmd.exe"));
    assert!(
        error
            .chain()
            .any(|cause| cause.to_string() == "injected system-directory denial")
    );
}

#[test]
fn drive_relative_windows_commands_are_rejected_before_candidate_inspection() {
    for requested in [r"C:node", r"c:tools\node.exe", "D:tools/node", "Z:"] {
        let error = resolve_windows_executable_with(
            OsStr::new(requested),
            Path::new(r"C:\repo"),
            OsStr::new(r"C:\tools"),
            OsStr::new(".EXE;.CMD"),
            |_| panic!("drive-relative commands must fail before candidate inspection"),
        )
        .unwrap_err();

        assert!(
            error.to_string().contains("drive-relative path"),
            "unexpected error for {requested:?}: {error:#}"
        );
    }
}

#[test]
fn drive_relative_classifier_preserves_supported_command_forms() {
    for drive_relative in [r"C:node", r"c:tools\node.exe", "D:tools/node", "Z:"] {
        assert!(is_drive_relative_windows_path(drive_relative));
    }
    for supported in [
        "node",
        r".\node.exe",
        r"C:\node.exe",
        "C:/node.exe",
        r"\\server\share\node.exe",
        r"\node.exe",
    ] {
        assert!(
            !is_drive_relative_windows_path(supported),
            "supported command was classified as drive-relative: {supported:?}"
        );
    }
}

#[test]
fn explicit_comspec_must_be_an_absolute_safe_windows_path() {
    for invalid in [
        "",
        "cmd.exe",
        r".\cmd.exe",
        r"C:cmd.exe",
        "C:\\Windows\\cmd.exe\nignored",
        "C:\\Windows\\\"cmd.exe",
        r"\\.\C:\Windows\System32\cmd.exe",
        r"\\?\relative\cmd.exe",
    ] {
        let error = resolve_command_interpreter_with(Some(OsStr::new(invalid)), || {
            panic!("an explicit ComSpec must not consult the native fallback")
        })
        .unwrap_err();
        assert!(
            error.to_string().contains("ComSpec"),
            "unexpected error for {invalid:?}: {error:#}"
        );
    }
}

#[cfg(not(windows))]
#[test]
fn absolute_windows_comspec_forms_are_accepted_by_cross_target_validation() {
    for valid in [
        r"C:\Windows\System32\cmd.exe",
        r"C:/Windows/System32/cmd.exe",
        r"\\server\share\cmd.exe",
        r"\\?\C:\Windows\System32\cmd.exe",
        r"\\?\UNC\server\share\cmd.exe",
    ] {
        assert_eq!(
            resolve_command_interpreter_with(Some(OsStr::new(valid)), || unreachable!()).unwrap(),
            OsString::from(valid)
        );
    }
}

#[cfg(windows)]
#[test]
fn missing_comspec_resolves_to_native_system_cmd_on_windows() {
    let expected = crate::windows_system::native_system_executable("cmd.exe").unwrap();
    let resolved = resolve_command_interpreter(None).unwrap();

    assert_eq!(PathBuf::from(resolved), fs::canonicalize(expected).unwrap());
}

#[cfg(windows)]
#[test]
fn missing_comspec_ignores_native_cwd_and_path_cmd_shadows() {
    let temp = tempdir().unwrap();
    let working_directory = temp.path().join("working");
    let path_directory = temp.path().join("path");
    fs::create_dir_all(&working_directory).unwrap();
    fs::create_dir_all(&path_directory).unwrap();
    fs::write(path_directory.join("pnpm.CMD"), "@exit /b 0\r\n").unwrap();
    fs::write(working_directory.join("cmd.exe"), "cwd shadow").unwrap();
    fs::write(path_directory.join("cmd.exe"), "PATH shadow").unwrap();
    let environment = WindowsEnvironment {
        path: path_directory.into_os_string(),
        path_extensions: OsString::from(".CMD"),
        command_interpreter: None,
    };

    let plan = plan_windows_command(
        &["pnpm".into(), "dev".into()],
        &working_directory,
        &environment,
    )
    .unwrap();

    let WindowsCommandPlan::CommandInterpreter {
        command_interpreter,
        ..
    } = plan
    else {
        panic!("command shim was not dispatched through the native interpreter");
    };
    assert_eq!(command_interpreter, test_command_interpreter());
}

fn expected_native_launch_path(path: PathBuf) -> PathBuf {
    fs::canonicalize(path).unwrap()
}

fn expected_command_shim_path(path: PathBuf) -> PathBuf {
    command_shim_launch_path(&path).unwrap()
}

#[test]
fn resolves_bare_command_shims_through_path_and_pathext() {
    let temp = tempdir().unwrap();
    let shim = temp.path().join("pnpm.CMD");
    fs::write(&shim, "shim").unwrap();

    let plan = plan_windows_command(
        &["pnpm".into(), "run".into(), "dev".into()],
        temp.path(),
        &environment(temp.path(), ".EXE;.CMD"),
    )
    .unwrap();

    let resolved = expected_command_shim_path(shim);
    assert_eq!(
        plan,
        WindowsCommandPlan::CommandInterpreter {
            command_interpreter: test_command_interpreter(),
            command_line: format!("\"\"{}\" run dev\"", resolved.display()),
        }
    );
}

#[test]
fn native_executables_remain_direct() {
    let temp = tempdir().unwrap();
    let native = temp.path().join("node.EXE");
    fs::write(&native, "native").unwrap();

    let plan = plan_windows_command(
        &["node".into(), "server.js".into()],
        temp.path(),
        &environment(temp.path(), ".EXE;.CMD"),
    )
    .unwrap();

    assert_eq!(
        plan,
        WindowsCommandPlan::Direct(expected_native_launch_path(native).into_os_string())
    );
}

#[test]
fn resolved_native_executable_uses_canonical_path_not_legacy_launch_spelling() {
    let canonical = PathBuf::from(format!(
        r"\\?\C:\{}\node.exe",
        "canonical-segment-".repeat(20)
    ));
    let plan = plan_resolved_windows_command(
        ResolvedWindowsExecutable {
            launch_path: PathBuf::from(r"C:\short-alias\node.exe"),
            canonical_path: canonical.clone(),
        },
        &["server.js".into()],
        &environment(Path::new(r"C:\tools"), ".EXE;.CMD"),
    )
    .unwrap();

    assert_eq!(plan, WindowsCommandPlan::Direct(canonical.into_os_string()));
}

#[test]
fn resolved_command_shim_uses_legacy_launch_spelling_not_canonical_target() {
    let canonical = PathBuf::from(format!(
        r"\\?\C:\{}\pnpm.cmd",
        "canonical-segment-".repeat(20)
    ));
    let plan = plan_resolved_windows_command(
        ResolvedWindowsExecutable {
            launch_path: PathBuf::from(r"C:\short-alias\pnpm.cmd"),
            canonical_path: canonical.clone(),
        },
        &["dev".into()],
        &environment(Path::new(r"C:\tools"), ".EXE;.CMD"),
    )
    .unwrap();

    let WindowsCommandPlan::CommandInterpreter { command_line, .. } = plan else {
        panic!("command shim was not dispatched through cmd.exe");
    };
    assert!(command_line.contains(r"C:\short-alias\pnpm.cmd"));
    assert!(!command_line.contains(&canonical.display().to_string()));
}

#[test]
fn explicit_extensionless_command_prefers_the_literal_file() {
    let temp = tempdir().unwrap();
    let literal = temp.path().join("astro");
    let shim = temp.path().join("astro.CMD");
    fs::write(&literal, "native").unwrap();
    fs::write(shim, "shim").unwrap();

    let plan = plan_windows_command(
        &[literal.display().to_string()],
        temp.path(),
        &environment(temp.path(), ".COM;.CMD"),
    )
    .unwrap();

    assert_eq!(
        plan,
        WindowsCommandPlan::Direct(expected_native_launch_path(literal).into_os_string())
    );
}

#[test]
fn explicit_extensionless_command_skips_missing_pathext_candidates() {
    let temp = tempdir().unwrap();
    let requested = temp.path().join("astro");
    let shim = temp.path().join("astro.CMD");
    fs::write(&shim, "shim").unwrap();

    let plan = plan_windows_command(
        &[requested.display().to_string(), "dev".into()],
        temp.path(),
        &environment(temp.path(), ".COM;.EXE;.BAT;.CMD"),
    )
    .unwrap();

    let resolved = expected_command_shim_path(shim);
    assert_eq!(
        plan,
        WindowsCommandPlan::CommandInterpreter {
            command_interpreter: test_command_interpreter(),
            command_line: format!("\"\"{}\" dev\"", resolved.display()),
        }
    );
}

fn assert_actionable_unresolved_command_error(error: &anyhow::Error, requested: &str) {
    let message = error.to_string();
    assert!(
        message.contains(&format!("`{requested}`")),
        "resolution error did not identify the requested command: {error:#}"
    );
    assert!(
        message.contains("existing regular executable"),
        "resolution error did not explain the required executable: {error:#}"
    );
    assert!(
        message.contains(".cmd/.bat shim") && message.contains("PATH"),
        "resolution error did not provide actionable search guidance: {error:#}"
    );
}

#[test]
fn skipped_only_drive_relative_path_fails_closed() {
    let temp = tempdir().unwrap();
    let environment = WindowsEnvironment {
        path: OsString::from("D:ambient"),
        path_extensions: OsString::from(".EXE;.CMD"),
        command_interpreter: Some(test_command_interpreter()),
    };

    let error = plan_windows_command(&["node".into()], temp.path(), &environment).unwrap_err();

    assert_actionable_unresolved_command_error(&error, "node");
}

#[test]
fn skipped_drive_relative_and_missing_path_entries_fail_closed() {
    let temp = tempdir().unwrap();
    let environment = WindowsEnvironment {
        path: OsString::from("D:ambient;missing-tools"),
        path_extensions: OsString::from(".EXE"),
        command_interpreter: Some(test_command_interpreter()),
    };
    let mut inspected = Vec::new();
    let resolved = resolve_windows_executable_with(
        OsStr::new("node"),
        temp.path(),
        &environment.path,
        &environment.path_extensions,
        |candidate| {
            inspected.push(candidate.to_path_buf());
            Ok(None)
        },
    )
    .unwrap();

    assert_eq!(resolved, None);
    assert_eq!(
        inspected,
        vec![temp.path().join("missing-tools").join("node.EXE")]
    );

    let error = plan_windows_command(&["node".into()], temp.path(), &environment).unwrap_err();
    assert_actionable_unresolved_command_error(&error, "node");
}

#[test]
fn missing_explicit_and_app_relative_commands_fail_closed() {
    let temp = tempdir().unwrap();
    let explicit = temp.path().join("missing.exe").display().to_string();
    let app_relative = "tools/missing";
    let environment = WindowsEnvironment {
        path: temp.path().as_os_str().to_os_string(),
        path_extensions: OsString::from(".EXE;.CMD"),
        command_interpreter: Some(test_command_interpreter()),
    };

    for requested in [&explicit, app_relative] {
        let error =
            plan_windows_command(&[requested.to_string()], temp.path(), &environment).unwrap_err();
        assert_actionable_unresolved_command_error(&error, requested);
    }
}

#[test]
fn path_search_skips_unusable_candidates_and_uses_later_executable() {
    let working_directory = Path::new("C:/repo");
    let path = OsStr::new("C:/denied;C:/dead-network;C:/tools");
    let mut inspected = Vec::new();
    let resolved = resolve_windows_executable_with(
        OsStr::new("node"),
        working_directory,
        path,
        OsStr::new(".EXE"),
        |candidate| {
            inspected.push(candidate.to_path_buf());
            let candidate = candidate.to_string_lossy();
            if candidate.contains("denied") {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "access denied",
                ));
            }
            if candidate.contains("dead-network") {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "network path unavailable",
                ));
            }
            Ok(candidate.contains("tools").then(|| {
                let path = PathBuf::from(candidate.as_ref());
                ResolvedWindowsExecutable {
                    launch_path: path.clone(),
                    canonical_path: path,
                }
            }))
        },
    )
    .unwrap();

    assert!(
        resolved
            .as_ref()
            .is_some_and(|resolved| resolved.launch_path.ends_with(Path::new("tools/node.EXE")))
    );
    assert_eq!(inspected.len(), 3);
}

#[test]
fn path_search_skips_drive_relative_entries_and_preserves_candidate_order() {
    let working_directory = Path::new(r"C:\repo\app");
    let search_path = OsStr::new(r#"D:ambient;;tools;"C:\quoted;safe";C:\safe;\\server\share\bin"#);
    let mut inspected = Vec::new();

    let resolved = resolve_windows_executable_with(
        OsStr::new("node"),
        working_directory,
        search_path,
        OsStr::new(".EXE;.CMD"),
        |candidate| {
            inspected.push(candidate.to_path_buf());
            Ok(None)
        },
    )
    .unwrap();

    let expected = [
        working_directory.to_path_buf(),
        working_directory.join("tools"),
        PathBuf::from(r"C:\quoted;safe"),
        PathBuf::from(r"C:\safe"),
        PathBuf::from(r"\\server\share\bin"),
    ]
    .into_iter()
    .flat_map(|directory| {
        [".EXE", ".CMD"].map(move |extension| {
            let mut candidate = directory.join("node").into_os_string();
            candidate.push(extension);
            PathBuf::from(candidate)
        })
    })
    .collect::<Vec<_>>();

    assert_eq!(resolved, None);
    assert_eq!(inspected, expected);
}

#[cfg(windows)]
#[test]
fn native_path_search_never_inspects_drive_relative_candidates() {
    let expected = PathBuf::from(r"C:\safe\node.EXE");
    let mut inspected = Vec::new();

    let resolved = resolve_windows_executable_with(
        OsStr::new("node"),
        Path::new(r"C:\repo\app"),
        OsStr::new(r"D:ambient;C:\safe"),
        OsStr::new(".EXE"),
        |candidate| {
            inspected.push(candidate.to_path_buf());
            Ok((candidate == expected).then(|| ResolvedWindowsExecutable {
                launch_path: candidate.to_path_buf(),
                canonical_path: candidate.to_path_buf(),
            }))
        },
    )
    .unwrap();

    assert_eq!(
        resolved.map(|resolved| resolved.launch_path),
        Some(expected.clone())
    );
    assert_eq!(inspected, vec![expected]);
}

#[test]
fn path_search_keeps_semicolons_inside_quoted_entries() {
    let working_directory = Path::new("C:/repo");
    let quoted_directory = PathBuf::from(r"C:\tools;beta");
    let expected_directory = quoted_directory;
    let expected = expected_directory.join("node.EXE");
    let mut inspected = Vec::new();

    let resolved = resolve_windows_executable_with(
        OsStr::new("node"),
        working_directory,
        OsStr::new(r#""C:\tools;beta";C:\fallback"#),
        OsStr::new(".EXE"),
        |candidate| {
            inspected.push(candidate.to_path_buf());
            Ok((candidate == expected).then(|| ResolvedWindowsExecutable {
                launch_path: candidate.to_path_buf(),
                canonical_path: candidate.to_path_buf(),
            }))
        },
    )
    .unwrap();

    assert_eq!(
        resolved
            .as_ref()
            .map(|resolved| resolved.launch_path.as_path()),
        Some(expected.as_path())
    );
    assert_eq!(inspected.first(), Some(&expected));
}

#[cfg(unix)]
#[test]
fn portable_windows_path_splitter_preserves_non_unicode_entries() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    let first = b"relative-tools-\xff";
    let mut raw = first.to_vec();
    raw.extend_from_slice(b";C:/fallback");
    let search_path = OsString::from_vec(raw);
    let entries = windows_search_path_entries(&search_path);

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].as_os_str().as_bytes(), first);
    assert_eq!(
        windows_search_path_entry_kind(&entries[0]),
        WindowsSearchPathEntryKind::Relative
    );

    let working_directory = Path::new("C:/repo");
    let mut inspected = Vec::new();
    resolve_windows_executable_with(
        OsStr::new("node"),
        working_directory,
        &search_path,
        OsStr::new(".EXE"),
        |candidate| {
            inspected.push(candidate.to_path_buf());
            Ok(None)
        },
    )
    .unwrap();
    let mut first_candidate = working_directory
        .join(&entries[0])
        .join("node")
        .into_os_string();
    first_candidate.push(".EXE");
    assert_eq!(inspected.first(), Some(&PathBuf::from(first_candidate)));
}

#[cfg(windows)]
#[test]
fn windows_path_splitter_preserves_non_unicode_entries() {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    let first = [
        u16::from(b'C'),
        u16::from(b':'),
        u16::from(b'\\'),
        u16::from(b't'),
        u16::from(b'o'),
        u16::from(b'o'),
        u16::from(b'l'),
        u16::from(b's'),
        u16::from(b'-'),
        0xd800,
    ];
    let mut raw = first.to_vec();
    raw.push(u16::from(b';'));
    raw.extend("C:\\fallback".encode_utf16());
    let entries = windows_search_path_entries(&OsString::from_wide(&raw));

    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries[0].as_os_str().encode_wide().collect::<Vec<_>>(),
        first
    );
}

#[cfg(windows)]
#[test]
fn child_compatible_path_rejects_unsafe_verbatim_namespaces() {
    let unsafe_path = Path::new(r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\repo");

    let error = windows_command_compatible_path(unsafe_path).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("cannot be represented safely"));
}

#[cfg(windows)]
#[test]
fn create_process_current_directory_keeps_legacy_max_path_boundary() {
    use std::os::windows::ffi::OsStrExt;

    let longest_supported = PathBuf::from(format!(r"C:\{}", "a".repeat(256)));
    assert_eq!(longest_supported.as_os_str().encode_wide().count(), 259);
    assert!(windows_command_compatible_path(&longest_supported).is_ok());

    let too_long = PathBuf::from(format!(r"C:\{}", "a".repeat(257)));
    assert_eq!(too_long.as_os_str().encode_wide().count(), 260);
    let error = windows_command_compatible_path(&too_long).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}

#[cfg(windows)]
#[test]
fn long_batch_launch_path_is_rejected_without_restricting_native_target() {
    let long_tail = "a".repeat(270);
    let native = PathBuf::from(format!(r"\\?\C:\{long_tail}\node.exe"));
    let direct = plan_resolved_windows_command(
        ResolvedWindowsExecutable {
            launch_path: PathBuf::from(r"C:\short\node.exe"),
            canonical_path: native.clone(),
        },
        &[],
        &environment(Path::new(r"C:\tools"), ".EXE;.CMD"),
    )
    .unwrap();
    assert_eq!(direct, WindowsCommandPlan::Direct(native.into_os_string()));

    let batch = PathBuf::from(format!(r"\\?\C:\{long_tail}\pnpm.cmd"));
    let error = plan_resolved_windows_command(
        ResolvedWindowsExecutable {
            launch_path: batch.clone(),
            canonical_path: batch,
        },
        &[],
        &environment(Path::new(r"C:\tools"), ".EXE;.CMD"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("cannot be represented safely"));
}

#[cfg(windows)]
#[test]
fn child_compatible_path_converts_verbatim_drive_and_unc_paths_at_launch_boundary() {
    assert_eq!(
        windows_command_compatible_path(Path::new(r"\\?\C:\repo\apps\web")).unwrap(),
        PathBuf::from(r"C:\repo\apps\web")
    );
    assert_eq!(
        windows_command_compatible_path(Path::new(r"\\?\UNC\server\share\repo\apps\web")).unwrap(),
        PathBuf::from(r"\\server\share\repo\apps\web")
    );
}

#[test]
fn explicit_windows_executable_preserves_inspection_errors() {
    let error = resolve_windows_executable_with(
        OsStr::new("C:/denied/node.exe"),
        Path::new("C:/repo"),
        OsStr::new("C:/tools"),
        OsStr::new(".EXE"),
        |_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "access denied",
            ))
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("C:/denied/node.exe"));
    assert!(
        error
            .chain()
            .any(|cause| cause.to_string() == "access denied")
    );
}

#[test]
fn command_shim_encoder_preserves_batch_argv_semantics() {
    for (argument, expected) in [
        ("", "\"\""),
        ("simple", "simple"),
        ("two words", "\"two words\""),
        (r"C:\repo\", "\"C:\\repo\\\\\""),
        ("%PATH%", "\"%%cd:~,%PATH%%cd:~,%\""),
        ("a&b", "\"a&b\""),
        ("(group)", "\"(group)\""),
        ("say\"hi", "\"say\"\"hi\""),
        ("Zażółć", "Zażółć"),
    ] {
        let mut encoded = String::new();
        append_batch_argument(&mut encoded, argument).unwrap();
        assert_eq!(encoded, expected, "argument {argument:?}");
    }
}

#[test]
fn command_shim_encoder_rejects_only_command_truncation_input() {
    for argument in ["nul\0byte", "line\nbreak", "line\rbreak"] {
        let mut encoded = String::new();
        let error = append_batch_argument(&mut encoded, argument).unwrap_err();
        assert!(error.to_string().contains("NUL, CR, or LF"));
    }
}

#[test]
fn command_shim_path_uses_forced_batch_quoting_and_percent_protection() {
    let command_line = encode_command_shim_invocation(
        Path::new(r"C:\repo %name%\space & (group)\run.cmd"),
        &["dev".into()],
    )
    .unwrap();

    assert!(command_line.starts_with("\"\""));
    assert!(command_line.ends_with(" dev\""));
    assert!(command_line.contains("%%cd:~,%name%%cd:~,%"));
}

#[test]
fn command_shim_encoder_rejects_remaining_verbatim_paths() {
    for executable in [
        Path::new(r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\run.cmd"),
        Path::new(r"\\.\C:\run.cmd"),
        Path::new("//?/Volume{01234567-89ab-cdef-0123-456789abcdef}/run.cmd"),
    ] {
        let error = encode_command_shim_invocation(executable, &[]).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unsafe Windows development command shim path")
        );
    }
}

#[test]
fn effective_environment_overrides_are_case_insensitive() {
    let overrides = vec![
        ("Path".into(), r"C:\tools".into()),
        ("pathext".into(), ".CMD".into()),
        ("ComSpec".into(), r"D:\shell\cmd.exe".into()),
    ];
    let environment = WindowsEnvironment::effective(&overrides);
    assert_eq!(environment.path, OsString::from(r"C:\tools"));
    assert_eq!(environment.path_extensions, OsString::from(".CMD"));
    assert_eq!(
        environment.command_interpreter,
        Some(OsString::from(r"D:\shell\cmd.exe"))
    );
}

#[cfg(windows)]
#[test]
fn command_shim_argv_roundtrip_helper() {
    if std::env::var_os("JIG_WINDOWS_BATCH_ROUNDTRIP").is_none() {
        return;
    }
    for (index, expected) in [
        "",
        "two words",
        r"C:\repo\",
        "%PATH%",
        "a&b",
        "(group)",
        "say\"hi",
        "Zażółć!",
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            std::env::var(format!("JIG_WINDOWS_BATCH_ARG_{}", index + 1)).unwrap(),
            expected
        );
    }
    assert_eq!(std::env::var("ASTRO_DEV_BACKGROUND").unwrap(), "0");
}

#[cfg(windows)]
#[test]
fn command_shim_spawn_round_trips_complex_arguments() {
    let temp = tempdir().unwrap();
    let shim_dir = temp.path().join("% shim & (roundtrip)");
    fs::create_dir(&shim_dir).unwrap();
    let shim = shim_dir.join("jig-shim.CMD");
    let test_exe = std::env::current_exe()
        .unwrap()
        .display()
        .to_string()
        .replace('%', "%%");
    let mut script = String::from("@echo off\r\nsetlocal DisableDelayedExpansion\r\n");
    script.push_str("set \"JIG_WINDOWS_BATCH_ROUNDTRIP=1\"\r\n");
    for index in 1..=8 {
        script.push_str(&format!(
            "set \"JIG_WINDOWS_BATCH_ARG_{index}=%~{index}\"\r\n"
        ));
    }
    script.push_str(&format!(
            "@\"{test_exe}\" --exact processes::windows_launch::tests::command_shim_argv_roundtrip_helper --nocapture\r\n"
        ));
    fs::write(&shim, script).unwrap();
    let argv = vec![
        shim.display().to_string(),
        "".into(),
        "two words".into(),
        r"C:\repo\".into(),
        "%PATH%".into(),
        "a&b".into(),
        "(group)".into(),
        "say\"hi".into(),
        "Zażółć!".into(),
    ];

    let spec = AppRunSpec {
        name: "windows-shim".into(),
        dir: temp.path().to_path_buf(),
        command: CommandSpec::Argv(Vec::new()),
        kind: AppKind::EnvPort,
        hostname: "windows-shim.example.localhost".into(),
        target_host: "127.0.0.1".into(),
        explicit_port: None,
        proxy: false,
    };
    let settings = ProxySettings::default();
    let dev_env = [("ASTRO_DEV_BACKGROUND".to_string(), String::new())];
    let mut spawned = super::super::spawn_child(&spec, &argv, 4321, &settings, &dev_env)
        .expect("spawn encoded Windows command shim");
    let status = spawned.child.wait().unwrap();
    let output = String::from_utf8(spawned.output.captured_bytes()).unwrap();

    assert!(status.success(), "shim helper failed:\n{output}");
    super::super::child_lifecycle::terminate_and_reap(&mut spawned.child).unwrap();
}

#[cfg(windows)]
#[test]
fn exited_command_shim_descendants_remain_owned_by_the_app_job() {
    use std::time::{Duration, Instant};

    let temp = tempdir().unwrap();
    let shim = temp.path().join("background.CMD");
    fs::write(
            &shim,
            "@echo off\r\nstart \"\" /b \"%ComSpec%\" /d /s /c \"ping -n 30 127.0.0.1 >nul\"\r\nexit /b 0\r\n",
        )
        .unwrap();
    let argv = vec![shim.display().to_string()];
    let spec = AppRunSpec {
        name: "windows-background-shim".into(),
        dir: temp.path().to_path_buf(),
        command: CommandSpec::Argv(Vec::new()),
        kind: AppKind::EnvPort,
        hostname: "windows-background-shim.example.localhost".into(),
        target_host: "127.0.0.1".into(),
        explicit_port: None,
        proxy: false,
    };
    let mut spawned =
        super::super::spawn_child(&spec, &argv, 4321, &ProxySettings::default(), &[]).unwrap();
    let root_pid = spawned.child.id();
    let deadline = Instant::now() + Duration::from_secs(5);
    while spawned.child.try_wait().unwrap().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        spawned.child.try_wait().unwrap().is_some(),
        "command shim wrapper did not exit"
    );
    assert!(
        super::super::child_lifecycle::windows_app_job_active_processes(root_pid)
            .unwrap()
            .is_some_and(|active| active > 0),
        "background descendant was not retained in the app job"
    );

    super::super::child_lifecycle::terminate_and_reap(&mut spawned.child).unwrap();

    assert_eq!(
        super::super::child_lifecycle::windows_app_job_active_processes(root_pid).unwrap(),
        None
    );
}
