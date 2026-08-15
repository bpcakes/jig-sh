use super::*;
use crate::test_env::{CurrentDirGuard, EnvVarGuard, TestRepoBuilder, lock_env};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::test_process::{
    TestProcessIdentity, assert_test_process_stopped, publish_test_process_identity,
    read_test_process_identity,
};
use serde_json::json;
use tempfile::tempdir;
#[cfg(unix)]
use wait_timeout::ChildExt;

#[cfg(any(target_os = "linux", target_os = "macos"))]
const OWNED_PROCESS_DESCENDANT_MARKER_ENV: &str = "JIG_DOCTOR_OWNED_PROCESS_DESCENDANT_MARKER";
const CURRENT_GENERATED_LAUNCHER_TEMPLATE: &str =
    include_str!("../bootstrap/embedded_template_snapshots/scripts/jig.jinja");
const CURRENT_GENERATED_INSTALLER: &str =
    include_str!("../bootstrap/embedded_template_snapshots/scripts/install-jig.sh.jinja");

fn current_generated_launcher() -> String {
    CURRENT_GENERATED_LAUNCHER_TEMPLATE.replace(
        "<<[ _jig.contract_version ]>>",
        &crate::context::CURRENT_CONTRACT_VERSION.to_string(),
    )
}

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

#[test]
fn required_programs_track_path_lookup_scope() {
    let lookups = |command: &str| {
        required_command_programs_for_shell(command)
            .programs
            .into_iter()
            .map(|program| (program.program, program.path_lookup))
            .collect::<Vec<_>>()
    };

    for command in [
        "PATH=repo-bin; sqlx prepare",
        "PATH[0]=repo-bin; sqlx prepare",
        "export PATH=repo-bin; sqlx prepare",
        "declare -x PATH=repo-bin; sqlx prepare",
        "builtin declare -x PATH=repo-bin; sqlx prepare",
        "read PATH <<< repo-bin; sqlx prepare",
        "printf -v PATH %s repo-bin; sqlx prepare",
        "printf -v 'PATH[0]' %s repo-bin; sqlx prepare",
        "mapfile -t PATH </dev/null; sqlx prepare",
        "getopts p PATH; sqlx prepare",
        "let PATH=0; sqlx prepare",
        "declare -n path_ref=PATH; sqlx prepare",
        "unset PATH; sqlx prepare",
        "PATH=repo-bin && sqlx prepare",
        "PATH+=:repo-bin; sqlx prepare",
        "false && export PATH=repo-bin; sqlx prepare",
        "source settings.sh; sqlx prepare",
    ] {
        assert_eq!(
            lookups(command).last(),
            Some(&("sqlx".to_string(), ProgramPathLookup::Unverifiable)),
            "{command:?}"
        );
    }

    assert_eq!(
        lookups("PATH=repo-bin sqlx prepare"),
        vec![(
            "sqlx".to_string(),
            ProgramPathLookup::CommandLocal(OsString::from("repo-bin")),
        )]
    );
    assert_eq!(
        lookups("PATH= sqlx prepare"),
        vec![(
            "sqlx".to_string(),
            ProgramPathLookup::CommandLocal(OsString::new()),
        )]
    );
    assert_eq!(
        lookups("command -p sqlx prepare"),
        vec![("sqlx".to_string(), ProgramPathLookup::Unverifiable)]
    );
    assert_eq!(
        lookups("env PATH=/missing sqlx prepare"),
        vec![
            ("env".to_string(), ProgramPathLookup::Captured),
            (
                "sqlx".to_string(),
                ProgramPathLookup::CommandLocal(OsString::from("/missing")),
            ),
        ]
    );
    for command in ["env -u PATH sqlx prepare", "env -i sqlx prepare"] {
        assert_eq!(
            lookups(command),
            vec![
                ("env".to_string(), ProgramPathLookup::Captured),
                ("sqlx".to_string(), ProgramPathLookup::Unverifiable),
            ],
            "{command:?}"
        );
    }
    assert_eq!(
        lookups("PATH=repo-bin env sqlx prepare"),
        vec![
            (
                "env".to_string(),
                ProgramPathLookup::CommandLocal(OsString::from("repo-bin")),
            ),
            (
                "sqlx".to_string(),
                ProgramPathLookup::CommandLocal(OsString::from("repo-bin")),
            ),
        ]
    );
    assert_eq!(
        lookups("exec -c env sqlx prepare"),
        vec![
            ("env".to_string(), ProgramPathLookup::Captured),
            ("sqlx".to_string(), ProgramPathLookup::Unverifiable),
        ]
    );
    assert_eq!(
        lookups("exec -c sqlx prepare"),
        vec![("sqlx".to_string(), ProgramPathLookup::Captured)]
    );
    assert_eq!(
        lookups("command -p env sqlx prepare"),
        vec![
            ("env".to_string(), ProgramPathLookup::Unverifiable),
            ("sqlx".to_string(), ProgramPathLookup::Captured),
        ]
    );

    assert_eq!(
        lookups("PATH=repo-bin sqlx prepare; cargo test"),
        vec![
            (
                "sqlx".to_string(),
                ProgramPathLookup::CommandLocal(OsString::from("repo-bin")),
            ),
            ("cargo".to_string(), ProgramPathLookup::Captured),
        ]
    );
    for command in [
        "PATH=repo-bin true; sqlx prepare",
        "PATH=repo-bin /bin/true; sqlx prepare",
        "printf '%s' PATH=repo-bin; sqlx prepare",
    ] {
        assert_eq!(
            lookups(command).last(),
            Some(&("sqlx".to_string(), ProgramPathLookup::Captured)),
            "{command:?}"
        );
    }
    assert_eq!(
        lookups("PATH=repo-bin; scripts/sqlx prepare").last(),
        Some(&("scripts/sqlx".to_string(), ProgramPathLookup::Explicit))
    );
}

#[test]
fn command_programs_report_compound_command_executables() {
    assert_eq!(
        command_programs(Path::new("."), "cargo test && npm run build"),
        vec!["cargo", "npm"]
    );
    assert_eq!(
        command_programs(
            Path::new("."),
            "RUSTFLAGS=-Dwarnings cargo test; env NODE_ENV=test pnpm test"
        ),
        vec!["cargo", "env", "pnpm"]
    );
}

#[test]
fn command_programs_skip_builtins_and_redirection_targets() {
    assert_eq!(
        command_programs(
            Path::new("."),
            "printf '%s\\n' skipped > /tmp/out && cargo test 2>&1"
        ),
        vec!["cargo"]
    );
    for command in [
        "cargo test &> /tmp/out",
        "cargo test &>>/tmp/out",
        "cargo test >| /tmp/out",
    ] {
        assert_eq!(command_programs(Path::new("."), command), vec!["cargo"]);
    }
    for command in [
        "'2'>/tmp/out cargo sqlx prepare -D sqlite:ignored.db",
        r"\2>/tmp/out cargo sqlx prepare -D sqlite:ignored.db",
        "''DATABASE_URL=sqlite:ignored.db cargo sqlx prepare",
        "'DATABASE_URL'=sqlite:ignored.db cargo sqlx prepare",
        "''! cargo sqlx prepare -D sqlite:ignored.db",
    ] {
        assert_ne!(
            command_program(command).as_deref(),
            Some("cargo"),
            "{command:?}"
        );
    }
    assert_eq!(
        command_program("DATABASE_URL='sqlite:actual.db' cargo sqlx prepare").as_deref(),
        Some("cargo")
    );
}

#[test]
fn command_programs_skip_shell_block_closers() {
    assert_eq!(
        command_programs_for_shell(
            "for manifest in crates/*/Cargo.toml; do cargo test --manifest-path \"$manifest\"; done; if [ \"$found\" -eq 0 ]; then printf skipped; fi"
        ),
        vec!["cargo"]
    );
}

#[test]
fn command_programs_follow_generated_optional_cargo_branch() {
    let temp = tempdir().unwrap();
    let command = format!(
        "{}cargo fetch{}printf '%s\\n' skipped{}",
        crate::shell::OPTIONAL_CARGO_COMMAND_PREFIX,
        crate::shell::OPTIONAL_CARGO_COMMAND_ELSE,
        crate::shell::OPTIONAL_CARGO_COMMAND_SUFFIX,
    );

    assert!(command_programs(temp.path(), &command).is_empty());

    fs::write(temp.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    assert_eq!(command_programs(temp.path(), &command), vec!["cargo"]);
}

#[test]
fn command_programs_require_cargo_sqlx_subcommand() {
    assert_eq!(
        command_programs_for_shell(
            "SQLX_OFFLINE=false SQLX_OFFLINE_DIR=.sqlx cargo sqlx prepare --check"
        ),
        vec!["cargo"]
    );
    assert_eq!(
        command_programs_for_shell(
            "cargo +nightly --config net.git-fetch-with-cli=true sqlx prepare --check"
        ),
        vec!["cargo"]
    );
    assert_eq!(
        command_programs_for_shell("/opt/jig/bin/cargo sqlx prepare -D sqlite:doctor.db"),
        vec!["/opt/jig/bin/cargo"]
    );
    assert_eq!(
        command_programs_for_shell("/opt/jig/bin/sqlx prepare -D sqlite:doctor.db"),
        vec!["/opt/jig/bin/sqlx"]
    );
    assert_eq!(
        command_programs_for_shell("/opt/jig/bin/cargo-sqlx sqlx prepare -D sqlite:doctor.db"),
        vec!["/opt/jig/bin/cargo-sqlx"]
    );
    assert_eq!(
        command_programs_for_shell("command -- cargo sqlx prepare -D sqlite:doctor.db"),
        vec!["cargo"]
    );
    assert_eq!(
        command_programs_for_shell("exec -- sqlx prepare -D sqlite:doctor.db"),
        vec!["sqlx"]
    );
    assert_eq!(
        command_programs_for_shell("nohup -- cargo-sqlx sqlx prepare -D sqlite:doctor.db"),
        vec!["nohup", "cargo-sqlx"]
    );
    assert_eq!(
        command_programs_for_shell(
            "command -v cargo >/dev/null && cargo sqlx prepare -D sqlite:doctor.db"
        ),
        vec!["cargo"]
    );
    assert_eq!(
        command_programs_for_shell("command -p cargo sqlx prepare -D sqlite:doctor.db"),
        vec!["cargo"]
    );
    assert_eq!(
        command_programs_for_shell("exec -a jig-cargo cargo sqlx prepare -D sqlite:doctor.db"),
        vec!["cargo"]
    );
    assert_eq!(
        command_programs_for_shell("exec -c cargo sqlx prepare -D sqlite:doctor.db"),
        vec!["cargo"]
    );
    assert!(
        command_programs_for_shell("exec -z cargo sqlx prepare -D sqlite:doctor.db").is_empty()
    );
    assert!(!cargo_sqlx_command_has_inline_config(
        "cargo --config net.git-fetch-with-cli=true sqlx prepare -D sqlite:doctor.db"
    ));
    assert!(cargo_sqlx_command_has_inline_config(
        "cargo --config alias.sqlx='run --package fake' sqlx prepare -D sqlite:doctor.db"
    ));
    assert!(cargo_sqlx_command_has_inline_config(
        "cargo --config include='dispatch.toml' sqlx prepare -D sqlite:doctor.db"
    ));
}

#[test]
fn cargo_dispatch_detects_command_local_alias_and_home_environment_changes() {
    for command in [
        "CARGO_ALIAS_SQLX='run --package fake' cargo sqlx prepare",
        "CARGO_HOME=/tmp/cargo-home cargo sqlx prepare",
        "CARGO_HOME[0]=/tmp/cargo-home cargo sqlx prepare",
        "env HOME=/tmp/home cargo sqlx prepare",
        "env -u USERPROFILE cargo sqlx prepare",
        "env --unset=HOMEDRIVE cargo sqlx prepare",
        "env -i cargo sqlx prepare",
        "export CARGO_HOME=/tmp/cargo-home; cargo sqlx prepare",
        "declare -x CARGO_HOME=/tmp/cargo-home; cargo sqlx prepare",
        "builtin declare -x CARGO_HOME=/tmp/cargo-home; cargo sqlx prepare",
        "declare -x CARGO_ALIAS_SQLX='run --package fake'; cargo sqlx prepare",
        "read CARGO_HOME <<< /tmp/cargo-home; cargo sqlx prepare",
        "printf -v CARGO_HOME %s /tmp/cargo-home; cargo sqlx prepare",
        "printf -v 'CARGO_HOME[0]' %s /tmp/cargo-home; cargo sqlx prepare",
        "mapfile -t CARGO_HOME </dev/null; cargo sqlx prepare",
        "getopts p CARGO_HOME; cargo sqlx prepare",
        "let CARGO_HOME=0; cargo sqlx prepare",
        "declare -n cargo_home_ref=CARGO_HOME; cargo sqlx prepare",
        "unset HOMEPATH; cargo sqlx prepare",
        "HOME=/tmp/home; cargo sqlx prepare",
        "exec -c cargo sqlx prepare",
    ] {
        assert!(
            cargo_sqlx_command_changes_dispatch_environment(command),
            "{command:?}",
        );
    }

    for command in [
        "OTHER_VALUE=/tmp cargo sqlx prepare",
        "env -u OTHER_VALUE cargo sqlx prepare",
        "printf '%s' HOME=/tmp/home; cargo sqlx prepare",
        "env export CARGO_HOME=/tmp/home; cargo sqlx prepare",
    ] {
        assert!(
            !cargo_sqlx_command_changes_dispatch_environment(command),
            "{command:?}",
        );
    }
}

#[test]
fn sqlx_driver_discovery_honors_command_environment_and_dotenv_precedence() {
    let temp = tempdir().unwrap();
    fs::write(
        temp.path().join(".env"),
        "DATABASE_URL=postgres://user:private-password@localhost/demo\n",
    )
    .unwrap();
    fs::write(
        temp.path().join(".env.example"),
        "DATABASE_URL=sqlite:example.db\n",
    )
    .unwrap();

    assert_eq!(
        configured_sqlx_driver(temp.path(), "cargo sqlx prepare --check", None),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Postgres,
            source: SqlxDriverSource::Dotenv,
        })
    );

    assert_eq!(
        configured_sqlx_driver(
            temp.path(),
            "cargo sqlx prepare --check",
            Some(OsStr::new("sqlite:environment.db")),
        ),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Sqlite,
            source: SqlxDriverSource::Environment,
        })
    );
    assert_eq!(
        configured_sqlx_driver(
            temp.path(),
            "DATABASE_URL=sqlite:assignment.db cargo sqlx prepare --check --database-url postgres://flag-user:flag-password@localhost/demo",
            Some(OsStr::new("sqlite:environment.db")),
        ),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Postgres,
            source: SqlxDriverSource::CommandFlag,
        })
    );
    assert_eq!(
        configured_sqlx_driver(
            temp.path(),
            "env DATABASE_URL=postgres://assignment-user:assignment-password@localhost/demo cargo sqlx prepare --check",
            None,
        ),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Postgres,
            source: SqlxDriverSource::CommandAssignment,
        })
    );
    for command in [
        "! DATABASE_URL=sqlite:negated.db sqlx prepare --check",
        "! env -i DATABASE_URL=sqlite:negated-env.db sqlx prepare --check",
    ] {
        assert_eq!(
            configured_sqlx_driver(
                temp.path(),
                command,
                Some(OsStr::new("postgres://localhost/ambient")),
            ),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::CommandAssignment,
            }),
            "{command:?}",
        );
    }
    for command in [
        "command env DATABASE_URL=postgres://localhost/command-env cargo sqlx prepare --check",
        "builtin command env DATABASE_URL=postgres://localhost/builtin-command-env cargo sqlx prepare --check",
        "exec env DATABASE_URL=postgres://localhost/exec-env cargo sqlx prepare --check",
        "env nohup env DATABASE_URL=postgres://localhost/nested-env cargo sqlx prepare --check",
    ] {
        assert_eq!(
            configured_sqlx_driver(temp.path(), command, None),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Postgres,
                source: SqlxDriverSource::CommandAssignment,
            }),
            "{command:?}",
        );
    }
    assert_eq!(
        configured_sqlx_driver(
            temp.path(),
            "cargo sqlx prepare --check --database-url=postgres://flag-user:flag-password@localhost/demo",
            None,
        ),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Postgres,
            source: SqlxDriverSource::CommandFlag,
        })
    );

    fs::remove_file(temp.path().join(".env")).unwrap();
    assert_eq!(
        configured_sqlx_driver(temp.path(), "cargo sqlx prepare --check", None),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Sqlite,
            source: SqlxDriverSource::DotenvExample,
        })
    );
}

#[test]
fn sqlx_driver_discovery_does_not_cross_post_assignment_keywords() {
    let temp = tempdir().unwrap();
    for command in [
        "DATABASE_URL=sqlite:x ! sqlx prepare",
        "! DATABASE_URL=sqlite:x ! sqlx prepare",
        "DATABASE_URL=sqlite:x then sqlx prepare",
    ] {
        assert!(
            matches!(
                configured_sqlx_driver(temp.path(), command, None),
                SqlxDriverResolution::Indeterminate(_)
            ),
            "{command:?}",
        );
    }

    assert_eq!(
        configured_sqlx_driver(temp.path(), "! DATABASE_URL=sqlite:x sqlx prepare", None,),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Sqlite,
            source: SqlxDriverSource::CommandAssignment,
        }),
    );
    for command in [
        "''! DATABASE_URL=sqlite:x sqlx prepare",
        r"\! DATABASE_URL=sqlite:x sqlx prepare",
    ] {
        assert!(
            matches!(
                configured_sqlx_driver(temp.path(), command, None),
                SqlxDriverResolution::Indeterminate(_)
            ),
            "{command:?}",
        );
    }
}

#[test]
fn sqlx_driver_discovery_stops_at_the_nearest_existing_dotenv() {
    let temp = tempdir().unwrap();
    let child = temp.path().join("crates/api");
    fs::create_dir_all(&child).unwrap();
    fs::write(
        temp.path().join(".env"),
        "DATABASE_URL=postgres://parent-user:parent-secret@localhost/demo\n",
    )
    .unwrap();
    fs::write(
        temp.path().join(".env.example"),
        "DATABASE_URL=sqlite:example-secret.db\n",
    )
    .unwrap();

    for contents in ["UNRELATED_SECRET=child-secret\n", "\n"] {
        fs::write(child.join(".env"), contents).unwrap();
        assert_eq!(
            configured_sqlx_driver_fallback(temp.path(), &child, None, false),
            SqlxDriverResolution::Absent,
            "nearest dotenv contents were {contents:?}",
        );
    }

    fs::write(child.join(".env"), "DATABASE_URL='unterminated\n").unwrap();
    assert!(matches!(
        configured_sqlx_driver_fallback(temp.path(), &child, None, false),
        SqlxDriverResolution::Indeterminate(reason)
            if reason == "a dotenv file could not be parsed safely"
    ));

    fs::remove_file(child.join(".env")).unwrap();
    assert_eq!(
        configured_sqlx_driver_fallback(temp.path(), &child, None, false),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Postgres,
            source: SqlxDriverSource::Dotenv,
        })
    );

    fs::remove_file(temp.path().join(".env")).unwrap();
    assert_eq!(
        configured_sqlx_driver_fallback(temp.path(), &child, None, false),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Sqlite,
            source: SqlxDriverSource::DotenvExample,
        })
    );
}

