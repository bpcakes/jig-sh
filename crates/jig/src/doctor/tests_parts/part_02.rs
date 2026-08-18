
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
