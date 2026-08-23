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
fn dotenv_database_url_key_matching_is_case_sensitive() {
    assert!(dotenv_database_url_key("DATABASE_URL"));
    assert!(!dotenv_database_url_key("database_url"));

    let temp = tempdir().unwrap();
    fs::write(
        temp.path().join(".env"),
        "database_url=sqlite:first.db\nDATABASE_URL=postgres://localhost/second\n",
    )
    .unwrap();
    let database_url = database_url_from_dotenv(&temp.path().join(".env")).unwrap();
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