#[test]
fn sqlx_driver_discovery_fails_open_for_dynamic_and_ambiguous_commands() {
    let temp = tempdir().unwrap();
    let ambient = Some(OsStr::new("sqlite:environment.db"));

    assert!(matches!(
        configured_sqlx_driver(
            temp.path(),
            "cargo sqlx prepare --database-url '$OTHER_DATABASE_URL'",
            ambient,
        ),
        SqlxDriverResolution::Indeterminate(_)
    ));
    assert!(matches!(
        configured_sqlx_driver(
            temp.path(),
            "cargo sqlx prepare --database-url '$DATABASE_URL'",
            ambient,
        ),
        SqlxDriverResolution::Indeterminate(_)
    ));
    assert!(matches!(
        configured_sqlx_driver(
            temp.path(),
            r"cargo sqlx prepare --database-url \$DATABASE_URL",
            ambient,
        ),
        SqlxDriverResolution::Indeterminate(_)
    ));
    assert_eq!(
        configured_sqlx_driver(
            temp.path(),
            "cargo sqlx prepare --database-url \"$DATABASE_URL\"",
            ambient,
        ),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Sqlite,
            source: SqlxDriverSource::CommandFlag,
        })
    );
    for command in [
        "cargo sqlx prepare --database-url $DATABASE_URL",
        "cargo sqlx prepare --database-url=$DATABASE_URL",
        "cargo sqlx prepare -D$DATABASE_URL",
    ] {
        assert!(
            matches!(
                configured_sqlx_driver(temp.path(), command, ambient),
                SqlxDriverResolution::Indeterminate(reason)
                    if reason.contains("unquoted DATABASE_URL")
            ),
            "{command:?}",
        );
    }
    assert_eq!(
        configured_sqlx_driver(
            temp.path(),
            "DATABASE_URL=$DATABASE_URL cargo sqlx prepare",
            ambient,
        ),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Sqlite,
            source: SqlxDriverSource::CommandAssignment,
        })
    );
    fs::write(
        temp.path().join(".env"),
        "DATABASE_URL=postgres://dotenv-must-not-be-used/doctor\n",
    )
    .unwrap();
    assert!(matches!(
        configured_sqlx_driver(
            temp.path(),
            "cargo sqlx prepare --database-url '$DATABASE_URL'",
            None,
        ),
        SqlxDriverResolution::Indeterminate(_)
    ));
    assert_eq!(
        configured_sqlx_driver(
            temp.path(),
            "DATABASE_URL=sqlite:first.db cargo sqlx prepare && printf ignored && cargo sqlx migrate info --database-url=sqlite:second.db",
            None,
        ),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Sqlite,
            source: SqlxDriverSource::CommandAssignment,
        })
    );
    assert!(matches!(
        configured_sqlx_driver(
            temp.path(),
            "DATABASE_URL=sqlite:first.db cargo sqlx prepare && cargo sqlx migrate info --database-url=postgres://localhost/second",
            None,
        ),
        SqlxDriverResolution::Indeterminate(_)
    ));
    assert!(matches!(
        configured_sqlx_driver(
            temp.path(),
            "export DATABASE_URL=postgres://localhost/demo && cargo sqlx prepare",
            None,
        ),
        SqlxDriverResolution::Indeterminate(_)
    ));
    assert!(matches!(
        configured_sqlx_driver(
            temp.path(),
            "DATABASE_URL=postgres://localhost/demo && cargo sqlx prepare",
            None,
        ),
        SqlxDriverResolution::Indeterminate(_)
    ));
    assert!(matches!(
        configured_sqlx_driver(
            temp.path(),
            "env -u DATABASE_URL cargo sqlx prepare",
            ambient,
        ),
        SqlxDriverResolution::Indeterminate(_)
    ));
    assert!(matches!(
        configured_sqlx_driver(temp.path(), "env - cargo sqlx prepare", ambient,),
        SqlxDriverResolution::Indeterminate(_)
    ));
    assert!(matches!(
        configured_sqlx_driver(
            temp.path(),
            "cargo sqlx prepare --database-url='postgres://localhost/demo",
            None,
        ),
        SqlxDriverResolution::Indeterminate(_)
    ));
    assert_eq!(
        configured_sqlx_driver(
            temp.path(),
            "printf DATABASE_URL=postgres://ignored && DATABASE_URL=sqlite:actual.db cargo sqlx prepare",
            None,
        ),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Sqlite,
            source: SqlxDriverSource::CommandAssignment,
        })
    );
}

#[test]
fn sqlx_driver_discovery_supports_cli_variants_short_flags_and_continuations() {
    let temp = tempdir().unwrap();
    for command in [
        "cargo sqlx prepare -D sqlite:first.db",
        "command /opt/bin/cargo sqlx prepare -Dsqlite:second.db",
        "exec sqlx prepare -D=sqlite:third.db",
        "nohup cargo-sqlx sqlx prepare --database-url sqlite:fourth.db",
        "cargo \\\n             sqlx prepare -D \\\r\n             sqlite:fifth.db",
        "command -- cargo sqlx prepare -D sqlite:sixth.db",
        "exec -- sqlx prepare -D sqlite:seventh.db",
        "nohup -- cargo-sqlx sqlx prepare -D sqlite:eighth.db",
    ] {
        assert_eq!(
            configured_sqlx_driver(temp.path(), command, None),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::CommandFlag,
            }),
            "{command:?}",
        );
    }
}

#[test]
fn sqlx_driver_discovery_ignores_comments_and_preserves_literal_hashes() {
    let temp = tempdir().unwrap();
    let ambient = Some(OsStr::new("sqlite:environment.db"));

    assert_eq!(
        configured_sqlx_driver(
            temp.path(),
            "cargo sqlx prepare # -D postgres://doctor-user:comment-secret@localhost/demo",
            ambient,
        ),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Sqlite,
            source: SqlxDriverSource::Environment,
        })
    );
    for command in [
        "cargo sqlx prepare -D sqlite:doctor.db#in-word",
        r"cargo sqlx prepare -D sqlite:doctor.db\#escaped",
        "cargo sqlx prepare -D 'sqlite:doctor.db#quoted'",
    ] {
        assert_eq!(
            configured_sqlx_driver(temp.path(), command, None),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::CommandFlag,
            }),
            "{command:?}",
        );
    }
}

#[test]
fn shell_parser_preserves_empty_quote_word_boundaries_before_hashes() {
    assert_eq!(command_programs_for_shell("''#foo"), vec!["#foo"]);
    assert_eq!(
        command_programs_for_shell("# ignored\ncargo test"),
        vec!["cargo"]
    );
    assert!(command_programs_for_shell("'' cargo sqlx prepare -D sqlite:wrong.db").is_empty());

    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(temp.path(), "'' cargo sqlx prepare -D sqlite:wrong.db");
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
    let check = required_tools_check_with_environment(
        &ctx,
        &DoctorEnvironment {
            search_path: Some(OsString::new()),
            ..DoctorEnvironment::default()
        },
    );

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present_unverified");
    assert!(check.detail.contains("scripts/jig check sqlx"));
}

#[test]
fn shell_parser_preserves_assignment_name_and_io_number_provenance() {
    let temp = tempdir().unwrap();
    assert_eq!(
        configured_sqlx_driver(
            temp.path(),
            "DATABASE_URL='sqlite:actual.db' cargo sqlx prepare",
            None,
        ),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Sqlite,
            source: SqlxDriverSource::CommandAssignment,
        })
    );
    for command in [
        "'2'>out cargo sqlx prepare -D sqlite:ignored.db",
        r"\2>out cargo sqlx prepare -D sqlite:ignored.db",
        "''DATABASE_URL=sqlite:ignored.db cargo sqlx prepare",
        "'DATABASE_URL'=sqlite:ignored.db cargo sqlx prepare",
        "''! cargo sqlx prepare -D sqlite:ignored.db",
        "''! DATABASE_URL=sqlite:ignored.db sqlx prepare",
        r"\! DATABASE_URL=sqlite:ignored.db sqlx prepare",
    ] {
        assert!(
            matches!(
                configured_sqlx_driver(temp.path(), command, None),
                SqlxDriverResolution::Indeterminate(_)
            ),
            "{command:?}",
        );
    }
}

#[test]
fn shell_parser_ignores_heredoc_bodies() {
    let temp = tempdir().unwrap();
    let unquoted_substitution = "cat <<EOF\n$(missing-helper)\nEOF";
    let unquoted = required_command_programs_for_shell(unquoted_substitution);
    assert_eq!(
        unquoted.ambiguity,
        Some(RequiredProgramAmbiguity::ShellSyntax)
    );
    assert_eq!(unquoted.programs[0].program, "cat");
    assert_eq!(
        unquoted.programs[0].path_lookup,
        ProgramPathLookup::Unverifiable
    );
    assert!(matches!(
        configured_sqlx_driver(temp.path(), unquoted_substitution, None),
        SqlxDriverResolution::Indeterminate(_)
    ));

    for inert in [
        "cat <<'EOF'\n$(missing-helper)\nEOF",
        "cat <<\\EOF\n$(missing-helper)\nEOF",
        "cat <<EOF\n\\$(missing-helper)\nEOF",
    ] {
        let discovery = required_command_programs_for_shell(inert);
        assert!(discovery.ambiguity.is_none(), "{inert:?}");
        assert_eq!(discovery.programs[0].program, "cat", "{inert:?}");
        assert_eq!(
            discovery.programs[0].path_lookup,
            ProgramPathLookup::Captured,
            "{inert:?}",
        );
    }

    let command = "cat <<'PAYLOAD'\nDATABASE_URL=postgres://body-secret cargo sqlx prepare -D postgres://body-secret\nPAYLOAD\ncargo sqlx prepare -D sqlite:actual.db";

    assert_eq!(command_programs_for_shell(command), vec!["cat", "cargo"]);
    assert_eq!(
        configured_sqlx_driver(temp.path(), command, None),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Sqlite,
            source: SqlxDriverSource::CommandFlag,
        })
    );

    let tab_stripped = "cat <<-EOF\n\tcargo sqlx prepare -D postgres://ignored\n\tEOF\nsqlx prepare -D sqlite:actual.db";
    assert_eq!(
        command_programs_for_shell(tab_stripped),
        vec!["cat", "sqlx"]
    );
    assert_eq!(
        configured_sqlx_driver(temp.path(), tab_stripped, None),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Sqlite,
            source: SqlxDriverSource::CommandFlag,
        })
    );

    for tab_stripped in [
        "cat <<- EOF\n\tcargo sqlx prepare -D postgres://ignored\n\tEOF\nsqlx prepare -D sqlite:actual.db",
        "cat <<-EOF\n\tcargo sqlx prepare -D postgres://ignored\n\tEOF\nsqlx prepare -D sqlite:actual.db",
    ] {
        assert_eq!(
            command_programs_for_shell(tab_stripped),
            vec!["cat", "sqlx"]
        );
        assert_eq!(
            configured_sqlx_driver(temp.path(), tab_stripped, None),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::CommandFlag,
            })
        );
    }

    let multiple_crlf = "cat <<ONE <<-'TWO'\r\ncargo sqlx prepare -D postgres://first\r\nONE\r\n\tcargo sqlx prepare -D postgres://second\r\n\tTWO\r\nsqlx prepare -D sqlite:actual.db";
    assert_eq!(
        command_programs_for_shell(multiple_crlf),
        vec!["cat", "sqlx"]
    );
    assert_eq!(
        configured_sqlx_driver(temp.path(), multiple_crlf, None),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Sqlite,
            source: SqlxDriverSource::CommandFlag,
        })
    );

    let unterminated = "cat <<EOF\ncargo sqlx prepare -D postgres://body-secret";
    assert_eq!(command_programs_for_shell(unterminated), vec!["cat"]);
    assert!(matches!(
        configured_sqlx_driver(temp.path(), unterminated, None),
        SqlxDriverResolution::Indeterminate(_)
    ));
}

#[test]
fn shell_words_preserve_literal_edge_quotes_and_semicolons() {
    assert_eq!(
        command_programs_for_shell(r#"'"sqlx"' prepare -D sqlite:ignored.db"#),
        vec!["\"sqlx\""]
    );
    assert_eq!(
        command_programs_for_shell("'sqlx;' prepare -D sqlite:ignored.db"),
        vec!["sqlx;"]
    );
    assert_eq!(
        command_programs_for_shell("'sqlx' prepare -D sqlite:actual.db"),
        vec!["sqlx"]
    );
}

#[test]
fn sqlx_driver_discovery_detects_later_database_url_mutations() {
    let temp = tempdir().unwrap();
    fs::write(temp.path().join(".env"), "DATABASE_URL=sqlite:root.db\n").unwrap();

    for command in [
        "FOO=one DATABASE_URL=postgres://localhost/assignment && cargo sqlx prepare",
        "DATABASE_URL[0]=postgres://localhost/array-assignment && cargo sqlx prepare",
        "FOO=one export DATABASE_URL=postgres://localhost/exported && cargo sqlx prepare",
        "command export DATABASE_URL=postgres://localhost/exported && cargo sqlx prepare",
        "builtin export DATABASE_URL=postgres://localhost/exported && cargo sqlx prepare",
        "builtin command export DATABASE_URL=postgres://localhost/exported && cargo sqlx prepare",
        "declare -x DATABASE_URL=postgres://localhost/declared && cargo sqlx prepare",
        "builtin declare -x DATABASE_URL=postgres://localhost/declared && cargo sqlx prepare",
        "read DATABASE_URL <<< postgres://localhost/read && cargo sqlx prepare",
        "builtin read DATABASE_URL </dev/null && cargo sqlx prepare",
        "printf -v DATABASE_URL %s postgres://localhost/printf && cargo sqlx prepare",
        "builtin printf -v DATABASE_URL %s postgres://localhost/printf && cargo sqlx prepare",
        "declare -n database_ref=DATABASE_URL && cargo sqlx prepare",
        "printf -v 'DATABASE_URL[0]' %s postgres://localhost/array-printf && cargo sqlx prepare",
        "printf '-vDATABASE_URL[0]' %s postgres://localhost/array-printf && cargo sqlx prepare",
        "read 'DATABASE_URL[0]' </dev/null && cargo sqlx prepare",
        "read '-aDATABASE_URL[0]' </dev/null && cargo sqlx prepare",
        "declare 'DATABASE_URL[0]=postgres://localhost/array-declare' && cargo sqlx prepare",
        "unset 'DATABASE_URL[0]' && cargo sqlx prepare",
        "mapfile -t DATABASE_URL </dev/null && cargo sqlx prepare",
        "readarray DATABASE_URL </dev/null && cargo sqlx prepare",
        "set -- -p; getopts p DATABASE_URL; cargo sqlx prepare",
        "let DATABASE_URL=0; cargo sqlx prepare",
    ] {
        assert!(
            matches!(
                configured_sqlx_driver(temp.path(), command, None),
                SqlxDriverResolution::Indeterminate(_)
            ),
            "{command:?}",
        );
    }
    assert_eq!(
        configured_sqlx_driver(
            temp.path(),
            "printf '%s' DATABASE_URL=postgres://ignored && cargo sqlx prepare -D sqlite:actual.db",
            None,
        ),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Sqlite,
            source: SqlxDriverSource::CommandFlag,
        })
    );
    assert_eq!(
        configured_sqlx_driver(
            temp.path(),
            "PATH=./repo-tools cargo sqlx prepare -D sqlite:actual.db",
            None,
        ),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Sqlite,
            source: SqlxDriverSource::CommandFlag,
        })
    );
}

#[test]
fn sqlx_driver_discovery_respects_database_url_scrub_order() {
    let temp = tempdir().unwrap();
    fs::write(temp.path().join(".env"), "DATABASE_URL=sqlite:dotenv.db\n").unwrap();
    let ambient = Some(OsStr::new("sqlite:ambient.db"));

    for command in [
        "DATABASE_URL=postgres://localhost/before env -i cargo sqlx prepare",
        "DATABASE_URL=postgres://localhost/before env --ignore-environment cargo sqlx prepare",
        "DATABASE_URL=postgres://localhost/before env -u DATABASE_URL cargo sqlx prepare",
        "DATABASE_URL=postgres://localhost/before env -uDATABASE_URL cargo sqlx prepare",
        "DATABASE_URL=postgres://localhost/before env --unset DATABASE_URL cargo sqlx prepare",
        "DATABASE_URL=postgres://localhost/before env --unset=DATABASE_URL cargo sqlx prepare",
        "DATABASE_URL=postgres://localhost/before exec -c cargo sqlx prepare",
        "DATABASE_URL+=postgres://localhost/appended cargo sqlx prepare",
    ] {
        assert!(
            matches!(
                configured_sqlx_driver(temp.path(), command, ambient),
                SqlxDriverResolution::Indeterminate(_)
            ),
            "{command:?}",
        );
    }

    for command in [
        "env -i DATABASE_URL=postgres://localhost/after cargo sqlx prepare",
        "DATABASE_URL=sqlite:before.db env -i DATABASE_URL=postgres://localhost/after cargo sqlx prepare",
        "DATABASE_URL=sqlite:before.db exec -c env DATABASE_URL=postgres://localhost/after cargo sqlx prepare",
        "DATABASE_URL+=ignored DATABASE_URL=postgres://localhost/after cargo sqlx prepare",
    ] {
        assert_eq!(
            configured_sqlx_driver(temp.path(), command, ambient),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Postgres,
                source: SqlxDriverSource::CommandAssignment,
            }),
            "{command:?}",
        );
    }

    assert_eq!(
        configured_sqlx_driver(
            temp.path(),
            "DATABASE_URL=postgres://localhost/before env -i cargo sqlx prepare -D sqlite:explicit.db",
            ambient,
        ),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Sqlite,
            source: SqlxDriverSource::CommandFlag,
        })
    );
}

#[test]
fn sqlx_driver_discovery_does_not_treat_wrapper_operands_as_assignments() {
    let temp = tempdir().unwrap();
    let ambient = Some(OsStr::new("postgres://localhost/ambient"));

    for command in [
        "env -C DATABASE_URL=sqlite:option.db cargo sqlx prepare",
        "env --chdir DATABASE_URL=sqlite:option.db cargo sqlx prepare",
        "command DATABASE_URL=sqlite:target.db cargo sqlx prepare",
        "exec DATABASE_URL=sqlite:target.db cargo sqlx prepare",
        "nohup DATABASE_URL=sqlite:target.db cargo sqlx prepare",
    ] {
        assert!(
            matches!(
                configured_sqlx_driver(temp.path(), command, ambient),
                SqlxDriverResolution::Indeterminate(_)
            ),
            "{command:?}",
        );
    }
}

#[test]
fn sqlx_driver_discovery_rejects_mixed_known_and_absent_requirements() {
    let temp = tempdir().unwrap();
    assert!(matches!(
        configured_sqlx_driver(
            temp.path(),
            "cargo sqlx prepare -D sqlite:known.db && cargo sqlx prepare --no-dotenv",
            None,
        ),
        SqlxDriverResolution::Indeterminate(reason)
            if reason.contains("some SQLx invocations")
    ));
}

#[test]
fn sqlx_driver_discovery_models_wrapper_options_conservatively() {
    let temp = tempdir().unwrap();
    let ambient = Some(OsStr::new("sqlite:environment.db"));

    assert_eq!(
        configured_sqlx_driver(
            temp.path(),
            "command -v cargo >/dev/null && cargo sqlx prepare -D sqlite:checked.db",
            ambient,
        ),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Sqlite,
            source: SqlxDriverSource::CommandFlag,
        })
    );
    for command in [
        "command -p cargo sqlx prepare -D sqlite:default-path.db",
        "exec -a jig-cargo cargo sqlx prepare -D sqlite:custom-argv-zero.db",
        "exec -z cargo sqlx prepare -D sqlite:unsupported-option.db",
    ] {
        assert!(
            matches!(
                configured_sqlx_driver(temp.path(), command, ambient),
                SqlxDriverResolution::Indeterminate(_)
            ),
            "{command:?}",
        );
    }
    assert!(matches!(
        configured_sqlx_driver(temp.path(), "exec -c cargo sqlx prepare", ambient,),
        SqlxDriverResolution::Indeterminate(_)
    ));
    assert_eq!(
        configured_sqlx_driver(
            temp.path(),
            "exec -c cargo sqlx prepare -D sqlite:explicit.db",
            ambient,
        ),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Sqlite,
            source: SqlxDriverSource::CommandFlag,
        })
    );
}

#[test]
fn sqlx_driver_discovery_uses_literal_cd_dotenv_and_rejects_unsafe_cwd() {
    let temp = tempdir().unwrap();
    let child = temp.path().join("crates/api");
    fs::create_dir_all(&child).unwrap();
    fs::write(temp.path().join(".env"), "DATABASE_URL=sqlite:root.db\n").unwrap();
    fs::write(
        child.join(".env"),
        "DATABASE_URL=postgres://localhost/child\n",
    )
    .unwrap();
    let parsed = parse_shell_commands("cd crates/api && cargo sqlx prepare");
    assert_eq!(parsed.separators.first(), Some(&ShellSeparator::And));
    let resolved = resolve_literal_cd(temp.path(), temp.path(), &parsed.commands[0]).unwrap();
    assert_eq!(resolved, fs::canonicalize(&child).unwrap());
    assert!(
        database_url_from_dotenv(&resolved.join(".env"))
            .unwrap()
            .is_some()
    );
    assert_eq!(
        configured_sqlx_driver_fallback(temp.path(), &resolved, None, false),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Postgres,
            source: SqlxDriverSource::Dotenv,
        })
    );

    assert_eq!(
        configured_sqlx_driver(temp.path(), "cd crates/api && cargo sqlx prepare", None,),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Postgres,
            source: SqlxDriverSource::Dotenv,
        })
    );
    for command in [
        "cd $APP_DIR && cargo sqlx prepare",
        "cd crates/api; cargo sqlx prepare",
        "CDPATH= cd crates/api && cargo sqlx prepare",
        "command cd crates/api && cargo sqlx prepare",
        "env -C crates/api cargo sqlx prepare",
        "env -Ccrates/api cargo sqlx prepare",
        "cargo -Ccrates/api sqlx prepare",
    ] {
        assert!(matches!(
            configured_sqlx_driver(temp.path(), command, None),
            SqlxDriverResolution::Indeterminate(_)
        ));
    }
    assert!(matches!(
        configured_sqlx_driver(
            temp.path(),
            "cd crates/api && cargo sqlx prepare --no-dotenv",
            None,
        ),
        SqlxDriverResolution::Indeterminate(_)
    ));
}

