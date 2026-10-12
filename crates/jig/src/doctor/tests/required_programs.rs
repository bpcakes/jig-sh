use std::ffi::{OsStr, OsString};

use crate::doctor::programs::command_programs_for_shell;
use crate::doctor::programs::{
    ProgramPathLookup, RequiredProgramAmbiguity, required_command_programs_for_shell,
    search_path_is_cwd_independent,
};
use crate::doctor::shell_analysis::{bash_builtin, bash_keyword};

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
    for command in ["env -C sub /usr/bin/tool", "cd sub && /usr/bin/tool"] {
        let discovery = required_command_programs_for_shell(command);
        assert_eq!(
            discovery.programs.last().unwrap().path_lookup,
            ProgramPathLookup::Explicit,
            "{command:?}",
        );
    }
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
