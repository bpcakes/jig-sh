use std::ffi::OsStr;
use std::fs;

use tempfile::tempdir;

use crate::doctor::database_url::{
    DotenvDatabaseUrl, database_url_from_dotenv, dotenv_database_url_key,
};
use crate::doctor::shell_analysis::{ShellSeparator, parse_shell_commands, resolve_literal_cd};
use crate::doctor::sqlx_driver::{
    SqlxDriver, SqlxDriverRequirement, SqlxDriverResolution, SqlxDriverSource,
    configured_sqlx_driver, configured_sqlx_driver_fallback,
};
use crate::test_env::{EnvVarGuard, lock_env};

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