#[test]
fn sqlx_driver_discovery_sees_parent_dotenv_before_local_example() {
    let outer = tempdir().unwrap();
    let root = outer.path().join("repo");
    fs::create_dir(&root).unwrap();
    fs::write(
        outer.path().join(".env"),
        "DATABASE_URL=postgres://localhost/parent\n",
    )
    .unwrap();
    fs::write(
        root.join(".env.example"),
        "DATABASE_URL=sqlite:local-example.db\n",
    )
    .unwrap();

    assert!(matches!(
        configured_sqlx_driver_fallback(&root, &root, None, false),
        SqlxDriverResolution::Indeterminate(reason)
            if reason.contains("above the Jig repository")
    ));
}

#[test]
fn sqlx_driver_discovery_accepts_bom_prefixed_dotenv_hints() {
    let temp = tempdir().unwrap();
    fs::write(
        temp.path().join(".env"),
        b"\xef\xbb\xbfDATABASE_URL=postgres://localhost/bom\n",
    )
    .unwrap();
    assert_eq!(
        configured_sqlx_driver_fallback(temp.path(), temp.path(), None, false),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Postgres,
            source: SqlxDriverSource::Dotenv,
        })
    );

    fs::remove_file(temp.path().join(".env")).unwrap();
    fs::write(
        temp.path().join(".env.example"),
        b"\xef\xbb\xbfDATABASE_URL=sqlite:bom.db\n",
    )
    .unwrap();
    assert_eq!(
        configured_sqlx_driver_fallback(temp.path(), temp.path(), None, false),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Sqlite,
            source: SqlxDriverSource::DotenvExample,
        })
    );
}

#[test]
fn sqlx_driver_discovery_rejects_dotenv_substitution_without_using_ambient_helpers() {
    let _env = lock_env();
    let helper = "JIG_DOCTOR_DOTENV_HELPER_DO_NOT_LEAK";
    let secret = "ambient-substitution-secret";
    let _helper = EnvVarGuard::set(helper, secret);
    let temp = tempdir().unwrap();

    fs::write(
        temp.path().join(".env"),
        format!("DATABASE_URL=${helper}:private.db\n"),
    )
    .unwrap();
    let dotenv = configured_sqlx_driver_fallback(temp.path(), temp.path(), None, false);
    assert!(matches!(
        dotenv,
        SqlxDriverResolution::Indeterminate(reason)
            if reason.contains("variable substitution")
    ));

    fs::remove_file(temp.path().join(".env")).unwrap();
    fs::write(
        temp.path().join(".env.example"),
        format!("DATABASE_URL=${{{helper}}}:private.db\n"),
    )
    .unwrap();
    let example = configured_sqlx_driver_fallback(temp.path(), temp.path(), None, false);
    assert!(matches!(
        example,
        SqlxDriverResolution::Indeterminate(reason)
            if reason.contains("variable substitution")
    ));

    for resolution in [dotenv, example] {
        assert!(!format!("{resolution:?}").contains(secret));
    }
}

#[test]
fn sqlx_driver_discovery_preserves_literal_dotenv_dollars() {
    let temp = tempdir().unwrap();
    for value in ["'sqlite:literal-$HELPER.db'", "sqlite:escaped-\\$HELPER.db"] {
        fs::write(temp.path().join(".env"), format!("DATABASE_URL={value}\n")).unwrap();
        assert_eq!(
            configured_sqlx_driver_fallback(temp.path(), temp.path(), None, false),
            SqlxDriverResolution::Known(SqlxDriverRequirement {
                driver: SqlxDriver::Sqlite,
                source: SqlxDriverSource::Dotenv,
            }),
            "{value:?}",
        );
    }
}

#[test]
fn dotenv_database_url_uses_the_platform_key_matching_rule() {
    assert!(dotenv_database_url_key("DATABASE_URL"));
    assert_eq!(dotenv_database_url_key("database_url"), cfg!(windows));

    let temp = tempdir().unwrap();
    fs::write(
        temp.path().join(".env"),
        "database_url=sqlite:first.db\nDATABASE_URL=postgres://localhost/second\n",
    )
    .unwrap();
    let database_url = database_url_from_dotenv(&temp.path().join(".env")).unwrap();
    #[cfg(windows)]
    assert_eq!(
        database_url,
        Some(DotenvDatabaseUrl::Literal("sqlite:first.db".into()))
    );
    #[cfg(not(windows))]
    assert_eq!(
        database_url,
        Some(DotenvDatabaseUrl::Literal(
            "postgres://localhost/second".into()
        ))
    );
}

#[test]
fn sqlx_driver_discovery_models_guarded_and_pipelined_cd_safely() {
    let temp = tempdir().unwrap();
    let child = temp.path().join("crates/api");
    fs::create_dir_all(&child).unwrap();
    fs::write(temp.path().join(".env"), "DATABASE_URL=sqlite:root.db\n").unwrap();
    fs::write(
        child.join(".env"),
        "DATABASE_URL=postgres://localhost/child\n",
    )
    .unwrap();

    assert_eq!(
        configured_sqlx_driver(
            temp.path(),
            "cd crates/api || exit 1; cargo sqlx prepare",
            None,
        ),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Postgres,
            source: SqlxDriverSource::Dotenv,
        })
    );
    for command in [
        "printf x | cd crates/api && cargo sqlx prepare",
        "true || cd crates/api && cargo sqlx prepare",
        "false && cd crates/api; cargo sqlx prepare",
    ] {
        assert!(
            matches!(
                configured_sqlx_driver(temp.path(), command, None),
                SqlxDriverResolution::Indeterminate(_)
            ),
            "{command:?}",
        );
    }
}

#[test]
fn sqlx_driver_discovery_normalizes_postgresql_alias() {
    assert_eq!(
        SqlxDriver::from_database_url("postgresql://localhost/demo"),
        Some(SqlxDriver::Postgres)
    );
    assert_eq!(
        SqlxDriver::from_database_url("SQLITE:demo.db"),
        Some(SqlxDriver::Sqlite)
    );
    assert_eq!(
        SqlxDriver::from_database_url("mysql://localhost/demo"),
        None
    );
}

#[test]
fn windows_executable_candidates_honor_validated_pathext_order() {
    let candidates = windows_executable_candidates(
        Path::new("/tools/cargo"),
        Some(OsStr::new(".exe;.CMD;../BAD;.Exe;.PS1")),
    );
    assert_eq!(
        candidates,
        vec![
            PathBuf::from("/tools/cargo"),
            PathBuf::from("/tools/cargo.EXE"),
            PathBuf::from("/tools/cargo.CMD"),
            PathBuf::from("/tools/cargo.PS1"),
        ]
    );
    assert_eq!(
        windows_search_path_executable_candidates(
            Path::new("/tools/cargo"),
            Some(OsStr::new(".exe;.CMD;../BAD;.Exe;.PS1")),
        ),
        vec![
            PathBuf::from("/tools/cargo.EXE"),
            PathBuf::from("/tools/cargo.CMD"),
            PathBuf::from("/tools/cargo.PS1"),
            PathBuf::from("/tools/cargo"),
        ]
    );
    assert_eq!(
        validated_windows_path_extensions(None),
        vec![".COM", ".EXE", ".BAT", ".CMD"]
    );
    assert!(executable_is_named("SQLX.ExE", "sqlx"));
    assert!(executable_is_named("cargo-sqlx.EXE", "cargo-sqlx"));
}

#[test]
fn sqlx_driver_probe_classifies_supported_and_missing_drivers() {
    assert_eq!(
        classify_sqlx_driver_probe(SqlxDriver::Sqlite, true, "", ""),
        SqlxDriverProbe::Compatible
    );
    assert_eq!(
        classify_sqlx_driver_probe(
            SqlxDriver::Postgres,
            false,
            "error: unknown value \"jig-doctor-invalid\" for `ssl_mode`",
            ""
        ),
        SqlxDriverProbe::Compatible
    );
    assert_eq!(
        classify_sqlx_driver_probe(
            SqlxDriver::Postgres,
            false,
            "error: invalid value 'jig-doctor-invalid' for sslmode",
            ""
        ),
        SqlxDriverProbe::Compatible
    );
    assert_eq!(
        classify_sqlx_driver_probe(
            SqlxDriver::Sqlite,
            false,
            "",
            "error: error with configuration: no driver found for URL scheme \"sqlite\""
        ),
        SqlxDriverProbe::Incompatible
    );
    assert!(matches!(
        classify_sqlx_driver_probe(SqlxDriver::Sqlite, false, "error: unexpected argument", ""),
        SqlxDriverProbe::Indeterminate(_)
    ));
    for (stdout, stderr) in [
        ("error: invalid value for ssl_mode", ""),
        ("error: invalid value 'jig-doctor-invalid'", ""),
        ("jig-doctor-invalid", "error: invalid value for ssl_mode"),
    ] {
        assert!(matches!(
            classify_sqlx_driver_probe(SqlxDriver::Postgres, false, stdout, stderr,),
            SqlxDriverProbe::Indeterminate(_)
        ));
    }
}

#[cfg(unix)]
#[test]
fn sqlx_driver_probe_invokes_shim_safely_and_times_out() {
    let _env = lock_env();
    let _secret = EnvVarGuard::set("JIG_DOCTOR_TEST_SECRET", "must-not-be-inherited");
    let _database_url = EnvVarGuard::set("DATABASE_URL", "postgres://must-not-be-inherited");
    let temp = tempdir().unwrap();
    let supported = temp.path().join("cargo-sqlx-supported");
    write_test_executable(
        &supported,
        "#!/bin/sh\n[ -z \"${JIG_DOCTOR_TEST_SECRET+x}\" ] || exit 8\n[ -z \"${DATABASE_URL+x}\" ] || exit 8\n[ \"$HOME\" = \"$USERPROFILE\" ] || exit 8\n[ \"$HOME\" = \"$TMPDIR\" ] || exit 8\n[ \"$HOME\" = \"$TMP\" ] || exit 8\n[ \"$HOME\" = \"$TEMP\" ] || exit 8\n[ \"$LC_ALL\" = C ] || exit 8\n[ \"$NO_COLOR\" = 1 ] || exit 8\n[ \"$1\" = sqlx ] || exit 9\nprintf '%s\\n' 'error: unknown value \"jig-doctor-invalid\" for ssl_mode'\nexit 1\n",
    );
    assert_eq!(
        probe_sqlx_driver_with_timeout(
            &supported,
            SqlxProbeStyle::CargoSubcommand,
            SqlxDriver::Postgres,
            Duration::from_secs(1)
        ),
        SqlxDriverProbe::Compatible
    );

    let direct = temp.path().join("sqlx-supported");
    write_test_executable(
        &direct,
        "#!/bin/sh\n[ \"$1\" = migrate ] || exit 9\nexit 0\n",
    );
    assert_eq!(
        probe_sqlx_driver_with_timeout(
            &direct,
            SqlxProbeStyle::Direct,
            SqlxDriver::Sqlite,
            Duration::from_secs(1),
        ),
        SqlxDriverProbe::Compatible
    );

    let repo = tempdir().unwrap();
    let tools = tempdir().unwrap();
    let unrelated = tempdir().unwrap();
    let tools = fs::canonicalize(tools.path()).unwrap();
    let path_limited = tools.join("sqlx-path-limited");
    write_test_executable(
        &path_limited,
        &format!(
            "#!/bin/sh\n[ \"$PATH\" = '{}' ] || exit 8\nexit 0\n",
            tools.display()
        ),
    );
    let broad_path = env::join_paths([tools.as_path(), unrelated.path()]).unwrap();
    assert_eq!(
        probe_sqlx_driver_with_timeout_and_environment(
            &path_limited,
            SqlxProbeStyle::Direct,
            SqlxDriver::Sqlite,
            Duration::from_secs(1),
            repo.path(),
            &DoctorEnvironment {
                search_path: Some(broad_path),
                ..DoctorEnvironment::default()
            },
        ),
        SqlxDriverProbe::Compatible
    );

    let hanging = temp.path().join("cargo-sqlx-hanging");
    write_test_executable(&hanging, "#!/bin/sh\nwhile :; do :; done\n");
    assert!(matches!(
        probe_sqlx_driver_with_timeout(
            &hanging,
            SqlxProbeStyle::CargoSubcommand,
            SqlxDriver::Sqlite,
            Duration::from_millis(20)
        ),
        SqlxDriverProbe::Indeterminate(reason) if reason.contains("timed out")
    ));

    let noisy = temp.path().join("cargo-sqlx-noisy");
    write_test_executable(
        &noisy,
        "#!/bin/sh\ni=0\nwhile [ \"$i\" -lt 5000 ]; do printf '0123456789abcdef0123456789abcdef' >&2; i=$((i + 1)); done\nexit 0\n",
    );
    let noisy_probe = probe_sqlx_driver_with_timeout(
        &noisy,
        SqlxProbeStyle::CargoSubcommand,
        SqlxDriver::Sqlite,
        Duration::from_secs(2),
    );
    assert!(
        matches!(
            &noisy_probe,
            SqlxDriverProbe::Indeterminate(reason) if reason.contains("capture limit")
        ),
        "unexpected noisy probe result: {noisy_probe:?}"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn sqlx_driver_probe_reaps_descendants_on_completion_and_timeout() {
    let temp = tempdir().unwrap();

    let completed_marker = temp.path().join("completed-descendant");
    let completed = temp.path().join("sqlx-completed");
    write_test_executable(
        &completed,
        &owned_test_descendant_script(&completed_marker, "exit 0"),
    );
    assert_eq!(
        probe_sqlx_driver_with_timeout(
            &completed,
            SqlxProbeStyle::Direct,
            SqlxDriver::Sqlite,
            Duration::from_secs(2),
        ),
        SqlxDriverProbe::Compatible
    );
    let completed_descendant = read_test_process_identity(&completed_marker);

    let timeout_marker = temp.path().join("timeout-descendant");
    let hanging = temp.path().join("sqlx-timeout-tree");
    write_test_executable(
        &hanging,
        &owned_test_descendant_script(&timeout_marker, "while :; do :; done"),
    );
    let timeout_probe = probe_sqlx_driver_with_timeout(
        &hanging,
        SqlxProbeStyle::Direct,
        SqlxDriver::Sqlite,
        Duration::from_millis(300),
    );
    assert!(
        matches!(
            &timeout_probe,
            SqlxDriverProbe::Indeterminate(reason) if reason == "the driver probe timed out"
        ),
        "unexpected timeout probe result: {timeout_probe:?}"
    );
    let timeout_descendant = read_test_process_identity(&timeout_marker);

    for descendant in [completed_descendant, timeout_descendant] {
        assert_test_process_stopped(&descendant);
    }
}

#[cfg(unix)]
// The scoped signal session must remain active until the explicit finish path
// restores handlers and re-delivers any recorded signal.
#[allow(clippy::significant_drop_tightening)]
#[test]
fn sqlx_driver_probe_sigint_helper() {
    let Some(executable) = std::env::var_os("JIG_SQLX_PROBE_SIGINT_HELPER") else {
        return;
    };
    let signal_session = DoctorSignalSession::start().unwrap();
    let cancelled = || signal_session.cancelled();
    let result = probe_sqlx_driver_with_timeout_and_environment_and_cancellation(
        Path::new(&executable),
        SqlxProbeStyle::Direct,
        SqlxDriver::Sqlite,
        Duration::from_secs(30),
        Path::new("/"),
        &DoctorEnvironment::default(),
        Some(&cancelled),
    );
    let _ = finish_doctor_signal_session(signal_session);
    panic!("SIGINT was not re-delivered after probe cleanup: {result:?}");
}

#[cfg(unix)]
#[test]
fn sqlx_probe_signal_finish_fails_closed_when_restoration_fails() {
    let signals = DoctorSignals {
        first: Some(libc::SIGINT),
        mask: doctor_signal_bit(libc::SIGINT),
    };
    assert_eq!(
        doctor_signal_finish_action(signals, true),
        DoctorSignalFinishAction::Redeliver(signals)
    );
    assert_eq!(
        doctor_signal_finish_action(signals, false),
        DoctorSignalFinishAction::Exit(128 + libc::SIGINT)
    );
    assert_eq!(
        doctor_signal_finish_action(DoctorSignals::default(), false),
        DoctorSignalFinishAction::Continue
    );
}

#[cfg(unix)]
// Session ownership deliberately spans signal delivery through restoration.
#[allow(clippy::significant_drop_tightening)]
#[test]
fn sqlx_probe_signal_session_redelivers_distinct_signals_once_after_restoration() {
    const HELPER: &str = "JIG_SQLX_PROBE_MIXED_SIGNAL_HELPER";
    if std::env::var_os(HELPER).is_none() {
        let status = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "doctor::tests::sqlx_probe_signal_session_redelivers_distinct_signals_once_after_restoration",
                    "--nocapture",
                ])
                .env(HELPER, "1")
                .status()
                .unwrap();
        assert!(status.success(), "mixed-signal helper exited with {status}");
        return;
    }

    SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.store(0, Ordering::SeqCst);
    SQLX_PROBE_TEST_REDELIVERED_SIGNAL_ORDER.store(0, Ordering::SeqCst);
    for signal in [libc::SIGINT, libc::SIGHUP, libc::SIGTERM] {
        // SAFETY: zero initializes the sigaction storage before its fields
        // and mask are populated below.
        let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
        action.sa_sigaction = record_sqlx_probe_test_redelivery as *const () as usize;
        action.sa_flags = 0;
        // SAFETY: the mask is writable storage owned by this test.
        assert_eq!(unsafe { libc::sigemptyset(&mut action.sa_mask) }, 0);
        // SAFETY: action is initialized and the helper subprocess owns its
        // process-wide dispositions for the remainder of this test.
        assert_eq!(
            unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) },
            0
        );
    }

    let session = DoctorSignalSession::start().unwrap();
    for signal in [
        libc::SIGINT,
        libc::SIGTERM,
        libc::SIGINT,
        libc::SIGHUP,
        libc::SIGTERM,
    ] {
        // SAFETY: each supported signal is handled synchronously by the
        // active scoped recorder in this isolated helper subprocess.
        assert_eq!(unsafe { libc::raise(signal) }, 0);
    }
    assert_eq!(
        SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.load(Ordering::SeqCst),
        0,
        "a signal reached its prior disposition before session retirement",
    );

    finish_doctor_signal_session(session).unwrap();
    assert_eq!(
        SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.load(Ordering::SeqCst),
        3,
    );
    assert_eq!(
        SQLX_PROBE_TEST_REDELIVERED_SIGNAL_ORDER.load(Ordering::SeqCst),
        1 | (2 << 2) | (3 << 4),
    );
}

#[cfg(unix)]
// Session ownership deliberately spans signal delivery through restoration.
#[allow(clippy::significant_drop_tightening)]
#[test]
fn sqlx_probe_signal_session_does_not_swallow_later_default_termination() {
    use std::os::unix::process::ExitStatusExt;

    const HELPER: &str = "JIG_SQLX_PROBE_LATER_DEFAULT_SIGNAL_HELPER";
    if std::env::var_os(HELPER).is_none() {
        let status = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "doctor::tests::sqlx_probe_signal_session_does_not_swallow_later_default_termination",
                    "--nocapture",
                ])
                .env(HELPER, "1")
                .status()
                .unwrap();
        assert_eq!(
            status.signal(),
            Some(libc::SIGTERM),
            "later-default-signal helper returned unexpected status {status}",
        );
        return;
    }

    // SAFETY: zero initializes the sigaction storage before its fields and
    // mask are populated below.
    let mut ignored = unsafe { std::mem::zeroed::<libc::sigaction>() };
    ignored.sa_sigaction = libc::SIG_IGN;
    ignored.sa_flags = 0;
    // SAFETY: the mask is writable storage owned by this helper process.
    assert_eq!(unsafe { libc::sigemptyset(&mut ignored.sa_mask) }, 0);
    // SAFETY: ignored is fully initialized and this isolated helper owns
    // its process-wide SIGINT disposition.
    assert_eq!(
        unsafe { libc::sigaction(libc::SIGINT, &ignored, std::ptr::null_mut()) },
        0,
    );
    install_default_doctor_signal_handler(libc::SIGTERM).unwrap();

    let session = DoctorSignalSession::start().unwrap();
    for signal in [libc::SIGINT, libc::SIGTERM] {
        // SAFETY: the active scoped session has installed a handler for
        // each supported signal in this isolated helper process.
        assert_eq!(unsafe { libc::raise(signal) }, 0);
    }
    finish_doctor_signal_session(session).unwrap();
    panic!("the later default SIGTERM disposition was swallowed");
}

