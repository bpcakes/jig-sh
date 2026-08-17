#[cfg(any(target_os = "linux", target_os = "macos"))]
const OWNED_PROCESS_DESCENDANT_MARKER_ENV: &str = "JIG_DOCTOR_OWNED_PROCESS_DESCENDANT_MARKER";

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn shell_quote_test_path(path: &Path) -> String {
    let path = path
        .to_str()
        .expect("test helper paths must be representable in shell fixtures");
    format!("'{}'", path.replace('\'', "'\"'\"'"))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn owned_test_descendant_script(marker: &Path, tail: &str) -> String {
    format!(
        "#!/bin/sh\n{marker_env}={marker} {test_exe} --exact doctor::tests::owned_process_descendant_helper --nocapture &\nwhile [ ! -f {marker} ]; do :; done\n{tail}\n",
        marker_env = OWNED_PROCESS_DESCENDANT_MARKER_ENV,
        marker = shell_quote_test_path(marker),
        test_exe = shell_quote_test_path(&std::env::current_exe().unwrap()),
    )
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn owned_process_descendant_helper() {
    let Some(marker) = std::env::var_os(OWNED_PROCESS_DESCENDANT_MARKER_ENV) else {
        return;
    };
    let identity = TestProcessIdentity::capture_current().expect("capture test helper identity");
    publish_test_process_identity(Path::new(&marker), &identity);
    std::thread::sleep(Duration::from_secs(30));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn proxy_list_output_helper() {
    let Some(mode) = std::env::var_os("JIG_DOCTOR_PROXY_LIST_HELPER") else {
        return;
    };
    if mode == "valid" {
        println!(r#"{{"ok":true,"running":false,"routes":[]}}"#);
        return;
    }

    let marker = PathBuf::from(
        std::env::var_os("JIG_DOCTOR_PROXY_LIST_DESCENDANT_MARKER")
            .expect("hanging proxy-list helper has a descendant marker"),
    );
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::owned_process_descendant_helper",
            "--nocapture",
        ])
        .env(OWNED_PROCESS_DESCENDANT_MARKER_ENV, &marker)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    std::mem::forget(child);
    let _ = read_test_process_identity(&marker);
    std::thread::sleep(Duration::from_secs(30));
}

#[test]
fn program_resolution_distinguishes_unset_and_explicitly_empty_path() {
    let _env = lock_env();
    let repo = tempdir().unwrap();
    let invocation = tempdir().unwrap();
    let _cwd = CurrentDirGuard::set(invocation.path());
    let program = "doctor-empty-path-tool";
    #[cfg(unix)]
    write_test_executable(&repo.path().join(program), "#!/bin/sh\nexit 0\n");
    #[cfg(windows)]
    fs::write(repo.path().join(format!("{program}.CMD")), "@exit /b 0\r\n").unwrap();
    #[cfg(not(any(unix, windows)))]
    fs::write(repo.path().join(program), "executable\n").unwrap();
    #[cfg(windows)]
    let path_extensions = Some(OsStr::new(".CMD"));
    #[cfg(not(windows))]
    let path_extensions = None;

    assert_eq!(
        resolve_program(repo.path(), program, None, path_extensions),
        None
    );
    let resolution =
        resolve_program(repo.path(), program, Some(OsStr::new("")), path_extensions).unwrap();
    assert!(resolution.path.starts_with(repo.path()));
    assert!(!resolution.path.starts_with(invocation.path()));
    assert_eq!(
        resolution.origin,
        ProgramOrigin::SearchPath {
            entry: PathBuf::new()
        }
    );
}

#[test]
fn command_programs_include_external_env_and_skip_shell_assignments() {
    assert_eq!(command_program("cargo test").as_deref(), Some("cargo"));
    assert_eq!(
        command_program("RUSTFLAGS=-Dwarnings cargo test").as_deref(),
        Some("cargo")
    );
    assert_eq!(
        command_programs_for_shell("env RUSTFLAGS=-Dwarnings cargo test"),
        vec!["env", "cargo"]
    );
    assert_eq!(
        command_programs_for_shell("env FOO.BAR=x cargo test"),
        vec!["env", "cargo"]
    );
    assert_eq!(
        command_program("\"scripts/jig\" check contract").as_deref(),
        Some("scripts/jig")
    );
}

#[test]
fn command_programs_only_treat_keywords_as_pre_assignment_prefixes() {
    for command in [
        "DATABASE_URL=sqlite:x ! sqlx prepare",
        "! DATABASE_URL=sqlite:x ! sqlx prepare",
        "DATABASE_URL=sqlite:x then sqlx prepare",
    ] {
        assert!(
            command_programs_for_shell(command).is_empty(),
            "{command:?}",
        );
    }

    assert_eq!(
        command_programs_for_shell("! DATABASE_URL=sqlite:x sqlx prepare"),
        vec!["sqlx"],
    );
    for command in [
        "''! DATABASE_URL=sqlite:x sqlx prepare",
        r"\! DATABASE_URL=sqlite:x sqlx prepare",
    ] {
        assert_eq!(
            command_programs_for_shell(command),
            vec!["!"],
            "{command:?}"
        );
    }
}

#[test]
fn required_programs_treat_env_split_strings_and_unknown_options_as_ambiguous() {
    for command in [
        "env -S 'private-split-tool --flag'",
        "env -Sprivate-split-tool",
        "env '-Sprivate-split-tool --flag'",
        "env --split-string 'private-split-tool --flag'",
        "env --split-string",
        "env '--split-string=private-split-tool --flag'",
        "env -iS 'private-split-tool --flag'",
        "env -S 'private-split-tool --flag' cargo",
        "env --private-option cargo",
    ] {
        let discovery = required_command_programs_for_shell(command);
        assert_eq!(
            discovery.ambiguity,
            Some(RequiredProgramAmbiguity::Wrapper),
            "{command:?}",
        );
        assert_eq!(
            discovery
                .programs
                .iter()
                .map(|program| program.program.as_str())
                .collect::<Vec<_>>(),
            vec!["env"],
            "{command:?}",
        );
        assert_eq!(
            discovery.programs[0].path_lookup,
            ProgramPathLookup::Captured,
            "{command:?}",
        );
    }

    for command in [
        "env -P /private/tools cargo test",
        "env -P/private/tools cargo test",
    ] {
        let discovery = required_command_programs_for_shell(command);
        assert_eq!(
            discovery.ambiguity,
            Some(RequiredProgramAmbiguity::Wrapper),
            "{command:?}",
        );
        assert_eq!(
            discovery
                .programs
                .iter()
                .map(|program| program.program.as_str())
                .collect::<Vec<_>>(),
            vec!["env", "cargo"],
            "{command:?}",
        );
        assert_eq!(
            discovery.programs[0].path_lookup,
            ProgramPathLookup::Captured
        );
        assert_eq!(
            discovery.programs[1].path_lookup,
            ProgramPathLookup::Unverifiable
        );
    }
}

#[test]
fn required_programs_model_builtin_dispatch_and_external_wrapper_boundaries() {
    for command in [
        "builtin export DATABASE_URL=sqlite:ignored.db",
        "builtin declare DATABASE_URL=sqlite:ignored.db",
        "builtin printf -v DATABASE_URL %s sqlite:ignored.db",
        "builtin command export DATABASE_URL=sqlite:ignored.db",
        "command builtin export DATABASE_URL=sqlite:ignored.db",
        "builtin doctor-not-a-builtin cargo test",
    ] {
        let discovery = required_command_programs_for_shell(command);
        assert!(discovery.programs.is_empty(), "{command:?}");
        assert!(discovery.ambiguity.is_none(), "{command:?}");
    }

    for (command, expected) in [
        ("builtin command cargo test", vec!["cargo"]),
        ("builtin exec cargo test", vec!["cargo"]),
        ("exec export DATABASE_URL=x", vec!["export"]),
        ("env export DATABASE_URL=x", vec!["env", "export"]),
        ("nohup export DATABASE_URL=x", vec!["nohup", "export"]),
        ("command exec export DATABASE_URL=x", vec!["export"]),
        ("env exec cargo test", vec!["env", "exec"]),
        ("nohup command cargo test", vec!["nohup", "command"]),
        ("exec env cargo test", vec!["env", "cargo"]),
        ("env nohup cargo test", vec!["env", "nohup", "cargo"]),
    ] {
        assert_eq!(command_programs_for_shell(command), expected, "{command:?}");
    }
}

#[test]
fn required_programs_emit_ordered_external_wrapper_chains() {
    for (command, expected) in [
        (
            "env nohup /usr/bin/time cargo test",
            vec!["env", "nohup", "/usr/bin/time", "cargo"],
        ),
        (
            "/opt/tools/env /opt/tools/nohup /opt/tools/time /opt/tools/cargo test",
            vec![
                "/opt/tools/env",
                "/opt/tools/nohup",
                "/opt/tools/time",
                "/opt/tools/cargo",
            ],
        ),
        ("command time cargo test", vec!["time", "cargo"]),
        ("exec env cargo test", vec!["env", "cargo"]),
        ("command exec cargo test", vec!["cargo"]),
    ] {
        let discovery = required_command_programs_for_shell(command);
        assert_eq!(
            discovery
                .programs
                .iter()
                .map(|program| program.program.as_str())
                .collect::<Vec<_>>(),
            expected,
            "{command:?}",
        );
        assert!(discovery.ambiguity.is_none(), "{command:?}");
    }
}

#[test]
fn required_programs_retain_external_wrappers_without_a_known_target() {
    for command in [
        "env",
        "env --help",
        "env -0",
        "nohup",
        "nohup --help",
        "/usr/bin/time --help",
    ] {
        let discovery = required_command_programs_for_shell(command);
        assert_eq!(discovery.programs.len(), 1, "{command:?}");
        assert!(discovery.ambiguity.is_none(), "{command:?}");
    }
    assert_eq!(
        command_programs_for_shell("nohup -- --help"),
        vec!["nohup", "--help"]
    );

    for (command, expected_wrapper) in [
        ("env \"$TOOL\" test", "env"),
        ("env --private-option cargo test", "env"),
        ("nohup --private-option cargo test", "nohup"),
        ("/usr/bin/time --private-option cargo test", "/usr/bin/time"),
    ] {
        let discovery = required_command_programs_for_shell(command);
        assert_eq!(
            discovery.ambiguity,
            Some(RequiredProgramAmbiguity::Wrapper),
            "{command:?}",
        );
        assert_eq!(discovery.programs.len(), 1, "{command:?}");
        assert_eq!(
            discovery.programs[0].program, expected_wrapper,
            "{command:?}",
        );
    }
}

#[test]
fn required_programs_recognize_complete_standard_bash_builtins_and_keywords() {
    for builtin in [
        ".",
        ":",
        "[",
        "alias",
        "bg",
        "bind",
        "break",
        "builtin",
        "caller",
        "cd",
        "command",
        "compgen",
        "complete",
        "compopt",
        "continue",
        "declare",
        "dirs",
        "disown",
        "echo",
        "enable",
        "eval",
        "exec",
        "exit",
        "export",
        "false",
        "fc",
        "fg",
        "getopts",
        "hash",
        "help",
        "history",
        "jobs",
        "kill",
        "let",
        "local",
        "logout",
        "mapfile",
        "popd",
        "printf",
        "pushd",
        "pwd",
        "read",
        "readarray",
        "readonly",
        "return",
        "set",
        "shift",
        "shopt",
        "source",
        "suspend",
        "test",
        "times",
        "trap",
        "true",
        "type",
        "typeset",
        "ulimit",
        "umask",
        "unalias",
        "unset",
        "wait",
    ] {
        assert!(bash_builtin(builtin), "missing Bash builtin {builtin:?}");
    }
    for keyword in [
        "!", "[[", "]]", "case", "coproc", "do", "done", "elif", "else", "esac", "fi", "for",
        "function", "if", "in", "select", "then", "time", "until", "while", "{", "}",
    ] {
        assert!(bash_keyword(keyword), "missing Bash keyword {keyword:?}");
    }

    for command in [
        "wait",
        "kill -0 1",
        "mapfile -t values </dev/null",
        "readarray values </dev/null",
        "getopts p option",
        "hash -r",
        "help wait",
        "shopt -s nullglob",
        "pushd .",
        "popd",
        "\"wait\"",
    ] {
        assert!(
            command_programs_for_shell(command).is_empty(),
            "{command:?}"
        );
    }

    let quoted_keyword = required_command_programs_for_shell("\"[[\" argument");
    assert_eq!(quoted_keyword.programs[0].program, "[[");
    assert!(quoted_keyword.ambiguity.is_none());
    let command_keyword = required_command_programs_for_shell("command time true");
    assert_eq!(command_keyword.programs[0].program, "time");
}

#[test]
fn required_programs_surface_dynamic_eval_source_time_and_global_parse_ambiguity() {
    for command in [
        "$TOOL test",
        "command \"$TOOL\" test",
        "eval 'cargo test'",
        "source scripts/setup.sh",
        ". scripts/setup.sh",
        "builtin eval 'cargo test'",
        "command source scripts/setup.sh",
        "time cargo test",
        "[[ -n value ]]",
        "cargo \"$(missing-helper)\" test",
        "cargo test >\"$(missing-helper)\"",
        "cargo `missing-helper` test",
    ] {
        let discovery = required_command_programs_for_shell(command);
        assert!(discovery.ambiguity.is_some(), "{command:?}");
        assert!(
            discovery
                .programs
                .iter()
                .all(|program| program.path_lookup == ProgramPathLookup::Unverifiable),
            "{command:?}",
        );
    }

    let dynamic_target = required_command_programs_for_shell("nohup \"$TOOL\" test");
    assert_eq!(
        dynamic_target.ambiguity,
        Some(RequiredProgramAmbiguity::Wrapper)
    );
    assert_eq!(dynamic_target.programs[0].program, "nohup");
    assert_eq!(
        dynamic_target.programs[0].path_lookup,
        ProgramPathLookup::Captured
    );

    let literal = required_command_programs_for_shell("'$TOOL' test");
    assert_eq!(literal.programs[0].program, "$TOOL");
    assert_eq!(literal.programs[0].path_lookup, ProgramPathLookup::Captured);

    for command in [
        "doctor_fn() { :; }; doctor_fn",
        "if true; then /definitely/missing; fi",
    ] {
        let discovery = required_command_programs_for_shell(command);
        assert_eq!(
            discovery.ambiguity,
            Some(RequiredProgramAmbiguity::ShellSyntax),
            "{command:?}",
        );
        assert!(
            discovery
                .programs
                .iter()
                .all(|program| program.path_lookup == ProgramPathLookup::Unverifiable),
            "{command:?}",
        );
    }
}

#[test]
fn required_programs_taint_dispatch_after_shell_state_mutations() {
    for (command, expected_program) in [
        ("hash -p /tmp/shim cargo; cargo test", "cargo"),
        ("enable -f /tmp/plugin custom; custom", "custom"),
        ("trap 'missing-helper' DEBUG; cargo test", "cargo"),
    ] {
        let discovery = required_command_programs_for_shell(command);
        assert_eq!(
            discovery.ambiguity,
            Some(RequiredProgramAmbiguity::ShellState),
            "{command:?}",
        );
        let program = discovery.programs.last().unwrap();
        assert_eq!(program.program, expected_program, "{command:?}");
        assert_eq!(
            program.path_lookup.clone(),
            ProgramPathLookup::Unverifiable,
            "{command:?}",
        );
    }

    for command in ["hash -t cargo; cargo test", "trap -p; cargo test"] {
        let discovery = required_command_programs_for_shell(command);
        assert!(discovery.ambiguity.is_none(), "{command:?}");
        assert_eq!(
            discovery.programs.last().unwrap().path_lookup.clone(),
            ProgramPathLookup::Captured,
            "{command:?}",
        );
    }
}

#[test]
fn required_programs_do_not_resolve_cwd_sensitive_paths_from_the_repo_root() {
    for command in [
        "env -C sub ./tool",
        "env --chdir sub ./tool",
        "cd sub && ./tool",
    ] {
        let discovery = required_command_programs_for_shell(command);
        let program = discovery.programs.last().unwrap();
        assert_eq!(program.program, "./tool", "{command:?}");
        assert_eq!(
            program.path_lookup.clone(),
            ProgramPathLookup::Unverifiable,
            "{command:?}"
        );
    }
    for command in ["env -C sub tool", "cd sub && tool"] {
        let discovery = required_command_programs_for_shell(command);
        assert_eq!(
            discovery.programs.last().unwrap().path_lookup.clone(),
            ProgramPathLookup::CapturedAfterCwdChange,
            "{command:?}",
        );
    }
    assert_eq!(
        required_command_programs_for_shell("env -C child cargo test")
            .programs
            .iter()
            .map(|program| (program.program.as_str(), program.path_lookup.clone()))
            .collect::<Vec<_>>(),
        vec![
            ("env", ProgramPathLookup::Captured),
            ("cargo", ProgramPathLookup::CapturedAfterCwdChange),
        ]
    );
    assert_eq!(
        required_command_programs_for_shell("env -C child nohup cargo test")
            .programs
            .iter()
            .map(|program| (program.program.as_str(), program.path_lookup.clone()))
            .collect::<Vec<_>>(),
        vec![
            ("env", ProgramPathLookup::Captured),
            ("nohup", ProgramPathLookup::CapturedAfterCwdChange),
            ("cargo", ProgramPathLookup::CapturedAfterCwdChange),
        ]
    );
    assert_eq!(
        required_command_programs_for_shell("cd sub; PATH=relative tool")
            .programs
            .last()
            .unwrap()
            .path_lookup,
        ProgramPathLookup::Unverifiable,
    );
    #[cfg(not(windows))]
    for command in ["env -C sub /usr/bin/tool", "cd sub && /usr/bin/tool"] {
        let discovery = required_command_programs_for_shell(command);
        assert_eq!(
            discovery.programs.last().unwrap().path_lookup,
            ProgramPathLookup::Explicit,
            "{command:?}",
        );
    }
    #[cfg(windows)]
    for command in [
        "env -C sub C:/Windows/System32/cmd.exe",
        "cd sub && C:/Windows/System32/cmd.exe",
    ] {
        let discovery = required_command_programs_for_shell(command);
        assert_eq!(
            discovery.programs.last().unwrap().path_lookup,
            ProgramPathLookup::Explicit,
            "{command:?}",
        );
    }
    #[cfg(not(windows))]
    {
        assert!(search_path_is_cwd_independent(Some(OsStr::new(
            "/usr/bin:/bin"
        ))));
        assert!(!search_path_is_cwd_independent(Some(OsStr::new(
            "bin:/usr/bin"
        ))));
        assert!(!search_path_is_cwd_independent(Some(OsStr::new(
            ":/usr/bin"
        ))));
    }
    #[cfg(windows)]
    {
        assert!(search_path_is_cwd_independent(Some(OsStr::new(
            r"C:\Windows;D:\bin"
        ))));
        assert!(!search_path_is_cwd_independent(Some(OsStr::new(
            r"bin;C:\Windows"
        ))));
        assert!(!search_path_is_cwd_independent(Some(OsStr::new(
            r";C:\Windows"
        ))));
    }
}

#[test]
fn required_programs_treat_env_null_with_a_utility_as_ambiguous() {
    for command in [
        "env -0 cargo test",
        "env --null cargo test",
        "env -i0 cargo test",
        "env -0 DATABASE_URL=sqlite:ignored.db cargo test",
    ] {
        let discovery = required_command_programs_for_shell(command);
        assert_eq!(discovery.programs[0].program, "env", "{command:?}");
        assert_eq!(discovery.programs.len(), 1, "{command:?}");
        assert_eq!(
            discovery.ambiguity,
            Some(RequiredProgramAmbiguity::Wrapper),
            "{command:?}",
        );
    }
    let print_environment = required_command_programs_for_shell("env -0 DATABASE_URL=value");
    assert_eq!(print_environment.programs[0].program, "env");
    assert_eq!(print_environment.programs.len(), 1);
    assert!(print_environment.ambiguity.is_none());
}

#[test]
fn executable_basename_is_utf8_boundary_safe() {
    assert_eq!(executable_basename("💩a"), Some("💩a"));
    assert_eq!(executable_basename("💩.EXE"), Some("💩"));
    let discovery = required_command_programs_for_shell("💩a --version");
    assert_eq!(discovery.programs[0].program, "💩a");
}

#[test]
fn command_programs_respect_env_option_and_wrapper_assignment_boundaries() {
    assert_eq!(
        command_programs_for_shell("env FOO=one -i cargo test"),
        vec!["env", "-i"]
    );
    assert_eq!(
        command_programs_for_shell("env -i FOO=one cargo test"),
        vec!["env", "cargo"]
    );
    assert_eq!(
        command_programs_for_shell("command env FOO=one cargo test"),
        vec!["env", "cargo"]
    );

    for command in [
        "command DATABASE_URL=sqlite:private.db cargo sqlx prepare",
        "exec DATABASE_URL=sqlite:private.db cargo sqlx prepare",
    ] {
        let discovery = required_command_programs_for_shell(command);
        assert_eq!(
            discovery.ambiguity,
            Some(RequiredProgramAmbiguity::Wrapper),
            "{command:?}",
        );
        assert!(discovery.programs.is_empty(), "{command:?}");
    }
    let nohup = required_command_programs_for_shell(
        "nohup DATABASE_URL=sqlite:private.db cargo sqlx prepare",
    );
    assert_eq!(nohup.ambiguity, Some(RequiredProgramAmbiguity::Wrapper));
    assert_eq!(nohup.programs[0].program, "nohup");
    assert_eq!(nohup.programs.len(), 1);
}
