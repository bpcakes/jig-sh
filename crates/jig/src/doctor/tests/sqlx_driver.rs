use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use tempfile::tempdir;

use crate::doctor::sqlx_driver::{
    SqlxDriver, SqlxDriverRequirement, SqlxDriverResolution, SqlxDriverSource,
    configured_sqlx_driver,
};

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
fn assert_driver_indeterminate(root: &Path, command: &str, ambient: Option<&OsStr>) {
    assert!(
        matches!(
            configured_sqlx_driver(root, command, ambient),
            SqlxDriverResolution::Indeterminate(_)
        ),
        "{command:?}"
    );
}
fn assert_sqlite_driver(
    root: &Path,
    command: &str,
    ambient: Option<&OsStr>,
    source: SqlxDriverSource,
) {
    assert_eq!(
        configured_sqlx_driver(root, command, ambient),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Sqlite,
            source,
        }),
        "{command:?}"
    );
}
#[test]
fn sqlx_driver_discovery_fails_open_for_dynamic_and_ambiguous_commands() {
    let temp = tempdir().unwrap();
    let ambient = Some(OsStr::new("sqlite:environment.db"));

    for command in [
        "cargo sqlx prepare --database-url '$OTHER_DATABASE_URL'",
        "cargo sqlx prepare --database-url '$DATABASE_URL'",
        r"cargo sqlx prepare --database-url \$DATABASE_URL",
    ] {
        assert_driver_indeterminate(temp.path(), command, ambient);
    }
    assert_sqlite_driver(
        temp.path(),
        "cargo sqlx prepare --database-url \"$DATABASE_URL\"",
        ambient,
        SqlxDriverSource::CommandFlag,
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
    assert_sqlite_driver(
        temp.path(),
        "DATABASE_URL=$DATABASE_URL cargo sqlx prepare",
        ambient,
        SqlxDriverSource::CommandAssignment,
    );
    fs::write(
        temp.path().join(".env"),
        "DATABASE_URL=postgres://dotenv-must-not-be-used/doctor\n",
    )
    .unwrap();
    for command in [
        "cargo sqlx prepare --database-url '$DATABASE_URL'",
        "DATABASE_URL=sqlite:first.db cargo sqlx prepare && cargo sqlx migrate info --database-url=postgres://localhost/second",
        "export DATABASE_URL=postgres://localhost/demo && cargo sqlx prepare",
        "DATABASE_URL=postgres://localhost/demo && cargo sqlx prepare",
        "cargo sqlx prepare --database-url='postgres://localhost/demo",
    ] {
        assert_driver_indeterminate(temp.path(), command, None);
    }
    for command in [
        "env -u DATABASE_URL cargo sqlx prepare",
        "env - cargo sqlx prepare",
    ] {
        assert_driver_indeterminate(temp.path(), command, ambient);
    }
    for command in [
        "DATABASE_URL=sqlite:first.db cargo sqlx prepare && printf ignored && cargo sqlx migrate info --database-url=sqlite:second.db",
        "printf DATABASE_URL=postgres://ignored && DATABASE_URL=sqlite:actual.db cargo sqlx prepare",
    ] {
        assert_sqlite_driver(
            temp.path(),
            command,
            None,
            SqlxDriverSource::CommandAssignment,
        );
    }
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