#[cfg(unix)]
#[test]
fn sqlx_probe_signal_session_drop_restores_previous_handlers() {
    const HELPER: &str = "JIG_SQLX_PROBE_DROP_RESTORE_HELPER";
    if std::env::var_os(HELPER).is_none() {
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "doctor::tests::sqlx_probe_signal_session_drop_restores_previous_handlers",
                "--nocapture",
            ])
            .env(HELPER, "1")
            .status()
            .unwrap();
        assert!(status.success(), "drop-restore helper exited with {status}");
        return;
    }

    // SAFETY: zero initializes the sigaction storage before its fields and
    // mask are populated below.
    let mut ignored = unsafe { std::mem::zeroed::<libc::sigaction>() };
    ignored.sa_sigaction = libc::SIG_IGN;
    ignored.sa_flags = 0;
    // SAFETY: the mask is writable storage owned by this helper process.
    assert_eq!(unsafe { libc::sigemptyset(&mut ignored.sa_mask) }, 0);
    // SAFETY: ignored is fully initialized and this isolated helper owns its
    // process-wide SIGINT disposition.
    assert_eq!(
        unsafe { libc::sigaction(libc::SIGINT, &ignored, std::ptr::null_mut()) },
        0,
    );

    {
        let _session = DoctorSignalSession::start().unwrap();
    }

    // SAFETY: current points to writable storage and a null action requests
    // the process's current disposition without changing it.
    let mut current = unsafe { std::mem::zeroed::<libc::sigaction>() };
    assert_eq!(
        unsafe { libc::sigaction(libc::SIGINT, std::ptr::null(), &mut current) },
        0,
    );
    assert_eq!(current.sa_sigaction, libc::SIG_IGN);
}

#[cfg(unix)]
// These guards serialize generations and are consumed only by explicit finish.
#[allow(clippy::significant_drop_tightening)]
#[test]
fn sqlx_probe_signal_session_serializes_then_reuses_a_fresh_generation() {
    use std::sync::mpsc;

    const HELPER: &str = "JIG_SQLX_PROBE_REUSABLE_BARRIER_HELPER";
    if std::env::var_os(HELPER).is_none() {
        let status = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "doctor::tests::sqlx_probe_signal_session_serializes_then_reuses_a_fresh_generation",
                    "--nocapture",
                ])
                .env(HELPER, "1")
                .status()
                .unwrap();
        assert!(
            status.success(),
            "reusable barrier helper exited with {status}"
        );
        return;
    }

    SQLX_PROBE_TEST_PAUSE_HANDLER.store(false, Ordering::SeqCst);
    SQLX_PROBE_TEST_HANDLER_PAUSED.store(false, Ordering::SeqCst);
    SQLX_PROBE_TEST_RELEASE_HANDLER.store(false, Ordering::SeqCst);
    SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.store(0, Ordering::SeqCst);

    // SAFETY: zero initializes the sigaction storage before its fields and
    // mask are populated below.
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    action.sa_sigaction = record_sqlx_probe_test_redelivery as *const () as usize;
    action.sa_flags = 0;
    // SAFETY: the mask is writable storage owned by this isolated helper.
    assert_eq!(unsafe { libc::sigemptyset(&mut action.sa_mask) }, 0);
    // SAFETY: this subprocess owns its SIGTERM disposition for the test.
    assert_eq!(
        unsafe { libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut()) },
        0
    );

    let (ready_tx, ready_rx) = mpsc::channel();
    let (finish_tx, finish_rx) = mpsc::channel();
    let (finished_tx, finished_rx) = mpsc::channel();
    let owner = std::thread::spawn(move || {
        let session = DoctorSignalSession::start().unwrap();
        ready_tx.send(session.generation()).unwrap();
        finish_rx.recv().unwrap();
        finished_tx
            .send(finish_doctor_signal_session(session).is_ok())
            .unwrap();
    });
    let first_generation = ready_rx.recv().unwrap();

    SQLX_PROBE_TEST_PAUSE_HANDLER.store(true, Ordering::SeqCst);
    let handler = std::thread::spawn(|| record_doctor_signal(libc::SIGTERM));
    let pause_deadline = Instant::now() + Duration::from_secs(1);
    while !SQLX_PROBE_TEST_HANDLER_PAUSED.load(Ordering::SeqCst) {
        assert!(Instant::now() < pause_deadline, "handler did not pause");
        std::thread::yield_now();
    }
    finish_tx.send(()).unwrap();

    let (next_tx, next_rx) = mpsc::channel();
    let next = std::thread::spawn(move || {
        let session = DoctorSignalSession::start().unwrap();
        let generation = session.generation();
        let redelivered = SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.load(Ordering::SeqCst);
        let finished = finish_doctor_signal_session(session).is_ok();
        next_tx.send((generation, redelivered, finished)).unwrap();
    });
    assert!(
        next_rx.recv_timeout(Duration::from_millis(50)).is_err(),
        "a second signal-session attempt bypassed the active owner"
    );

    SQLX_PROBE_TEST_RELEASE_HANDLER.store(true, Ordering::SeqCst);
    handler.join().unwrap();
    assert!(finished_rx.recv().unwrap());
    owner.join().unwrap();

    let (next_generation, redelivered, next_finished) =
        next_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    next.join().unwrap();
    assert!(next_generation > first_generation);
    assert_eq!(redelivered, 1, "the next owner entered before redelivery");
    assert!(next_finished);

    SQLX_PROBE_TEST_PAUSE_HANDLER.store(false, Ordering::SeqCst);
    SQLX_PROBE_TEST_HANDLER_PAUSED.store(false, Ordering::SeqCst);
    SQLX_PROBE_TEST_RELEASE_HANDLER.store(false, Ordering::SeqCst);
}

#[cfg(unix)]
// These guards pin the generation until delayed callbacks are accounted for.
#[allow(clippy::significant_drop_tightening)]
#[test]
fn sqlx_probe_signal_session_assigns_a_delayed_entry_to_the_current_generation() {
    const HELPER: &str = "JIG_SQLX_PROBE_DELAYED_ENTRY_HELPER";
    if std::env::var_os(HELPER).is_none() {
        let status = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "doctor::tests::sqlx_probe_signal_session_assigns_a_delayed_entry_to_the_current_generation",
                    "--nocapture",
                ])
                .env(HELPER, "1")
                .status()
                .unwrap();
        assert!(
            status.success(),
            "delayed-entry helper exited with {status}"
        );
        return;
    }

    SQLX_PROBE_TEST_PAUSE_HANDLER_BEFORE_CLAIM.store(true, Ordering::SeqCst);
    SQLX_PROBE_TEST_HANDLER_PAUSED_BEFORE_CLAIM.store(false, Ordering::SeqCst);
    SQLX_PROBE_TEST_RELEASE_HANDLER_BEFORE_CLAIM.store(false, Ordering::SeqCst);
    SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.store(0, Ordering::SeqCst);

    // SAFETY: zero initializes the sigaction storage before its fields and
    // mask are populated below.
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    action.sa_sigaction = record_sqlx_probe_test_redelivery as *const () as usize;
    action.sa_flags = 0;
    // SAFETY: the mask is writable storage owned by this isolated helper.
    assert_eq!(unsafe { libc::sigemptyset(&mut action.sa_mask) }, 0);
    // SAFETY: this subprocess owns its SIGTERM disposition for the test.
    assert_eq!(
        unsafe { libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut()) },
        0
    );

    let first = DoctorSignalSession::start().unwrap();
    let first_generation = first.generation();
    let delayed = std::thread::spawn(|| record_doctor_signal(libc::SIGTERM));
    let pause_deadline = Instant::now() + Duration::from_secs(1);
    while !SQLX_PROBE_TEST_HANDLER_PAUSED_BEFORE_CLAIM.load(Ordering::SeqCst) {
        assert!(
            Instant::now() < pause_deadline,
            "handler did not pause before claiming a generation"
        );
        std::thread::yield_now();
    }

    finish_doctor_signal_session(first).unwrap();
    let second = DoctorSignalSession::start().unwrap();
    let second_generation = second.generation();
    assert!(second_generation > first_generation);

    SQLX_PROBE_TEST_RELEASE_HANDLER_BEFORE_CLAIM.store(true, Ordering::SeqCst);
    delayed.join().unwrap();
    assert!(
        second.cancelled(),
        "delayed callback did not join the active generation"
    );
    SQLX_PROBE_TEST_PAUSE_HANDLER_BEFORE_CLAIM.store(false, Ordering::SeqCst);
    finish_doctor_signal_session(second).unwrap();
    assert_eq!(
        SQLX_PROBE_TEST_REDELIVERED_SIGNAL_COUNT.load(Ordering::SeqCst),
        1
    );

    let third = DoctorSignalSession::start().unwrap();
    assert!(third.generation() > second_generation);
    finish_doctor_signal_session(third).unwrap();
}

#[cfg(unix)]
// The guard must outlive the paused handler until fail-closed retirement.
#[allow(clippy::significant_drop_tightening)]
#[test]
fn sqlx_probe_signal_session_timeout_fails_closed_for_a_recorded_signal() {
    use std::sync::mpsc;

    const HELPER: &str = "JIG_SQLX_PROBE_RECORDED_TIMEOUT_HELPER";
    if std::env::var_os(HELPER).is_none() {
        let status = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "doctor::tests::sqlx_probe_signal_session_timeout_fails_closed_for_a_recorded_signal",
                    "--nocapture",
                ])
                .env(HELPER, "1")
                .status()
                .unwrap();
        assert_eq!(
            status.code(),
            Some(128 + libc::SIGTERM),
            "recorded-timeout helper returned unexpected status {status}"
        );
        return;
    }

    SQLX_PROBE_TEST_PAUSE_HANDLER_AFTER_RECORD.store(true, Ordering::SeqCst);
    SQLX_PROBE_TEST_HANDLER_PAUSED_AFTER_RECORD.store(false, Ordering::SeqCst);
    SQLX_PROBE_TEST_RELEASE_HANDLER_AFTER_RECORD.store(false, Ordering::SeqCst);
    SQLX_PROBE_TEST_PAUSE_QUIESCENCE_TIMEOUT.store(true, Ordering::SeqCst);
    SQLX_PROBE_TEST_QUIESCENCE_TIMED_OUT.store(false, Ordering::SeqCst);
    SQLX_PROBE_TEST_RELEASE_QUIESCENCE_TIMEOUT.store(false, Ordering::SeqCst);

    let session = DoctorSignalSession::start().unwrap();
    let (handler_done_tx, handler_done_rx) = mpsc::channel();
    let handler = std::thread::spawn(move || {
        record_doctor_signal(libc::SIGTERM);
        handler_done_tx.send(()).unwrap();
    });
    let pause_deadline = Instant::now() + Duration::from_secs(1);
    while !SQLX_PROBE_TEST_HANDLER_PAUSED_AFTER_RECORD.load(Ordering::SeqCst) {
        assert!(
            Instant::now() < pause_deadline,
            "handler did not pause after recording"
        );
        std::thread::yield_now();
    }

    let coordinator = std::thread::spawn(move || {
        let timeout_deadline = Instant::now() + Duration::from_secs(2);
        while !SQLX_PROBE_TEST_QUIESCENCE_TIMED_OUT.load(Ordering::SeqCst) {
            assert!(
                Instant::now() < timeout_deadline,
                "signal retirement did not reach its quiescence timeout"
            );
            std::thread::yield_now();
        }
        SQLX_PROBE_TEST_RELEASE_HANDLER_AFTER_RECORD.store(true, Ordering::SeqCst);
        handler_done_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("recorded handler did not complete before poison publication");
        SQLX_PROBE_TEST_RELEASE_QUIESCENCE_TIMEOUT.store(true, Ordering::SeqCst);
    });

    let result = finish_doctor_signal_session(session);
    coordinator.join().unwrap();
    handler.join().unwrap();
    panic!("recorded signal was not claimed by fail-closed retirement: {result:?}");
}

#[cfg(unix)]
#[test]
fn inactive_sqlx_probe_handler_exits_instead_of_swallowing_signal() {
    const HELPER: &str = "JIG_SQLX_PROBE_INACTIVE_HANDLER_HELPER";
    if std::env::var_os(HELPER).is_some() {
        DOCTOR_ACTIVE_GENERATION.store(0, Ordering::SeqCst);
        DOCTOR_SIGNAL_GENERATION.store(0, Ordering::SeqCst);
        record_doctor_signal(libc::SIGTERM);
        panic!("an inactive SQLx probe handler swallowed SIGTERM");
    }

    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::inactive_sqlx_probe_handler_exits_instead_of_swallowing_signal",
            "--nocapture",
        ])
        .env(HELPER, "1")
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(128 + libc::SIGTERM));
}

#[cfg(unix)]
#[test]
fn poisoned_sqlx_probe_session_lock_blocks_future_sessions() {
    const HELPER: &str = "JIG_SQLX_PROBE_POISONED_LOCK_HELPER";
    if std::env::var_os(HELPER).is_some() {
        let poisoner = std::thread::spawn(|| {
            let _guard = DOCTOR_SIGNAL_SESSION.lock().unwrap();
            panic!("poison the signal-session mutex");
        });
        assert!(poisoner.join().is_err());
        let error = DoctorSignalSession::start()
            .err()
            .expect("poisoned mutex must reject a new signal session");
        assert!(error.to_string().contains("mutex is poisoned"));
        return;
    }

    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::poisoned_sqlx_probe_session_lock_blocks_future_sessions",
            "--nocapture",
        ])
        .env(HELPER, "1")
        .status()
        .unwrap();
    assert!(status.success(), "poison helper exited with {status}");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn sqlx_driver_probe_sigint_reaps_descendants_before_redelivery() {
    use std::os::unix::process::ExitStatusExt;

    let temp = tempdir().unwrap();
    let descendant_marker = temp.path().join("probe-descendant");
    let probe = temp.path().join("sqlx-sigint-tree");
    write_test_executable(
        &probe,
        &owned_test_descendant_script(&descendant_marker, "while :; do :; done"),
    );
    let mut helper = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::sqlx_driver_probe_sigint_helper",
            "--nocapture",
        ])
        .env("JIG_SQLX_PROBE_SIGINT_HELPER", &probe)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let descendant = read_test_process_identity(&descendant_marker);
    // SAFETY: the test owns this live helper PID and sends a standard
    // termination signal solely to that subprocess.
    assert_eq!(
        unsafe { libc::kill(helper.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    let status = helper
        .wait_timeout(Duration::from_secs(3))
        .unwrap()
        .expect("SIGINT helper did not terminate after probe cleanup");
    assert_eq!(status.signal(), Some(libc::SIGINT));
    assert_test_process_stopped(&descendant);
}

#[test]
fn proxy_list_command_preserves_the_portable_launcher_plan() {
    let temp = tempdir().unwrap();
    let (launcher, command) = proxy_list_command(temp.path()).unwrap();
    let args = command
        .get_args()
        .map(OsStr::to_os_string)
        .collect::<Vec<_>>();

    assert!(launcher.is_absolute());
    assert_eq!(command.get_current_dir(), Some(temp.path()));
    for key in crate::shell::BASH_CONTROL_ENVIRONMENT_KEYS {
        assert!(
            command
                .get_envs()
                .any(|(candidate, value)| candidate == OsStr::new(key) && value.is_none()),
            "{key} was not removed from the launcher-backed proxy diagnostic"
        );
    }
    #[cfg(windows)]
    {
        assert_eq!(command.get_program(), OsStr::new("bash"));
        assert_eq!(
            args,
            vec![
                launcher.into_os_string(),
                OsString::from("proxy"),
                OsString::from("list"),
                OsString::from("--json"),
            ]
        );
    }
    #[cfg(not(windows))]
    {
        assert_eq!(command.get_program(), launcher.as_os_str());
        assert_eq!(
            args,
            vec![
                OsString::from("proxy"),
                OsString::from("list"),
                OsString::from("--json"),
            ]
        );
    }
}

#[cfg(windows)]
#[test]
fn proxy_list_command_converts_verbatim_roots_for_bash_and_its_working_directory() {
    let (launcher, command) = proxy_list_command(Path::new(r"\\?\C:\repo")).unwrap();
    let args = command
        .get_args()
        .map(OsStr::to_os_string)
        .collect::<Vec<_>>();

    assert_eq!(launcher, PathBuf::from(r"C:\repo\scripts\jig"));
    assert_eq!(command.get_program(), OsStr::new("bash"));
    assert_eq!(command.get_current_dir(), Some(Path::new(r"C:\repo")));
    assert_eq!(args[0], launcher.as_os_str());

    let (unc_launcher, unc_command) =
        proxy_list_command(Path::new(r"\\?\UNC\server\share\repo")).unwrap();
    assert_eq!(
        unc_launcher,
        PathBuf::from(r"\\server\share\repo\scripts\jig")
    );
    assert_eq!(
        unc_command.get_current_dir(),
        Some(Path::new(r"\\server\share\repo"))
    );
    assert_eq!(
        unc_command.get_args().next(),
        Some(unc_launcher.as_os_str())
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn proxy_list_output_executes_the_launcher_through_a_clean_bash_environment() {
    let temp = tempdir().unwrap();
    write_doctor_fixture(temp.path());
    let poison_marker = temp.path().join("proxy-poison-ran");
    let trace_marker = temp.path().join("proxy-trace-poison-ran");
    fs::write(
        temp.path().join("scripts/proxy-startup-poison.sh"),
        "printf poison > \"$JIG_DOCTOR_PROXY_POISON_MARKER\"\nexit 91\n",
    )
    .unwrap();
    fs::write(
            temp.path().join("scripts/jig"),
            r#"#!/bin/bash
if [ "$#" -ne 3 ] || [ "$1" != proxy ] || [ "$2" != list ] || [ "$3" != --json ]; then
  exit 19
fi
if [ ! -f .jig.toml ]; then
  exit 20
fi
if [ -n "${BASH_ENV+x}" ] || [ -n "${ENV+x}" ] || [ -n "${CDPATH+x}" ] || [ -n "${BASH_XTRACEFD+x}" ]; then
  exit 21
fi
if declare -F jig_doctor_proxy_poison >/dev/null; then
  exit 22
fi
case "$-" in *x*|*v*) exit 23 ;; esac
shopt -q extglob && exit 24
case "$PS4" in *JIG_DOCTOR_PROXY_PS4_POISON*) exit 25 ;; esac
[ "$JIG_DOCTOR_PROXY_ORDINARY" = preserved ] || exit 26
printf '%s\n' '{"ok":true,"running":false,"routes":[]}'
"#,
        )
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            temp.path().join("scripts/jig"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let (_, mut command) = proxy_list_command(temp.path()).unwrap();
    command
        .env(
            "BASH_ENV",
            temp.path().join("scripts/proxy-startup-poison.sh"),
        )
        .env("ENV", temp.path().join("scripts/proxy-startup-poison.sh"))
        .env("CDPATH", ".")
        .env(
            "BASH_FUNC_jig_doctor_proxy_poison%%",
            "() { printf poison > \"$JIG_DOCTOR_PROXY_POISON_MARKER\"; }",
        )
        .env("SHELLOPTS", "xtrace:verbose")
        .env("BASHOPTS", "extglob")
        .env(
            "PS4",
            "JIG_DOCTOR_PROXY_PS4_POISON$(printf poison > \"$JIG_DOCTOR_PROXY_TRACE_MARKER\")",
        )
        .env("BASH_XTRACEFD", "2")
        .env("JIG_DOCTOR_PROXY_POISON_MARKER", &poison_marker)
        .env("JIG_DOCTOR_PROXY_TRACE_MARKER", &trace_marker)
        .env("JIG_DOCTOR_PROXY_ORDINARY", "preserved");
    crate::shell::sanitize_bash_environment(&mut command);

    let output = proxy_list_output_with_timeout(&mut command, Duration::from_secs(2)).unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["running"], false);
    assert_eq!(output["routes"], json!([]));
    assert!(
        !poison_marker.exists(),
        "Bash startup control environment executed during proxy diagnostics"
    );
    assert!(
        !trace_marker.exists(),
        "Bash trace environment executed during proxy diagnostics"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn proxy_list_output_accepts_valid_json_larger_than_the_diagnostic_default() {
    let temp = tempdir().unwrap();
    let launcher = temp.path().join("proxy-list-valid");
    write_test_executable(
        &launcher,
        "#!/bin/sh\nprintf '%s' '{\"ok\":true,\"running\":false,\"routes\":[],\"padding\":\"'\ni=0\nwhile [ \"$i\" -lt 1100 ]; do printf 0123456789abcdef; i=$((i + 1)); done\nprintf '%s\\n' '\"}'\n",
    );
    let mut command = Command::new(launcher);

    let output = proxy_list_output_with_timeout(&mut command, Duration::from_secs(2)).unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["running"], false);
    assert_eq!(output["routes"], json!([]));
    assert!(
        output["padding"].as_str().unwrap().len() > ProcessOutputLimits::default().stdout,
        "proxy functional JSON must not share the 16 KiB diagnostic cap"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn proxy_list_output_reports_injected_stdout_truncation() {
    let temp = tempdir().unwrap();
    let launcher = temp.path().join("proxy-list-truncated");
    write_test_executable(
        &launcher,
        "#!/bin/sh\nprintf '%s' '{\"ok\":true,\"running\":false,\"routes\":[],\"padding\":\"'\ni=0\nwhile [ \"$i\" -lt 100 ]; do printf 0123456789abcdef; i=$((i + 1)); done\nprintf '%s\\n' '\"}'\n",
    );
    let mut command = Command::new(launcher);

    let error = proxy_list_output_with_timeout_and_limits_and_cancellation(
        &mut command,
        Duration::from_secs(2),
        ProcessOutputLimits {
            stdout: 128,
            stderr: ProcessOutputLimits::default().stderr,
        },
        || false,
    )
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("exceeded the diagnostic capture limit"),
        "{error}"
    );
}

#[cfg(feature = "dev-proxy")]
#[test]
fn proxy_list_capture_limit_exceeds_the_unchanged_routes_file_limit() {
    assert_eq!(jig_dev_proxy::MAX_ROUTES_FILE_BYTES, 4 * 1024 * 1024);
    let limits = proxy_list_output_limits();
    assert!(limits.stdout > jig_dev_proxy::MAX_ROUTES_FILE_BYTES as usize);
    assert_eq!(limits.stderr, ProcessOutputLimits::default().stderr);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn proxy_list_output_timeout_reaps_its_exact_descendant() {
    let temp = tempdir().unwrap();
    let marker = temp.path().join("proxy-list-descendant");
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "doctor::tests::proxy_list_output_helper",
            "--nocapture",
        ])
        .env("JIG_DOCTOR_PROXY_LIST_HELPER", "hanging")
        .env("JIG_DOCTOR_PROXY_LIST_DESCENDANT_MARKER", &marker);

    let error = proxy_list_output_with_timeout(&mut command, Duration::from_millis(100))
        .unwrap_err()
        .to_string();
    let descendant = read_test_process_identity(&marker);

    assert!(error.contains("timed out"), "{error}");
    assert_test_process_stopped(&descendant);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn proxy_check_sigint_helper() {
    let Some(root) = std::env::var_os("JIG_DOCTOR_PROXY_SIGINT_ROOT") else {
        return;
    };
    let ctx = RepoContext::load_from_root(PathBuf::from(root)).unwrap();
    let result = doctor_context_checks(&ctx);
    panic!("SIGINT was not re-delivered after proxy cleanup: {result:?}");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn proxy_check_sigint_reaps_its_exact_descendant_before_redelivery() {
    use std::os::unix::process::ExitStatusExt;

    let temp = tempdir().unwrap();
    write_doctor_fixture(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            "{}\n[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\ncoverage_threshold = 80\n",
            fs::read_to_string(temp.path().join(".jig.toml")).unwrap()
        ),
    )
    .unwrap();
    fs::create_dir(temp.path().join("web")).unwrap();
    write_test_executable(
        &temp.path().join("scripts/jig"),
        &format!(
            "#!/bin/sh\nexec {} --exact doctor::tests::proxy_list_output_helper --nocapture\n",
            shell_quote_test_path(&std::env::current_exe().unwrap())
        ),
    );
    let descendant_marker = temp.path().join("proxy-sigint-descendant");
    let mut helper = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::proxy_check_sigint_helper",
            "--nocapture",
        ])
        .env("JIG_DOCTOR_PROXY_SIGINT_ROOT", temp.path())
        .env("JIG_DOCTOR_PROXY_LIST_HELPER", "hanging")
        .env(
            "JIG_DOCTOR_PROXY_LIST_DESCENDANT_MARKER",
            &descendant_marker,
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let descendant = read_test_process_identity(&descendant_marker);
    // SAFETY: this test owns the live isolated helper subprocess.
    assert_eq!(
        unsafe { libc::kill(helper.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    let status = helper
        .wait_timeout(Duration::from_secs(3))
        .unwrap()
        .expect("SIGINT helper did not terminate after proxy cleanup");
    assert_eq!(status.signal(), Some(libc::SIGINT));
    assert_test_process_stopped(&descendant);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn production_sqlx_probe_sigint_helper() {
    let Some(marker) = std::env::var_os("JIG_DOCTOR_SQLX_PRODUCTION_MARKER") else {
        return;
    };
    let identity = TestProcessIdentity::capture_current().unwrap();
    publish_test_process_identity(Path::new(&marker), &identity);
    std::thread::sleep(Duration::from_secs(30));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn doctor_sqlx_sigint_sequence_helper() {
    let Some(root) = std::env::var_os("JIG_DOCTOR_SQLX_SEQUENCE_ROOT") else {
        return;
    };
    let ctx = RepoContext::load_from_root(PathBuf::from(root)).unwrap();
    let result = doctor_context_checks(&ctx);
    panic!("SIGINT was not re-delivered after SQLx cleanup: {result:?}");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn cancellation_during_production_sqlx_prevents_codex_and_proxy_spawns() {
    use std::os::unix::process::ExitStatusExt;

    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        "sqlx prepare -D sqlite:production-signal.db",
    );
    let config_path = temp.path().join(".jig.toml");
    fs::write(
            &config_path,
            format!(
                "{}\n[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\ncoverage_threshold = 80\n",
                fs::read_to_string(&config_path).unwrap().replace(
                    "[agent_tooling.codex]\nmarketplaces = []",
                    "[[agent_tooling.codex.marketplaces]]\nid = \"test-skills\"\nsource = \"example/test-skills\"",
                )
            ),
        )
        .unwrap();
    fs::create_dir(temp.path().join("web")).unwrap();

    let probe_marker = temp.path().join("sqlx-production-probe");
    let tools = tempdir().unwrap();
    write_test_executable(
        &tools.path().join("sqlx"),
        &format!(
            "#!/bin/sh\nJIG_DOCTOR_SQLX_PRODUCTION_MARKER={} exec {} --exact doctor::tests::production_sqlx_probe_sigint_helper --nocapture\n",
            shell_quote_test_path(&probe_marker),
            shell_quote_test_path(&std::env::current_exe().unwrap())
        ),
    );
    let codex_marker = temp.path().join("codex-started");
    let codex = temp.path().join("codex");
    write_test_executable(
        &codex,
        &format!(
            "#!/bin/sh\nprintf c > '{}'\nexit 0\n",
            codex_marker.display()
        ),
    );
    let proxy_marker = temp.path().join("proxy-started");
    write_test_executable(
        &temp.path().join("scripts/jig"),
        &format!(
            "#!/bin/sh\nprintf p > '{}'\nprintf '%s\n' '{{\"ok\":true,\"running\":false,\"routes\":[]}}'\n",
            proxy_marker.display()
        ),
    );

    let mut helper = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::doctor_sqlx_sigint_sequence_helper",
            "--nocapture",
        ])
        .env("JIG_DOCTOR_SQLX_SEQUENCE_ROOT", temp.path())
        .env("JIG_CODEX_BIN", &codex)
        .env("CODEX_HOME", temp.path().join("codex-home"))
        .env("PATH", fs::canonicalize(tools.path()).unwrap())
        .env_remove("BASH_ENV")
        .env_remove("ENV")
        .env_remove("CDPATH")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let probe = read_test_process_identity(&probe_marker);
    // SAFETY: this test owns the isolated doctor helper subprocess.
    assert_eq!(
        unsafe { libc::kill(helper.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    let status = helper
        .wait_timeout(Duration::from_secs(3))
        .unwrap()
        .expect("doctor helper did not terminate after SQLx cleanup");
    assert_eq!(status.signal(), Some(libc::SIGINT));
    assert_test_process_stopped(&probe);
    assert!(
        !codex_marker.exists(),
        "Codex started after SQLx cancellation"
    );
    assert!(
        !proxy_marker.exists(),
        "proxy started after SQLx cancellation"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn production_codex_probe_helper() {
    let Some(marker) = std::env::var_os("JIG_DOCTOR_CODEX_DESCENDANT_MARKER") else {
        return;
    };
    for _ in 0..2_000 {
        println!("codex-probe-secret-that-must-not-leak");
        eprintln!("codex-probe-secret-that-must-not-leak");
    }
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
    let _ = read_test_process_identity(Path::new(&marker));
    std::thread::sleep(Duration::from_secs(30));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn doctor_codex_sigint_sequence_helper() {
    let Some(root) = std::env::var_os("JIG_DOCTOR_CODEX_SEQUENCE_ROOT") else {
        return;
    };
    let ctx = RepoContext::load_from_root(PathBuf::from(root)).unwrap();
    let result = doctor_context_checks(&ctx);
    panic!("SIGINT was not re-delivered after Codex cleanup: {result:?}");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn cancellation_during_noisy_codex_reaps_descendant_and_prevents_proxy_spawn() {
    use std::os::unix::process::ExitStatusExt;

    let temp = tempdir().unwrap();
    write_doctor_fixture(temp.path());
    let config_path = temp.path().join(".jig.toml");
    fs::write(
            &config_path,
            format!(
                "{}\n[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\ncoverage_threshold = 80\n",
                fs::read_to_string(&config_path).unwrap().replace(
                    "[agent_tooling.codex]\nmarketplaces = []",
                    "[[agent_tooling.codex.marketplaces]]\nid = \"test-skills\"\nsource = \"example/test-skills\"",
                )
            ),
        )
        .unwrap();
    fs::create_dir(temp.path().join("web")).unwrap();
    let codex = temp.path().join("codex");
    write_test_executable(
        &codex,
        &format!(
            "#!/bin/sh\nexec {} --exact doctor::tests::production_codex_probe_helper --nocapture\n",
            shell_quote_test_path(&std::env::current_exe().unwrap())
        ),
    );
    let proxy_marker = temp.path().join("proxy-started");
    write_test_executable(
        &temp.path().join("scripts/jig"),
        &format!(
            "#!/bin/sh\nprintf p > '{}'\nprintf '%s\n' '{{\"ok\":true,\"running\":false,\"routes\":[]}}'\n",
            proxy_marker.display()
        ),
    );
    let descendant_marker = temp.path().join("codex-descendant");
    let mut helper = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::doctor_codex_sigint_sequence_helper",
            "--nocapture",
        ])
        .env("JIG_DOCTOR_CODEX_SEQUENCE_ROOT", temp.path())
        .env("JIG_DOCTOR_CODEX_DESCENDANT_MARKER", &descendant_marker)
        .env("JIG_CODEX_BIN", &codex)
        .env("CODEX_HOME", temp.path().join("codex-home"))
        .env_remove("BASH_ENV")
        .env_remove("ENV")
        .env_remove("CDPATH")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let descendant = read_test_process_identity(&descendant_marker);
    // SAFETY: this test owns the isolated doctor helper subprocess.
    assert_eq!(
        unsafe { libc::kill(helper.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    let status = helper
        .wait_timeout(Duration::from_secs(3))
        .unwrap()
        .expect("doctor helper did not terminate after Codex cleanup");
    assert_eq!(status.signal(), Some(libc::SIGINT));
    assert_test_process_stopped(&descendant);
    assert!(
        !proxy_marker.exists(),
        "proxy started after Codex cancellation"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn standalone_codex_sigint_sequence_helper() {
    let Some(codex) = std::env::var_os("JIG_STANDALONE_CODEX_SIGINT_BIN") else {
        return;
    };
    let result = standalone_codex_support_probe_with_signal_session(
        codex.as_os_str(),
        Duration::from_secs(30),
    );
    panic!("SIGINT was not re-delivered after standalone Codex cleanup: {result:?}");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn standalone_codex_sigint_reaps_its_exact_descendant_before_redelivery() {
    use std::os::unix::process::ExitStatusExt;

    let temp = tempdir().unwrap();
    let codex = temp.path().join("codex");
    write_test_executable(
        &codex,
        &format!(
            "#!/bin/sh\nexec {} --exact doctor::tests::production_codex_probe_helper --nocapture\n",
            shell_quote_test_path(&std::env::current_exe().unwrap())
        ),
    );
    let descendant_marker = temp.path().join("standalone-codex-descendant");
    let mut helper = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::standalone_codex_sigint_sequence_helper",
            "--nocapture",
        ])
        .env("JIG_STANDALONE_CODEX_SIGINT_BIN", &codex)
        .env("JIG_DOCTOR_CODEX_DESCENDANT_MARKER", &descendant_marker)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let descendant = read_test_process_identity(&descendant_marker);
    // SAFETY: this test owns the isolated standalone doctor helper.
    assert_eq!(
        unsafe { libc::kill(helper.id() as libc::pid_t, libc::SIGINT) },
        0
    );
    let status = helper
        .wait_timeout(Duration::from_secs(3))
        .unwrap()
        .expect("standalone doctor helper did not terminate after Codex cleanup");
    assert_eq!(status.signal(), Some(libc::SIGINT));
    assert_test_process_stopped(&descendant);
}

#[cfg(unix)]
#[test]
fn required_tools_distinguishes_missing_and_incompatible_sqlx_cli() {
    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(temp.path(), "cargo-sqlx sqlx prepare");
    fs::write(
        temp.path().join(".env"),
        "DATABASE_URL=sqlite:private-database-name.db\n",
    )
    .unwrap();

    let tools = tempdir().unwrap();
    let bin = tools.path().to_path_buf();
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let missing = required_tools_check_with_environment(&ctx, &doctor_environment(&bin, None));
    assert!(!missing.ok);
    assert_eq!(missing.status, "missing");
    assert!(missing.detail.contains("cargo-sqlx"));

    write_test_executable(
        &bin.join("cargo-sqlx"),
        "#!/bin/sh\nprintf '%s\\n' 'error: error with configuration: no driver found for URL scheme \"sqlite\"'\nexit 1\n",
    );
    let incompatible = required_tools_check_with_environment(&ctx, &doctor_environment(&bin, None));
    assert!(!incompatible.ok);
    assert_eq!(incompatible.status, "incompatible");
    assert!(incompatible.detail.contains("lacks the SQLite driver"));
    assert!(
        incompatible
            .fix
            .as_deref()
            .unwrap()
            .contains("--features sqlite")
    );
    assert_eq!(
        cargo_sqlx_program(&incompatible)["driver_probe"]["status"],
        "missing_driver"
    );
    assert_eq!(
        cargo_sqlx_program(&incompatible)["driver_probe"]["compatible"],
        false
    );

    let serialized = serde_json::to_string(&incompatible).unwrap();
    assert!(!serialized.contains("private-database-name"));
    assert!(!serialized.contains("sqlite:private"));
    let summary = format_summary(&output(None, vec![incompatible]));
    assert!(summary.contains("Required tools: needs setup (incompatible, required)"));
    assert!(summary.contains("--features sqlite"));
    assert!(!summary.contains("private-database-name"));
}

#[cfg(unix)]
#[test]
fn required_tools_require_external_wrappers_and_their_targets() {
    let run = |command: &str, executables: &[&str]| {
        let repo = tempdir().unwrap();
        write_doctor_fixture_with_bootstrap_command(repo.path(), command);
        let tools = tempdir().unwrap();
        for executable in executables {
            write_test_executable(&tools.path().join(executable), "#!/bin/sh\nexit 0\n");
        }
        let ctx = RepoContext::load_from_root(repo.path().to_path_buf()).unwrap();
        required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None))
    };
    let bootstrap_programs = |check: &DoctorCheck| {
        check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["command_key"] == "bootstrap_command")
            .unwrap()["programs"]
            .as_array()
            .unwrap()
            .clone()
    };

    for wrapper in ["env", "nohup"] {
        let command = format!("{wrapper} cargo test");
        let missing_wrapper = run(&command, &["cargo"]);
        assert_eq!(missing_wrapper.status, "missing", "{wrapper}");
        assert!(!missing_wrapper.ok, "{wrapper}");
        let programs = bootstrap_programs(&missing_wrapper);
        assert_eq!(programs[0]["program"], wrapper, "{wrapper}");
        assert_eq!(programs[0]["present"], false, "{wrapper}");
        assert_eq!(programs[1]["program"], "cargo", "{wrapper}");
        assert_eq!(programs[1]["present"], true, "{wrapper}");

        let missing_target = run(&command, &[wrapper]);
        assert_eq!(missing_target.status, "missing", "{wrapper}");
        let programs = bootstrap_programs(&missing_target);
        assert_eq!(programs[0]["present"], true, "{wrapper}");
        assert_eq!(programs[1]["present"], false, "{wrapper}");

        let all_present = run(&command, &[wrapper, "cargo"]);
        assert_eq!(all_present.status, "present", "{wrapper}");
        assert!(all_present.ok, "{wrapper}");
    }

    for command in ["env --help", "env -0"] {
        let missing_wrapper = run(command, &[]);
        assert_eq!(missing_wrapper.status, "missing", "{command:?}");
        let programs = bootstrap_programs(&missing_wrapper);
        assert_eq!(programs.len(), 1, "{command:?}");
        assert_eq!(programs[0]["program"], "env", "{command:?}");
        assert_eq!(programs[0]["present"], false, "{command:?}");
    }

    let dynamic_target = run("env \"$TOOL\" test", &[]);
    assert_eq!(dynamic_target.status, "missing");
    let programs = bootstrap_programs(&dynamic_target);
    assert_eq!(programs[0]["program"], "env");
    assert_eq!(programs[0]["present"], false);
    assert_eq!(programs[1]["program"], Value::Null);
    assert_eq!(programs[1]["present"], Value::Null);
    assert!(
        !serde_json::to_string(&dynamic_target)
            .unwrap()
            .contains("TOOL")
    );
}

#[cfg(unix)]
#[test]
fn required_tools_check_nested_external_time_chain_in_order() {
    let repo = tempdir().unwrap();
    let tools = tempdir().unwrap();
    for executable in ["env", "nohup", "time", "cargo"] {
        write_test_executable(&tools.path().join(executable), "#!/bin/sh\nexit 0\n");
    }
    let time = tools.path().join("time");
    write_doctor_fixture_with_bootstrap_command(
        repo.path(),
        &format!("env nohup {} cargo test", time.display()),
    );
    let ctx = RepoContext::load_from_root(repo.path().to_path_buf()).unwrap();
    let check =
        required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present");
    let programs = check.data["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["command_key"] == "bootstrap_command")
        .unwrap()["programs"]
        .as_array()
        .unwrap();
    assert_eq!(programs.len(), 4);
    assert_eq!(programs[0]["program"], "env");
    assert_eq!(programs[1]["program"], "nohup");
    assert_eq!(programs[2]["program"], time.display().to_string());
    assert_eq!(programs[3]["program"], "cargo");
    assert!(programs.iter().all(|program| program["present"] == true));
}

#[cfg(unix)]
#[test]
fn required_tools_marks_ambiguous_wrappers_unverified_without_leaking() {
    for (command, secret) in [
        (
            "env -S 'doctor-split-secret missing-tool --flag'",
            "doctor-split-secret",
        ),
        (
            "env '--split-string=doctor-long-split-secret missing-tool --flag' cargo",
            "doctor-long-split-secret",
        ),
        (
            "exec -z doctor-wrapper-secret cargo test",
            "doctor-wrapper-secret",
        ),
    ] {
        let temp = tempdir().unwrap();
        write_doctor_fixture_with_bootstrap_command(temp.path(), command);
        let tools = tempdir().unwrap();
        write_test_executable(&tools.path().join("env"), "#!/bin/sh\nexit 0\n");
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let check = required_tools_check_with_environment(
            &ctx,
            &DoctorEnvironment {
                search_path: Some(tools.path().as_os_str().to_os_string()),
                ..DoctorEnvironment::default()
            },
        );

        assert!(check.ok, "{command:?}: {}", check.detail);
        assert_eq!(check.status, "present_unverified", "{command:?}");
        assert!(check.fix.is_none(), "{command:?}");
        let tool = check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["command_key"] == "bootstrap_command")
            .unwrap();
        assert!(tool["present"].is_null(), "{command:?}");
        assert!(
            tool["programs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|program| program["present"].is_null()),
            "{command:?}",
        );
        let serialized = serde_json::to_string(&check).unwrap();
        assert!(!serialized.contains(secret), "{command:?}");
        assert!(!serialized.contains("No external executable required"));
    }
}

#[test]
fn required_tools_downgrades_dynamic_and_complex_shell_commands() {
    for command in [
        "$DOCTOR_DYNAMIC_TOOL test",
        "eval 'doctor-eval-missing-tool --version'",
        "doctor_fn() { :; }; doctor_fn",
        "cargo \"$(missing-helper)\" test",
        "cargo test >\"$(missing-helper)\"",
        "cat <<EOF\n$(missing-helper)\nEOF",
    ] {
        let temp = tempdir().unwrap();
        write_doctor_fixture_with_bootstrap_command(temp.path(), command);
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
        let check = required_tools_check_with_environment(
            &ctx,
            &DoctorEnvironment {
                search_path: Some(OsString::new()),
                ..DoctorEnvironment::default()
            },
        );

        assert!(check.ok, "{command:?}: {}", check.detail);
        assert_eq!(check.status, "present_unverified", "{command:?}");
        assert!(
            check.detail.contains("must be run to verify"),
            "{command:?}"
        );
        assert!(!check.detail.contains("Missing command"), "{command:?}");
        let serialized = serde_json::to_string(&check).unwrap();
        assert!(!serialized.contains("missing-helper"), "{command:?}");
        assert!(!serialized.contains("DOCTOR_DYNAMIC_TOOL"), "{command:?}");
        assert!(
            !serialized.contains("doctor-eval-missing-tool"),
            "{command:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn required_tools_preserve_known_presence_but_downgrade_inherited_shell_state() {
    for issue in [
        ShellEnvironmentIssue::BashEnv,
        ShellEnvironmentIssue::PosixEnv,
        ShellEnvironmentIssue::CdPath,
        ShellEnvironmentIssue::ImportedFunction,
    ] {
        let repo = tempdir().unwrap();
        write_doctor_fixture_with_bootstrap_command(repo.path(), "env cargo test");
        let tools = tempdir().unwrap();
        for executable in ["env", "cargo"] {
            write_test_executable(&tools.path().join(executable), "#!/bin/sh\nexit 0\n");
        }
        let ctx = RepoContext::load_from_root(repo.path().to_path_buf()).unwrap();
        let mut environment = doctor_environment(tools.path(), None);
        environment.shell_environment_issue = Some(issue);

        let check = required_tools_check_with_environment(&ctx, &environment);

        assert!(check.ok, "{issue:?}: {}", check.detail);
        assert_eq!(check.status, "present_unverified", "{issue:?}");
        let tool = check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["command_key"] == "bootstrap_command")
            .unwrap();
        assert!(tool["present"].is_null(), "{issue:?}");
        let programs = tool["programs"].as_array().unwrap();
        assert_eq!(programs[0]["program"], "env", "{issue:?}");
        assert_eq!(programs[0]["present"], true, "{issue:?}");
        assert_eq!(programs[1]["program"], "cargo", "{issue:?}");
        assert_eq!(programs[1]["present"], true, "{issue:?}");
        assert!(programs.last().unwrap()["present"].is_null(), "{issue:?}");
        assert!(
            !serde_json::to_string(&check)
                .unwrap()
                .contains("No external executable required")
        );
    }
}

#[cfg(unix)]
#[test]
fn required_tools_downgrade_prior_dispatch_mutations() {
    for (command, target) in [
        ("hash -p /tmp/shim cargo; cargo test", "cargo"),
        ("enable -f /tmp/plugin custom; custom", "custom"),
        ("trap 'missing-helper' DEBUG; cargo test", "cargo"),
    ] {
        let repo = tempdir().unwrap();
        write_doctor_fixture_with_bootstrap_command(repo.path(), command);
        let tools = tempdir().unwrap();
        write_test_executable(&tools.path().join(target), "#!/bin/sh\nexit 0\n");
        let ctx = RepoContext::load_from_root(repo.path().to_path_buf()).unwrap();

        let check =
            required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));

        assert!(check.ok, "{command:?}: {}", check.detail);
        assert_eq!(check.status, "present_unverified", "{command:?}");
        let tool = check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["command_key"] == "bootstrap_command")
            .unwrap();
        assert!(tool["present"].is_null(), "{command:?}");
        let target = tool["programs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|program| program["program"] == target)
            .unwrap();
        assert!(target["present"].is_null(), "{command:?}");
        let serialized = serde_json::to_string(&check).unwrap();
        assert!(!serialized.contains("/tmp/shim"), "{command:?}");
        assert!(!serialized.contains("/tmp/plugin"), "{command:?}");
        assert!(!serialized.contains("missing-helper"), "{command:?}");
    }
}

#[cfg(unix)]
#[test]
fn required_tools_resolve_literal_relative_and_empty_path_from_repo_root() {
    let _env = lock_env();
    for (command, relative_executable) in [
        ("PATH=bin cargo test", "bin/cargo"),
        ("PATH= cargo test", "cargo"),
    ] {
        let repo = tempdir().unwrap();
        write_doctor_fixture_with_bootstrap_command(repo.path(), command);
        let invocation = repo.path().join("invocation/subdir");
        fs::create_dir_all(&invocation).unwrap();
        let executable = repo.path().join(relative_executable);
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        write_test_executable(&executable, "#!/bin/sh\nexit 0\n");
        let _cwd = CurrentDirGuard::set(&invocation);
        let ctx = RepoContext::load_from_root(repo.path().to_path_buf()).unwrap();

        let check = required_tools_check_with_environment(
            &ctx,
            &DoctorEnvironment {
                search_path: Some(invocation.as_os_str().to_os_string()),
                ..DoctorEnvironment::default()
            },
        );

        assert!(check.ok, "{command:?}: {}", check.detail);
        assert_eq!(check.status, "present", "{command:?}");
        let tool = check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["command_key"] == "bootstrap_command")
            .unwrap();
        assert_eq!(tool["present"], true, "{command:?}");
        assert_eq!(tool["programs"][0]["present"], true, "{command:?}");
    }
}

#[cfg(unix)]
#[test]
fn required_tools_accept_external_env_non_bash_assignment_names() {
    let repo = tempdir().unwrap();
    write_doctor_fixture_with_bootstrap_command(repo.path(), "env FOO.BAR=x cargo test");
    let tools = tempdir().unwrap();
    for executable in ["env", "cargo"] {
        write_test_executable(&tools.path().join(executable), "#!/bin/sh\nexit 0\n");
    }
    let ctx = RepoContext::load_from_root(repo.path().to_path_buf()).unwrap();

    let check =
        required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present");
    let programs = check.data["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["command_key"] == "bootstrap_command")
        .unwrap()["programs"]
        .as_array()
        .unwrap();
    assert_eq!(programs.len(), 2);
    assert_eq!(programs[0]["program"], "env");
    assert_eq!(programs[1]["program"], "cargo");
    assert!(programs.iter().all(|program| program["present"] == true));

    fs::remove_file(tools.path().join("cargo")).unwrap();
    let missing =
        required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));
    assert!(!missing.ok);
    assert_eq!(missing.status, "missing");
    let programs = missing.data["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["command_key"] == "bootstrap_command")
        .unwrap()["programs"]
        .as_array()
        .unwrap();
    assert_eq!(programs[0]["program"], "env");
    assert_eq!(programs[0]["present"], true);
    assert_eq!(programs[1]["program"], "cargo");
    assert_eq!(programs[1]["present"], false);
}

#[cfg(unix)]
#[test]
fn required_tools_avoid_cwd_false_present_and_false_missing_results() {
    for tool_location in ["root", "sub"] {
        let temp = tempdir().unwrap();
        let sub = temp.path().join("sub");
        fs::create_dir(&sub).unwrap();
        write_doctor_fixture_with_bootstrap_command(temp.path(), "env -C sub ./doctor-cwd-tool");
        let tool = if tool_location == "root" {
            temp.path().join("doctor-cwd-tool")
        } else {
            sub.join("doctor-cwd-tool")
        };
        write_test_executable(&tool, "#!/bin/sh\nexit 0\n");
        let tools = tempdir().unwrap();
        write_test_executable(&tools.path().join("env"), "#!/bin/sh\nexit 0\n");
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let check = required_tools_check_with_environment(
            &ctx,
            &DoctorEnvironment {
                search_path: Some(tools.path().as_os_str().to_os_string()),
                ..DoctorEnvironment::default()
            },
        );

        assert!(check.ok, "{tool_location}: {}", check.detail);
        assert_eq!(check.status, "present_unverified", "{tool_location}");
        let bootstrap = check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["command_key"] == "bootstrap_command")
            .unwrap();
        assert!(bootstrap["present"].is_null(), "{tool_location}");
        assert_eq!(bootstrap["programs"][0]["program"], "env");
        assert_eq!(bootstrap["programs"][0]["present"], true);
        assert!(bootstrap["programs"][1]["present"].is_null());
    }
}

#[cfg(unix)]
#[test]
fn required_tools_does_not_probe_driver_from_assignment_removed_by_wrapper() {
    let temp = tempdir().unwrap();
    let secret = "doctor-removed-assignment-secret";
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        &format!(
            "DATABASE_URL=postgres://doctor:{secret}@localhost/demo env -u DATABASE_URL cargo-sqlx sqlx prepare"
        ),
    );
    let tools = tempdir().unwrap();
    let marker = tools.path().join("probe-marker");
    write_test_executable(&tools.path().join("env"), "#!/bin/sh\nexit 0\n");
    write_test_executable(
        &tools.path().join("cargo-sqlx"),
        &format!("#!/bin/sh\nprintf ran > '{}'\nexit 0\n", marker.display()),
    );
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check = required_tools_check_with_environment(
        &ctx,
        &doctor_environment(tools.path(), Some("sqlite:ambient.db")),
    );

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present_unverified");
    assert!(!marker.exists());
    assert_eq!(
        cargo_sqlx_program(&check)["driver_probe"]["driver"],
        json!(null)
    );
    assert_eq!(
        cargo_sqlx_program(&check)["driver_probe"]["status"],
        "unverified"
    );
    let serialized = serde_json::to_string(&check).unwrap();
    assert!(!serialized.contains(secret));
    assert!(!serialized.contains("postgres://doctor"));
}

#[cfg(unix)]
#[test]
fn required_tools_preserve_sqlx_probe_through_external_wrapper_chain() {
    let repo = tempdir().unwrap();
    let tools = tempdir().unwrap();
    for executable in ["env", "nohup", "time"] {
        write_test_executable(&tools.path().join(executable), "#!/bin/sh\nexit 0\n");
    }
    let marker = tools.path().join("sqlx-probe-marker");
    write_test_executable(
        &tools.path().join("sqlx"),
        &format!("#!/bin/sh\nprintf ran > '{}'\nexit 0\n", marker.display()),
    );
    let time = tools.path().join("time");
    write_sqlx_doctor_fixture_with_command(
        repo.path(),
        &format!(
            "env nohup {} sqlx prepare -D sqlite:wrapper-chain.db",
            time.display()
        ),
    );
    let ctx = RepoContext::load_from_root(repo.path().to_path_buf()).unwrap();
    let check =
        required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present");
    assert!(marker.exists());
    assert_eq!(
        cargo_sqlx_program(&check)["driver_probe"]["status"],
        "compatible"
    );
    let serialized = serde_json::to_string(&check).unwrap();
    assert!(!serialized.contains(&time.display().to_string()));
    assert!(!serialized.contains("wrapper-chain.db"));
}

#[cfg(unix)]
#[test]
fn required_tools_redacts_sqlx_commands_even_when_resolution_is_ambiguous() {
    let temp = tempdir().unwrap();
    let secret = "doctor-inline-password";
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        &format!(
            "cargo sqlx prepare --database-url='postgres://doctor-user:{secret}@localhost/demo"
        ),
    );

    let tools = tempdir().unwrap();
    let bin = tools.path().to_path_buf();
    write_test_executable(&bin.join("cargo"), "#!/bin/sh\nexit 0\n");
    write_test_executable(&bin.join("cargo-sqlx"), "#!/bin/sh\nexit 0\n");
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check = required_tools_check_with_environment(&ctx, &doctor_environment(&bin, None));
    assert!(check.ok);
    assert_eq!(check.status, "present_unverified");
    assert!(check.fix.is_none());
    assert_eq!(
        check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["command_key"] == "sqlx_check_command")
            .unwrap()["command"],
        "<redacted: sqlx_check_command>"
    );

    let output = output(None, vec![check]);
    let serialized = serde_json::to_string(&output).unwrap();
    let summary = format_summary(&output);
    assert!(!serialized.contains(secret));
    assert!(!serialized.contains("postgres://doctor-user"));
    assert!(!summary.contains(secret));
    assert!(!summary.contains("postgres://doctor-user"));
    assert!(summary.contains("present_unverified"));
    assert!(summary.contains("scripts/jig check sqlx"));
    assert!(summary.contains("Next required step: none"));
}

#[cfg(unix)]
#[test]
fn required_tools_redact_unquoted_database_url_expansion_values() {
    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        "cargo sqlx prepare --database-url=$DATABASE_URL",
    );
    let tools = tempdir().unwrap();
    write_test_executable(&tools.path().join("cargo"), "#!/bin/sh\nexit 0\n");
    let secret = "doctor-unquoted-expansion-secret";
    let database_url = format!("sqlite:first.db -D postgres://doctor:{secret}@localhost/injected");
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check = required_tools_check_with_environment(
        &ctx,
        &doctor_environment(tools.path(), Some(&database_url)),
    );

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present_unverified");
    let serialized = serde_json::to_string(&check).unwrap();
    assert!(!serialized.contains(secret));
    assert!(!serialized.contains("postgres://doctor"));
}

#[cfg(unix)]
#[test]
fn required_tools_nearest_dotenv_diagnostics_do_not_leak_values_or_home() {
    let temp = tempdir().unwrap();
    let child = temp.path().join("crates/api");
    fs::create_dir_all(&child).unwrap();
    write_sqlx_doctor_fixture_with_command(temp.path(), "cd crates/api && cargo sqlx prepare");
    let parent_secret = "parent-database-secret";
    let child_secret = "nearest-unrelated-secret";
    fs::write(
        temp.path().join(".env"),
        format!("DATABASE_URL=postgres://doctor:{parent_secret}@localhost/demo\n"),
    )
    .unwrap();
    fs::write(child.join(".env"), format!("OTHER_VALUE={child_secret}\n")).unwrap();
    let tools = tempdir().unwrap();
    write_test_executable(&tools.path().join("cargo"), "#!/bin/sh\nexit 0\n");
    write_test_executable(&tools.path().join("cargo-sqlx"), "#!/bin/sh\nexit 0\n");
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check =
        required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present_unverified");
    let output = output(None, vec![check]);
    let serialized = serde_json::to_string(&output).unwrap();
    let summary = format_summary(&output);
    for rendered in [&serialized, &summary] {
        assert!(!rendered.contains(parent_secret));
        assert!(!rendered.contains(child_secret));
        if let Some(home) = env::var_os("HOME").and_then(|home| home.into_string().ok()) {
            assert!(!rendered.contains(&home));
        }
    }
}

#[cfg(unix)]
#[test]
fn required_tools_redacts_url_tokens_misparsed_as_sqlx_executables() {
    let temp = tempdir().unwrap();
    let secret = "misparsed-inline-password";
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        &format!(
            "postgres://doctor-user:{secret}@localhost/demo; cargo sqlx prepare --database-url='$DYNAMIC_DATABASE_URL'"
        ),
    );

    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    write_test_executable(&bin.join("cargo"), "#!/bin/sh\nexit 0\n");
    write_test_executable(&bin.join("cargo-sqlx"), "#!/bin/sh\nexit 0\n");
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check = required_tools_check_with_environment(&ctx, &doctor_environment(&bin, None));
    assert!(!check.ok);
    assert_eq!(check.status, "missing");
    let serialized = serde_json::to_string(&check).unwrap();
    let summary = format_summary(&output(None, vec![check]));
    assert!(!serialized.contains(secret));
    assert!(!serialized.contains("postgres://doctor-user"));
    assert!(!summary.contains(secret));
    assert!(!summary.contains("postgres://doctor-user"));
    assert!(serialized.contains("<redacted: command executable>"));
}

#[cfg(unix)]
#[test]
fn required_tools_treats_indeterminate_sqlx_probe_as_present_unverified() {
    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        "cargo-sqlx sqlx prepare -D sqlite:doctor.db",
    );

    let tools = tempdir().unwrap();
    let bin = tools.path().to_path_buf();
    write_test_executable(
        &bin.join("cargo-sqlx"),
        "#!/bin/sh\nprintf '%s\\n' 'unexpected doctor probe response'\nexit 2\n",
    );
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check = required_tools_check_with_environment(
        &ctx,
        &doctor_environment(&bin, Some("sqlite:doctor.db")),
    );
    assert!(check.ok);
    assert_eq!(check.status, "present_unverified");
    assert!(check.fix.is_none());
    assert_eq!(
        cargo_sqlx_program(&check)["driver_probe"]["status"],
        "unverified"
    );
    assert!(cargo_sqlx_program(&check)["driver_probe"]["compatible"].is_null());
    assert!(check.detail.contains("scripts/jig check sqlx"));
    assert!(check.detail.contains("in the SQLx CLI"));
    assert!(!check.detail.contains("in cargo-sqlx"));
    assert!(!check.detail.contains("reinstall"));
}

#[cfg(unix)]
#[test]
fn required_tools_does_not_execute_sqlx_probe_with_shell_environment_poisoning() {
    use std::os::unix::ffi::OsStringExt;

    let secret = "shell-environment-poison-secret";
    let issue = |controls: [Option<&OsStr>; 7], variables: Vec<(OsString, OsString)>| {
        inherited_shell_environment_issue(
            [
                (ShellEnvironmentIssue::BashEnv, controls[0]),
                (ShellEnvironmentIssue::PosixEnv, controls[1]),
                (ShellEnvironmentIssue::CdPath, controls[2]),
                (ShellEnvironmentIssue::ShellOptions, controls[3]),
                (ShellEnvironmentIssue::BashOptions, controls[4]),
                (ShellEnvironmentIssue::TracePrompt, controls[5]),
                (ShellEnvironmentIssue::TraceFileDescriptor, controls[6]),
            ],
            variables,
        )
    };
    let scenarios = [
        issue(
            [Some(OsStr::new(secret)), None, None, None, None, None, None],
            Vec::new(),
        ),
        issue(
            [None, Some(OsStr::new(secret)), None, None, None, None, None],
            Vec::new(),
        ),
        issue(
            [None, None, Some(OsStr::new(secret)), None, None, None, None],
            Vec::new(),
        ),
        issue(
            [None, None, None, Some(OsStr::new(secret)), None, None, None],
            Vec::new(),
        ),
        issue(
            [None, None, None, None, Some(OsStr::new(secret)), None, None],
            Vec::new(),
        ),
        issue(
            [None, None, None, None, None, Some(OsStr::new(secret)), None],
            Vec::new(),
        ),
        issue(
            [None, None, None, None, None, None, Some(OsStr::new(secret))],
            Vec::new(),
        ),
        issue(
            [None; 7],
            vec![(
                OsString::from("BASH_FUNC_sqlx%%"),
                OsString::from(format!("() {{ printf {secret}; }}")),
            )],
        ),
        issue(
            [None; 7],
            vec![(
                OsString::from_vec(b"BASH_FUNC_sqlx_\xff%%".to_vec()),
                OsString::from(format!("() {{ printf {secret}; }}")),
            )],
        ),
    ];
    assert_eq!(
        scenarios,
        [
            Some(ShellEnvironmentIssue::BashEnv),
            Some(ShellEnvironmentIssue::PosixEnv),
            Some(ShellEnvironmentIssue::CdPath),
            Some(ShellEnvironmentIssue::ShellOptions),
            Some(ShellEnvironmentIssue::BashOptions),
            Some(ShellEnvironmentIssue::TracePrompt),
            Some(ShellEnvironmentIssue::TraceFileDescriptor),
            Some(ShellEnvironmentIssue::ImportedFunction),
            Some(ShellEnvironmentIssue::ImportedFunction),
        ]
    );

    for (index, issue) in scenarios.into_iter().enumerate() {
        let temp = tempdir().unwrap();
        write_sqlx_doctor_fixture_with_command(temp.path(), "sqlx prepare -D sqlite:doctor.db");
        let bin = temp.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let marker = temp.path().join(format!("probe-ran-{index}"));
        write_test_executable(
            &bin.join("sqlx"),
            &format!("#!/bin/sh\nprintf ran > '{}'\nexit 0\n", marker.display()),
        );
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
        let mut environment = doctor_environment(&bin, Some("sqlite:doctor.db"));
        environment.shell_environment_issue = issue;

        let check = required_tools_check_with_environment(&ctx, &environment);

        assert!(check.ok, "{}", check.detail);
        assert_eq!(check.status, "present_unverified");
        assert!(
            check
                .detail
                .contains("external executable reference(s) inspected")
        );
        assert!(
            !marker.exists(),
            "ambient shell state allowed probe execution"
        );
        let probe = &cargo_sqlx_program(&check)["driver_probe"];
        assert!(probe["driver"].is_null());
        assert!(probe["source"].is_null());
        assert_eq!(probe["status"], "unverified");
        let serialized = serde_json::to_string(&check).unwrap();
        assert!(!serialized.contains(secret));
        assert!(serialized.contains("inherited shell state"));
    }

    assert_eq!(issue([None; 7], Vec::new()), None);
    assert_eq!(issue([Some(OsStr::new("")); 7], Vec::new()), None);
}

#[test]
fn doctor_environment_capture_audits_bash_startup_state_without_retaining_values() {
    let _env = lock_env();
    let _posix_env = EnvVarGuard::remove("ENV");
    let _cdpath = EnvVarGuard::remove("CDPATH");
    let secret = "doctor-bash-env-secret";
    let _bash_env = EnvVarGuard::set("BASH_ENV", secret);

    let environment = DoctorEnvironment::capture();

    assert_eq!(
        environment.shell_environment_issue,
        Some(ShellEnvironmentIssue::BashEnv)
    );
    assert!(!format!("{environment:?}").contains(secret));
}

#[cfg(unix)]
#[test]
fn required_tools_does_not_trust_ambiguous_cargo_sqlx_dispatch() {
    for case in [
        "environment",
        "command_environment",
        "inline",
        "inline_include",
        "config",
        "config_include",
        "nested_config",
        "relative_cargo_home",
    ] {
        let temp = tempdir().unwrap();
        let command = match case {
            "command_environment" => {
                "CARGO_ALIAS_SQLX='run --package fake' cargo sqlx prepare -D sqlite:doctor.db"
            }
            "inline" => {
                "cargo --config alias.sqlx='run --package fake' sqlx prepare -D sqlite:doctor.db"
            }
            "inline_include" => {
                "cargo --config include='dispatch.toml' sqlx prepare -D sqlite:doctor.db"
            }
            "nested_config" => "cd crates/api && cargo sqlx prepare -D sqlite:doctor.db",
            _ => "cargo sqlx prepare -D sqlite:doctor.db",
        };
        write_sqlx_doctor_fixture_with_command(temp.path(), command);
        if matches!(case, "config" | "config_include" | "nested_config") {
            let config_dir = if case == "nested_config" {
                temp.path().join("crates/api/.cargo")
            } else {
                temp.path().join(".cargo")
            };
            fs::create_dir_all(&config_dir).unwrap();
            fs::write(
                config_dir.join("config.toml"),
                if case == "config_include" {
                    "include = 'dispatch.toml'\n"
                } else {
                    "[alias]\nsqlx = 'run --package fake'\n"
                },
            )
            .unwrap();
        }
        let tools = tempdir().unwrap();
        write_test_executable(&tools.path().join("cargo"), "#!/bin/sh\nexit 0\n");
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
        let mut environment = doctor_environment(tools.path(), None);
        if case == "environment" {
            environment.cargo_alias_sqlx = Some("run --package fake".into());
        } else if case == "relative_cargo_home" {
            environment.cargo_home = Some("relative-cargo-home".into());
        }

        let check = required_tools_check_with_environment(&ctx, &environment);

        assert!(check.ok, "{case}: {}", check.detail);
        assert_eq!(check.status, "present_unverified", "{case}");
        assert!(check.detail.contains("cargo sqlx dispatch"), "{case}");
        if matches!(
            case,
            "inline" | "inline_include" | "config" | "config_include" | "nested_config"
        ) {
            assert!(check.detail.contains("config"), "{case}: {}", check.detail);
        }
        if matches!(case, "environment" | "command_environment") {
            let detail = check.detail.to_ascii_lowercase();
            assert!(
                detail.contains("alias") || detail.contains("home"),
                "{case}: {}",
                check.detail,
            );
        }
        if case == "relative_cargo_home" {
            assert!(check.detail.contains("config"), "{case}: {}", check.detail);
        }
        assert_eq!(cargo_sqlx_program(&check)["present"], true, "{case}");
        assert_eq!(
            cargo_sqlx_program(&check)["driver_probe"]["status"],
            "unverified",
            "{case}",
        );
        assert!(
            check.data["tools"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|tool| tool["programs"].as_array().unwrap())
                .all(|program| program["program"] != "cargo-sqlx"),
            "{case}",
        );
    }
}

#[cfg(unix)]
#[test]
fn unresolved_cargo_does_not_probe_an_external_subcommand() {
    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(temp.path(), "cargo sqlx prepare -D sqlite:doctor.db");
    let tools = tempdir().unwrap();
    let probe_marker = temp.path().join("probe-marker");
    write_test_executable(
        &tools.path().join("cargo-sqlx"),
        &format!(
            "#!/bin/sh\nprintf probed > '{}'\nexit 0\n",
            probe_marker.display()
        ),
    );
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check =
        required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));

    assert!(!check.ok);
    assert_eq!(check.status, "missing");
    assert!(!probe_marker.exists());
    assert_eq!(
        cargo_sqlx_program(&check)["driver_probe"]["status"],
        "unverified"
    );
    assert!(check.detail.contains("external cargo path does not prove"));
}

#[cfg(unix)]
#[test]
fn required_tools_reports_explicit_cargo_wrapper_without_subcommand_probe() {
    let temp = tempdir().unwrap();
    fs::create_dir(temp.path().join("scripts")).unwrap();
    let cargo = temp.path().join("scripts/cargo");
    write_test_executable(&cargo, "#!/bin/sh\nexit 0\n");
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        "scripts/cargo sqlx prepare -D sqlite:doctor.db",
    );
    let tools = tempdir().unwrap();
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check =
        required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present_unverified");
    assert!(check.detail.contains("external cargo path does not prove"));
    assert!(
        check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|tool| tool["programs"].as_array().unwrap())
            .all(|program| program["program"] != "cargo-sqlx")
    );
}

#[cfg(unix)]
#[test]
fn cargo_alias_leaves_cargo_unverified_while_direct_clis_probe() {
    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        "cargo sqlx prepare -D sqlite:alias.db && sqlx prepare -D sqlite:direct.db && cargo-sqlx sqlx prepare -D sqlite:shim.db",
    );
    let tools = tempdir().unwrap();
    let probe_log = temp.path().join("probe-log");
    write_test_executable(&tools.path().join("cargo"), "#!/bin/sh\nexit 0\n");
    write_test_executable(
        &tools.path().join("sqlx"),
        &format!("#!/bin/sh\nprintf d >> '{}'\nexit 0\n", probe_log.display()),
    );
    write_test_executable(
        &tools.path().join("cargo-sqlx"),
        &format!("#!/bin/sh\nprintf c >> '{}'\nexit 0\n", probe_log.display()),
    );
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
    let mut environment = doctor_environment(tools.path(), None);
    environment.cargo_alias_sqlx = Some("run --package fake".into());

    let check = required_tools_check_with_environment(&ctx, &environment);

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present_unverified");
    assert_eq!(fs::read_to_string(probe_log).unwrap(), "dc");
    let probes = check.data["tools"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|tool| tool["programs"].as_array().unwrap())
        .filter_map(|program| program.get("driver_probe"))
        .collect::<Vec<_>>();
    assert_eq!(
        probes
            .iter()
            .filter(|probe| probe["status"] == "unverified")
            .count(),
        1
    );
    assert_eq!(
        probes
            .iter()
            .filter(|probe| probe["status"] == "compatible")
            .count(),
        2
    );
}

#[cfg(unix)]
#[test]
fn required_tools_never_probes_cargo_subcommand_dispatch() {
    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        "DATABASE_URL=sqlite:first.db cargo sqlx prepare && cargo sqlx migrate info --database-url=sqlite:second.db",
    );

    let tools = tempdir().unwrap();
    let bin = tools.path().to_path_buf();
    let probe_count = temp.path().join("probe-count");
    write_test_executable(&bin.join("cargo"), "#!/bin/sh\nexit 0\n");
    write_test_executable(
        &bin.join("cargo-sqlx"),
        &format!(
            "#!/bin/sh\nprintf x >> '{}'\nexit 0\n",
            probe_count.display()
        ),
    );
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check = required_tools_check_with_environment(&ctx, &doctor_environment(&bin, None));
    assert!(check.ok);
    assert_eq!(check.status, "present_unverified");
    assert!(!probe_count.exists());
    assert_eq!(
        check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|tool| tool["programs"].as_array().unwrap())
            .filter(|program| program.get("driver_probe").is_some())
            .count(),
        2
    );
    assert!(
        check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|tool| tool["programs"].as_array().unwrap())
            .filter_map(|program| program.get("driver_probe"))
            .all(|probe| probe["status"] == "unverified")
    );
}

#[cfg(unix)]
#[test]
fn required_tools_neither_resolves_nor_probes_changed_path_invocations() {
    for repo_tool_is_present in [false, true] {
        let temp = tempdir().unwrap();
        let repo_tools = temp.path().join("repo-secret-bin");
        fs::create_dir(&repo_tools).unwrap();
        write_sqlx_doctor_fixture_with_command(
            temp.path(),
            "PATH=repo-secret-bin; sqlx prepare -D sqlite:path-secret.db",
        );

        let ambient = tempdir().unwrap();
        let marker = temp.path().join("path-probe-must-not-run");
        let body = format!("#!/bin/sh\nprintf ran > '{}'\nexit 0\n", marker.display());
        if repo_tool_is_present {
            write_test_executable(&repo_tools.join("sqlx"), &body);
        } else {
            write_test_executable(&ambient.path().join("sqlx"), &body);
        }
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let check =
            required_tools_check_with_environment(&ctx, &doctor_environment(ambient.path(), None));

        assert!(check.ok, "{}", check.detail);
        assert_eq!(check.status, "present_unverified");
        let sqlx_tool = check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["command_key"] == "sqlx_check_command")
            .unwrap();
        assert!(sqlx_tool["present"].is_null());
        assert!(sqlx_tool["programs"][0]["present"].is_null());
        assert_eq!(
            sqlx_tool["programs"][0]["driver_probe"]["status"],
            "unverified"
        );
        assert!(!marker.exists());

        let serialized = serde_json::to_string(&check).unwrap();
        assert!(!serialized.contains("repo-secret-bin"));
        assert!(!serialized.contains("path-secret"));
        assert!(serialized.contains("may change the executable lookup context"));
    }
}

#[cfg(unix)]
#[test]
fn required_tools_localizes_changed_path_and_only_probes_captured_path() {
    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        "PATH=repo-tools sqlx prepare -D sqlite:first.db && sqlx prepare -D sqlite:second.db",
    );
    let tools = tempdir().unwrap();
    let marker = temp.path().join("ambient-probe-count");
    let repo_tools = temp.path().join("repo-tools");
    fs::create_dir(&repo_tools).unwrap();
    write_test_executable(
        &repo_tools.join("sqlx"),
        &format!("#!/bin/sh\nprintf r >> '{}'\nexit 0\n", marker.display()),
    );
    write_test_executable(
        &tools.path().join("sqlx"),
        &format!("#!/bin/sh\nprintf x >> '{}'\nexit 0\n", marker.display()),
    );
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check =
        required_tools_check_with_environment(&ctx, &doctor_environment(tools.path(), None));

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present_unverified");
    let programs = check.data["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["command_key"] == "sqlx_check_command")
        .unwrap()["programs"]
        .as_array()
        .unwrap();
    assert_eq!(programs.len(), 2);
    assert_eq!(programs[0]["present"], true);
    assert_eq!(programs[0]["driver_probe"]["status"], "unverified");
    assert_eq!(programs[1]["present"], true);
    assert_eq!(programs[1]["driver_probe"]["status"], "compatible");
    assert_eq!(fs::read_to_string(marker).unwrap(), "x");
}

#[cfg(unix)]
#[test]
fn doctor_reuses_one_signal_generation_per_batch_and_allows_later_batches() {
    const HELPER: &str = "JIG_SQLX_PROBE_REUSABLE_BATCH_HELPER";
    if let Some(root) = std::env::var_os(HELPER) {
        let root = PathBuf::from(root);
        let ctx = RepoContext::load_from_root(root.clone()).unwrap();
        let first = doctor_context_checks(&ctx);
        assert!(first.required_tools.ok, "{}", first.required_tools.detail);
        assert_eq!(
            cargo_sqlx_program(&first.required_tools)["driver_probe"]["status"],
            "compatible"
        );
        assert_eq!(first.agent.status, "missing", "{}", first.agent.detail);
        assert_eq!(first.agent.data["codex"]["available"], true);
        assert_eq!(first.proxy.status, "not running", "{}", first.proxy.detail);
        assert_eq!(
            fs::read_to_string(root.join("probe-count")).unwrap(),
            "dckp"
        );

        let second = doctor_context_checks(&ctx);
        assert!(second.required_tools.ok, "{}", second.required_tools.detail);
        assert_eq!(second.required_tools.status, "present");
        assert_eq!(
            cargo_sqlx_program(&second.required_tools)["driver_probe"]["status"],
            "compatible"
        );
        assert_eq!(second.agent.status, "missing", "{}", second.agent.detail);
        assert_eq!(second.agent.data["codex"]["available"], true);
        assert_eq!(
            second.proxy.status, "not running",
            "{}",
            second.proxy.detail
        );
        assert_eq!(
            fs::read_to_string(root.join("probe-count")).unwrap(),
            "dckpdckp"
        );
        return;
    }

    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        "sqlx prepare -D sqlite:reusable.db && cargo-sqlx sqlx prepare -D sqlite:reusable.db",
    );
    let tools = tempdir().unwrap();
    write_test_executable(
        &tools.path().join("sqlx"),
        &format!(
            "#!/bin/sh\nprintf d >> '{}'\nexit 0\n",
            temp.path().join("probe-count").display()
        ),
    );
    write_test_executable(
        &tools.path().join("cargo-sqlx"),
        &format!(
            "#!/bin/sh\nprintf c >> '{}'\nexit 0\n",
            temp.path().join("probe-count").display()
        ),
    );
    fs::write(
        temp.path().join(".jig.toml"),
        format!(
            "{}\n[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\ncoverage_threshold = 80\n",
            fs::read_to_string(temp.path().join(".jig.toml")).unwrap()
        ),
    )
    .unwrap();
    fs::create_dir(temp.path().join("web")).unwrap();
    fs::write(
            temp.path().join(".jig.toml"),
            fs::read_to_string(temp.path().join(".jig.toml"))
                .unwrap()
                .replace(
                    "[agent_tooling.codex]\nmarketplaces = []",
                    "[[agent_tooling.codex.marketplaces]]\nid = \"test-skills\"\nsource = \"example/test-skills\"",
                ),
        )
        .unwrap();
    let codex = temp.path().join("codex");
    write_test_executable(
        &codex,
        &format!(
            "#!/bin/sh\nprintf k >> '{}'\n[ \"$*\" = \"plugin marketplace add --help\" ]\n",
            temp.path().join("probe-count").display()
        ),
    );
    write_test_executable(
        &temp.path().join("scripts/jig"),
        &format!(
            "#!/bin/sh\nprintf p >> '{}'\nprintf '%s\\n' '{{\"ok\":true,\"running\":false,\"routes\":[]}}'\n",
            temp.path().join("probe-count").display()
        ),
    );
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "doctor::tests::doctor_reuses_one_signal_generation_per_batch_and_allows_later_batches",
            "--nocapture",
        ])
        .env(HELPER, temp.path())
        .env("PATH", fs::canonicalize(tools.path()).unwrap())
        .env("JIG_CODEX_BIN", codex)
        .env("CODEX_HOME", temp.path().join("codex-home"))
        .env_remove("BASH_ENV")
        .env_remove("ENV")
        .env_remove("CDPATH")
        .status()
        .unwrap();
    assert!(
        status.success(),
        "reusable batch helper exited with {status}"
    );
}

#[cfg(unix)]
#[test]
fn signal_retirement_failure_invalidates_every_configured_process_check() {
    let temp = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(temp.path(), "sqlx prepare -D sqlite:retirement.db");
    let config_path = temp.path().join(".jig.toml");
    fs::write(
            &config_path,
            format!(
                "{}\n[[frontend_apps]]\nname = \"web\"\ndir = \"web\"\ncoverage_threshold = 80\n",
                fs::read_to_string(&config_path).unwrap().replace(
                    "[agent_tooling.codex]\nmarketplaces = []",
                    "[[agent_tooling.codex.marketplaces]]\nid = \"test-skills\"\nsource = \"example/test-skills\"",
                )
            ),
        )
        .unwrap();
    fs::create_dir(temp.path().join("web")).unwrap();
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();
    let mut checks = DoctorContextChecks {
        required_tools: check(
            "required_tools",
            "Required tools",
            true,
            true,
            "present",
            "present",
        ),
        agent: check(
            "agent_skills",
            "Agent skills",
            false,
            true,
            "installed",
            "installed",
        ),
        proxy: check("proxy", "Dev proxy", false, true, "running", "running"),
    };

    mark_doctor_signal_retirement_failure(&ctx, &mut checks);

    assert_eq!(checks.required_tools.status, "present_unverified");
    assert!(
        checks
            .required_tools
            .detail
            .contains("could not retire safely")
    );
    for process_check in [&checks.agent, &checks.proxy] {
        assert!(!process_check.ok);
        assert_eq!(process_check.status, "error");
        assert!(process_check.detail.contains("could not retire safely"));
    }
}

#[cfg(unix)]
#[test]
fn required_tools_probes_bare_path_forms_but_not_explicit_sqlx_paths() {
    let temp = tempdir().unwrap();
    let tools = tempdir().unwrap();
    let bin = tools.path().to_path_buf();
    let probe_log = temp.path().join("probe-log");
    write_test_executable(&bin.join("cargo"), "#!/bin/sh\nexit 0\n");
    write_test_executable(
        &bin.join("sqlx"),
        &format!(
            "#!/bin/sh\n[ \"$1\" = migrate ] || exit 9\nprintf d >> '{}'\nexit 0\n",
            probe_log.display()
        ),
    );
    write_test_executable(
        &bin.join("cargo-sqlx"),
        &format!(
            "#!/bin/sh\n[ \"$1\" = sqlx ] || exit 9\n[ \"$2\" = migrate ] || exit 9\nprintf c >> '{}'\nexit 0\n",
            probe_log.display()
        ),
    );
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        &format!(
            "CARGO=cargo sqlx prepare -D sqlite:direct.db && {} sqlx prepare -Dsqlite:shim.db && {} sqlx prepare -D=sqlite:cargo.db",
            bin.join("cargo-sqlx").display(),
            bin.join("cargo").display(),
        ),
    );
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check = required_tools_check_with_environment(&ctx, &doctor_environment(&bin, None));

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present_unverified");
    assert_eq!(fs::read_to_string(probe_log).unwrap(), "d");
    let probes = check.data["tools"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|tool| tool["programs"].as_array().unwrap())
        .filter_map(|program| program.get("driver_probe"))
        .collect::<Vec<_>>();
    assert_eq!(probes.len(), 3);
    assert_eq!(
        probes
            .iter()
            .filter(|probe| probe["status"] == "compatible")
            .count(),
        1
    );
    assert_eq!(
        probes
            .iter()
            .filter(|probe| probe["status"] == "unverified")
            .count(),
        2
    );
    let serialized = serde_json::to_string(&check).unwrap();
    assert!(!serialized.contains(&bin.display().to_string()));
    assert!(!serialized.contains("sqlite:direct.db"));
}

#[cfg(unix)]
#[test]
fn required_tools_never_executes_repo_local_or_explicit_sqlx_tools() {
    let temp = tempdir().unwrap();
    let repo_bin = temp.path().join("bin");
    let repo_scripts = temp.path().join("scripts");
    fs::create_dir(&repo_bin).unwrap();
    fs::create_dir(&repo_scripts).unwrap();
    let external = tempdir().unwrap();
    let marker = temp.path().join("probe-must-not-run");
    let body = format!("#!/bin/sh\nprintf ran >> '{}'\nexit 0\n", marker.display());
    write_test_executable(&repo_bin.join("sqlx"), &body);
    write_test_executable(&repo_scripts.join("sqlx"), &body);
    write_test_executable(&external.path().join("sqlx"), &body);
    let relative_external = Path::new("..").join(
        external
            .path()
            .file_name()
            .expect("temporary tool directory has a basename"),
    );
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        &format!(
            "sqlx prepare -D sqlite:repo-path.db && scripts/sqlx prepare -D sqlite:repo-explicit.db && {}/sqlx prepare -D sqlite:custom-relative.db && {} prepare -D sqlite:custom-absolute.db",
            relative_external.display(),
            external.path().join("sqlx").display(),
        ),
    );
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check = required_tools_check_with_environment(&ctx, &doctor_environment(&repo_bin, None));

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present_unverified");
    assert!(!marker.exists());
    let serialized = serde_json::to_string(&check).unwrap();
    assert!(!serialized.contains(&temp.path().display().to_string()));
    assert!(!serialized.contains(&external.path().display().to_string()));

    let symlink_repo = tempdir().unwrap();
    let symlink_scripts = symlink_repo.path().join("scripts");
    let symlink_marker = symlink_repo.path().join("probe-must-not-run");
    write_sqlx_doctor_fixture_with_command(
        symlink_repo.path(),
        "sqlx prepare -D sqlite:symlink.db",
    );
    let symlink_body = format!(
        "#!/bin/sh\nprintf ran >> '{}'\nexit 0\n",
        symlink_marker.display()
    );
    write_test_executable(&symlink_scripts.join("sqlx"), &symlink_body);
    let symlink_path = tempdir().unwrap();
    std::os::unix::fs::symlink(
        symlink_scripts.join("sqlx"),
        symlink_path.path().join("sqlx"),
    )
    .unwrap();
    let symlink_ctx = RepoContext::load_from_root(symlink_repo.path().to_path_buf()).unwrap();
    let symlink_check = required_tools_check_with_environment(
        &symlink_ctx,
        &doctor_environment(symlink_path.path(), None),
    );
    assert!(symlink_check.ok, "{}", symlink_check.detail);
    assert_eq!(symlink_check.status, "present_unverified");
    assert!(!symlink_marker.exists());

    let linked_directory_repo = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(
        linked_directory_repo.path(),
        "sqlx prepare -D sqlite:linked-directory.db",
    );
    let real_tools = tempdir().unwrap();
    let linked_directory_marker = linked_directory_repo.path().join("probe-must-not-run");
    write_test_executable(
        &real_tools.path().join("sqlx"),
        &format!(
            "#!/bin/sh\nprintf ran > '{}'\nexit 0\n",
            linked_directory_marker.display()
        ),
    );
    let path_container = tempdir().unwrap();
    let linked_tools = path_container.path().join("linked-tools");
    std::os::unix::fs::symlink(real_tools.path(), &linked_tools).unwrap();
    let linked_directory_ctx =
        RepoContext::load_from_root(linked_directory_repo.path().to_path_buf()).unwrap();
    let linked_directory_check = required_tools_check_with_environment(
        &linked_directory_ctx,
        &DoctorEnvironment {
            search_path: Some(linked_tools.into_os_string()),
            ..DoctorEnvironment::default()
        },
    );
    assert!(
        linked_directory_check.ok,
        "{}",
        linked_directory_check.detail
    );
    assert_eq!(linked_directory_check.status, "present_unverified");
    assert!(!linked_directory_marker.exists());
}

#[cfg(unix)]
#[test]
fn required_tools_ignores_commented_sqlx_urls_and_wrapper_separator() {
    let temp = tempdir().unwrap();
    let secret = "commented-database-secret";
    write_sqlx_doctor_fixture_with_command(
        temp.path(),
        &format!(
            "command -v cargo >/dev/null && command -- cargo sqlx prepare # -D postgres://doctor-user:{secret}@localhost/demo"
        ),
    );
    let tools = tempdir().unwrap();
    let bin = tools.path().to_path_buf();
    write_test_executable(&bin.join("cargo"), "#!/bin/sh\nexit 0\n");
    write_test_executable(&bin.join("cargo-sqlx"), "#!/bin/sh\nexit 0\n");
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check = required_tools_check_with_environment(
        &ctx,
        &doctor_environment(&bin, Some("sqlite:doctor.db")),
    );

    assert!(check.ok, "{}", check.detail);
    assert_eq!(check.status, "present_unverified");
    assert_eq!(
        cargo_sqlx_program(&check)["driver_probe"]["driver"],
        "sqlite"
    );
    assert_eq!(
        cargo_sqlx_program(&check)["driver_probe"]["status"],
        "unverified"
    );
    assert!(
        check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|tool| tool["programs"].as_array().unwrap())
            .all(|program| program["program"] != "cargo-sqlx")
    );
    assert!(
        check.data["tools"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|tool| tool["programs"].as_array().unwrap())
            .all(|program| !matches!(program["program"].as_str(), Some("--" | "-v")))
    );
    let output = output(None, vec![check]);
    let serialized = serde_json::to_string(&output).unwrap();
    let summary = format_summary(&output);
    for rendered in [&serialized, &summary] {
        assert!(!rendered.contains(secret));
        assert!(!rendered.contains("postgres://doctor-user"));
    }
}

#[cfg(unix)]
#[test]
fn required_tools_fails_open_for_nontransparent_wrapper_options() {
    for command in [
        "command -p cargo sqlx prepare -D sqlite:command-p-wrapper-secret.db",
        "exec -a private-argv-zero cargo sqlx prepare -D sqlite:exec-a-wrapper-secret.db",
        "exec -c cargo sqlx prepare",
    ] {
        let temp = tempdir().unwrap();
        write_sqlx_doctor_fixture_with_command(temp.path(), command);
        let bin = temp.path().join("bin");
        fs::create_dir(&bin).unwrap();
        write_test_executable(&bin.join("cargo"), "#!/bin/sh\nexit 0\n");
        write_test_executable(&bin.join("cargo-sqlx"), "#!/bin/sh\nexit 0\n");
        let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

        let check = required_tools_check_with_environment(
            &ctx,
            &doctor_environment(&bin, Some("sqlite:ambient-wrapper-secret.db")),
        );

        assert!(check.ok, "{command:?}: {}", check.detail);
        assert_eq!(check.status, "present_unverified", "{command:?}");
        assert!(check.fix.is_none());
        assert!(
            check.data["tools"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|tool| tool["programs"].as_array().unwrap())
                .filter_map(|program| program["program"].as_str())
                .all(|program| matches!(program, "cargo" | "cargo-sqlx")),
            "{command:?}",
        );
        let serialized = serde_json::to_string(&check).unwrap();
        for secret in [
            "command-p-wrapper-secret",
            "exec-a-wrapper-secret",
            "ambient-wrapper-secret",
            "private-argv-zero",
        ] {
            assert!(!serialized.contains(secret), "{command:?}: leaked {secret}");
        }
    }
}

#[cfg(unix)]
#[test]
fn required_tools_marks_no_url_and_custom_sqlx_wrappers_unverified() {
    let no_url = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(no_url.path(), "cargo sqlx prepare --no-dotenv");
    let no_url_bin = no_url.path().join("bin");
    fs::create_dir(&no_url_bin).unwrap();
    write_test_executable(&no_url_bin.join("cargo"), "#!/bin/sh\nexit 0\n");
    write_test_executable(&no_url_bin.join("cargo-sqlx"), "#!/bin/sh\nexit 0\n");
    let no_url_ctx = RepoContext::load_from_root(no_url.path().to_path_buf()).unwrap();

    let no_url_check =
        required_tools_check_with_environment(&no_url_ctx, &doctor_environment(&no_url_bin, None));
    assert!(no_url_check.ok, "{}", no_url_check.detail);
    assert_eq!(no_url_check.status, "present_unverified");
    assert!(no_url_check.fix.is_none());
    assert!(no_url_check.detail.contains("scripts/jig check sqlx"));
    assert!(
        !no_url_check
            .detail
            .to_ascii_lowercase()
            .contains("reinstall")
    );

    let wrapper = tempdir().unwrap();
    write_sqlx_doctor_fixture_with_command(wrapper.path(), "scripts/private-sqlx-wrapper --check");
    write_test_executable(
        &wrapper.path().join("scripts/private-sqlx-wrapper"),
        "#!/bin/sh\nexit 99\n",
    );
    let wrapper_bin = wrapper.path().join("bin");
    fs::create_dir(&wrapper_bin).unwrap();
    let wrapper_ctx = RepoContext::load_from_root(wrapper.path().to_path_buf()).unwrap();

    let wrapper_check = required_tools_check_with_environment(
        &wrapper_ctx,
        &doctor_environment(&wrapper_bin, None),
    );
    assert!(wrapper_check.ok, "{}", wrapper_check.detail);
    assert_eq!(wrapper_check.status, "present_unverified");
    assert!(wrapper_check.fix.is_none());
    let serialized = serde_json::to_string(&wrapper_check).unwrap();
    assert!(!serialized.contains("private-sqlx-wrapper"));
    assert!(serialized.contains("<redacted: command executable>"));
}

#[cfg(unix)]
#[test]
fn required_tools_redacts_every_command_body_and_generic_credential_token() {
    let temp = tempdir().unwrap();
    write_doctor_fixture(temp.path());
    let secret = "generic-required-command-secret";
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "bootstrap_command = \"printf bootstrap\"",
        &format!(
            "bootstrap_command = {:?}",
            format!("postgres://doctor-user:{secret}@localhost/demo --check")
        ),
    );
    fs::write(config_path, config).unwrap();
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let ctx = RepoContext::load_from_root(temp.path().to_path_buf()).unwrap();

    let check = required_tools_check_with_environment(&ctx, &doctor_environment(&bin, None));
    assert!(!check.ok);
    assert_eq!(check.status, "missing");
    assert_eq!(
        check.data["tools"][0]["command"],
        "<redacted: bootstrap_command>"
    );
    assert_eq!(check.data["tools"][0]["command_redacted"], true);
    let output = output(None, vec![check]);
    let serialized = serde_json::to_string(&output).unwrap();
    let summary = format_summary(&output);
    for rendered in [&serialized, &summary] {
        assert!(!rendered.contains(secret));
        assert!(!rendered.contains("postgres://doctor-user"));
    }
    assert!(serialized.contains("<redacted: command executable>"));
}

#[test]
fn agent_next_step_prefers_command_shaped_steps() {
    let steps = vec![
        json!("Codex CLI is not available on PATH."),
        json!("Run `scripts/jig agent bootstrap` to register skills."),
    ];

    assert_eq!(
        agent_next_step(&steps),
        Some("Run `scripts/jig agent bootstrap` to register skills.")
    );
}

#[test]
fn summary_surfaces_optional_missing_agent_skills() {
    let output = json!({
        "ok": true,
        "repo": {
            "root": "/tmp/demo",
        },
        "checks": [
            {
                "label": "Agent skills",
                "status": "missing",
                "required": false,
                "ok": false,
            },
        ],
        "next_step": "Run `scripts/jig agent bootstrap`.",
    });

    let summary = format_summary(&output);

    assert!(summary.contains("Jig doctor: ready"));
    assert!(summary.contains("Agent skills: optional setup (missing, optional)"));
    assert!(summary.contains("Next required step: none"));
    assert!(summary.contains("Optional setup: scripts/jig agent bootstrap"));
}

#[test]
fn summary_surfaces_required_tool_missing_detail() {
    let output = json!({
        "ok": false,
        "repo": {
            "root": "/tmp/demo",
        },
        "checks": [
            {
                "label": "Required tools",
                "status": "missing",
                "required": true,
                "ok": false,
                "detail": "Missing command executable(s): schema_dump_command: scripts/dump-schema.sh",
            },
        ],
        "next_step": "Install the missing executable.",
    });

    let summary = format_summary(&output);

    assert!(summary.contains("Required tools: needs setup (missing, required)"));
    assert!(summary.contains(
        "Detail: Missing command executable(s): schema_dump_command: scripts/dump-schema.sh"
    ));
    assert!(summary.contains("Next required step: Install the missing executable."));
    assert!(summary.contains("Optional setup: none"));
}

#[test]
fn doctor_reports_unified_readiness_checks() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    write_doctor_fixture(temp.path());
    let _cwd = CurrentDirGuard::set(temp.path());

    let output = run().unwrap();

    assert_eq!(output["command"], "doctor");
    assert_eq!(output["repo"]["name"], "demo");
    assert_eq!(output["checks"].as_array().unwrap().len(), 8);
    assert!(check_by_id(&output, "runtime")["ok"].as_bool().unwrap());
    assert!(check_by_id(&output, "config")["ok"].as_bool().unwrap());
    assert!(check_by_id(&output, "contract")["ok"].as_bool().unwrap());
    assert!(
        check_by_id(&output, "required_tools")["ok"]
            .as_bool()
            .unwrap()
    );
    assert!(
        check_by_id(&output, "agent_skills")["ok"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(check_by_id(&output, "agent_skills")["required"], false);
    assert_eq!(check_by_id(&output, "proxy")["status"], "not configured");
    assert!(check_by_id(&output, "proxy")["ok"].as_bool().unwrap());
    assert_eq!(check_by_id(&output, "vault")["required"], false);
}

#[test]
fn doctor_reports_all_checks_when_config_is_invalid() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    fs::write(temp.path().join(".jig.toml"), "repo_name = \n").unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(
        temp.path().join("scripts/jig"),
        "#!/bin/sh\n# Runtime selection uses __runtime-compatible.\n",
    )
    .unwrap();
    let _cwd = CurrentDirGuard::set(temp.path());

    let output = run().unwrap();

    assert_eq!(output["command"], "doctor");
    assert_eq!(output["checks"].as_array().unwrap().len(), 8);
    assert_eq!(check_by_id(&output, "config")["status"], "invalid");
    assert_eq!(check_by_id(&output, "contract")["status"], "blocked");
    assert_eq!(check_by_id(&output, "required_tools")["status"], "blocked");
    assert_eq!(check_by_id(&output, "agent_skills")["status"], "blocked");
    assert_eq!(check_by_id(&output, "proxy")["status"], "blocked");
    assert_eq!(check_by_id(&output, "vault")["status"], "blocked");
    for id in ["contract", "required_tools", "agent_skills", "proxy"] {
        assert!(
            check_by_id(&output, id)["detail"]
                .as_str()
                .unwrap()
                .contains(".jig.toml")
        );
    }
    assert!(
        check_by_id(&output, "vault")["detail"]
            .as_str()
            .unwrap()
            .contains("repo context")
    );
    assert!(output["next_step"].as_str().unwrap().contains(".jig.toml"));
    assert!(
        output["next_required_step"]
            .as_str()
            .unwrap()
            .contains(".jig.toml")
    );
    assert!(output["optional_setup"].is_null());
    let summary = format_summary(&output);
    assert!(summary.contains("Next required step: Fix `.jig.toml`"));
    assert!(summary.contains("Optional setup: none"));
}

#[test]
fn doctor_uses_configured_repo_root_before_current_directory() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let other = temp.path().join("other");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&other).unwrap();
    write_doctor_fixture(&repo);
    let _repo_root = EnvVarGuard::set("JIG_REPO_ROOT", &repo);
    let _cwd = CurrentDirGuard::set(&other);

    let output = run().unwrap();

    assert_eq!(
        output["repo"]["root"],
        fs::canonicalize(&repo).unwrap().display().to_string()
    );
    assert_eq!(output["repo"]["name"], "demo");
}

#[test]
fn doctor_reports_invalid_configured_repo_root() {
    let _env = lock_env();
    let temp = tempdir().unwrap();
    let missing_config = temp.path().join("missing-config");
    fs::create_dir_all(&missing_config).unwrap();
    let _repo_root = EnvVarGuard::set("JIG_REPO_ROOT", &missing_config);

    let output = run().unwrap();

    assert_eq!(output["ok"], false);
    assert_eq!(check_by_id(&output, "repo")["status"], "missing");
    assert!(
        check_by_id(&output, "repo")["detail"]
            .as_str()
            .unwrap()
            .contains("JIG_REPO_ROOT does not contain .jig.toml")
    );
    assert!(
            check_by_id(&output, "repo")["fix"]
                .as_str()
                .unwrap()
                .contains("init <path> --preset harness-only --repo-name <name> --sqlx-enabled false --no-input --no-vault")
        );
}

#[cfg(unix)]
fn cargo_sqlx_program(check: &DoctorCheck) -> &Value {
    check.data["tools"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|tool| tool["programs"].as_array().unwrap())
        .find(|program| program.get("driver_probe").is_some())
        .unwrap()
}

#[cfg(unix)]
fn doctor_environment(bin: &Path, database_url: Option<&str>) -> DoctorEnvironment {
    let bin = fs::canonicalize(bin).unwrap_or_else(|_| bin.to_path_buf());
    DoctorEnvironment {
        search_path: Some(bin.into_os_string()),
        path_extensions: None,
        database_url: database_url.map(OsString::from),
        cargo_alias_sqlx: None,
        cargo_home: None,
        home: None,
        probe_environment: Vec::new(),
        shell_environment_issue: None,
    }
}

#[cfg(unix)]
fn write_test_executable(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn write_sqlx_doctor_fixture_with_command(root: &Path, command: &str) {
    write_doctor_fixture(root);
    let config_path = root.join(".jig.toml");
    let sqlx_config = format!(
        "sqlx_enabled = true\nrust_crate_roots = [\"crates\"]\nrust_migration_dir = \"migrations\"\nrust_sqlx_metadata_dir = \".sqlx\"\nschema_dump_enabled = false\nsqlx_check_command = {command:?}\n\n[agent_tooling.codex]"
    );
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace("[agent_tooling.codex]", &sqlx_config);
    fs::write(config_path, config).unwrap();
    fs::create_dir(root.join("migrations")).unwrap();

    let contract_path = root.join(".agent/jig-contract.json");
    let mut contract: Value =
        serde_json::from_str(&fs::read_to_string(&contract_path).unwrap()).unwrap();
    contract["required_commands"]
        .as_array_mut()
        .unwrap()
        .push(json!("sqlx_check_command"));
    let tools = contract["tools"].as_array_mut().unwrap();
    tools.push(json!({
        "name": tool::SQLX_CHECK,
        "kind": "command",
        "description": "Run the configured SQLx check command.",
        "command": "sqlx_check_command",
    }));
    tools.push(json!({
        "name": tool::MIGRATION_ADD,
        "kind": "native",
        "description": "Add timestamped SQL migration stubs.",
    }));
    fs::write(
        contract_path,
        serde_json::to_string_pretty(&contract).unwrap(),
    )
    .unwrap();
}

fn write_doctor_fixture_with_bootstrap_command(root: &Path, command: &str) {
    write_doctor_fixture(root);
    let config_path = root.join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "bootstrap_command = \"printf bootstrap\"",
        &format!("bootstrap_command = {command:?}"),
    );
    fs::write(config_path, config).unwrap();
}

fn check_by_id<'a>(output: &'a Value, id: &str) -> &'a Value {
    output["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == id)
        .unwrap()
}

fn write_doctor_fixture(root: &Path) {
    fs::create_dir_all(root.join("scripts")).unwrap();
    TestRepoBuilder::new(root)
        .jig_version(env!("CARGO_PKG_VERSION"))
        .contract_version(crate::context::CURRENT_CONTRACT_VERSION)
        .config(
            r#"
bootstrap_command = "printf bootstrap"

[agent_tooling.codex]
marketplaces = []
"#,
        )
        .required_commands(["bootstrap_command"])
        .tool(json!({
            "name": tool::CONTRACT_CHECK,
            "kind": "native",
            "description": "Contract check."
        }))
        .tool(json!({
            "name": tool::BOOTSTRAP,
            "kind": "command",
            "description": "Bootstrap.",
            "command": "bootstrap_command"
        }))
        .write();
    fs::write(root.join(".mcp.json"), "{}").unwrap();
    fs::write(
        root.join("scripts/install-jig.sh"),
        CURRENT_GENERATED_INSTALLER,
    )
    .unwrap();
    fs::write(root.join("scripts/jig"), current_generated_launcher()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root.join("scripts/jig"), fs::Permissions::from_mode(0o755)).unwrap();
    }
}

mod root;
mod runtime;
