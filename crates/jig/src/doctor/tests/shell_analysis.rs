use std::ffi::OsString;
use std::path::Path;

use jig_context::RepoContext;
use tempfile::tempdir;

use super::support::write_sqlx_doctor_fixture_with_command;
use crate::doctor::environment::DoctorEnvironment;
use crate::doctor::programs::command_programs_for_shell;
use crate::doctor::programs::{
    ProgramPathLookup, RequiredProgramAmbiguity, required_command_programs_for_shell,
};
use crate::doctor::required_tools::required_tools_check_with_environment;
use crate::doctor::sqlx_driver::{
    SqlxDriver, SqlxDriverRequirement, SqlxDriverResolution, SqlxDriverSource,
    configured_sqlx_driver,
};

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
fn assert_inert_heredoc(command: &str) {
    let discovery = required_command_programs_for_shell(command);
    assert!(discovery.ambiguity.is_none(), "{command:?}");
    assert_eq!(discovery.programs[0].program, "cat", "{command:?}");
    assert_eq!(
        discovery.programs[0].path_lookup,
        ProgramPathLookup::Captured,
        "{command:?}",
    );
}
fn assert_heredoc_selects_sqlite(root: &Path, command: &str, programs: &[&str]) {
    assert_eq!(command_programs_for_shell(command), programs);
    assert_eq!(
        configured_sqlx_driver(root, command, None),
        SqlxDriverResolution::Known(SqlxDriverRequirement {
            driver: SqlxDriver::Sqlite,
            source: SqlxDriverSource::CommandFlag,
        })
    );
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
        assert_inert_heredoc(inert);
    }

    let command = "cat <<'PAYLOAD'\nDATABASE_URL=postgres://body-secret cargo sqlx prepare -D postgres://body-secret\nPAYLOAD\ncargo sqlx prepare -D sqlite:actual.db";

    assert_heredoc_selects_sqlite(temp.path(), command, &["cat", "cargo"]);

    let tab_stripped = "cat <<-EOF\n\tcargo sqlx prepare -D postgres://ignored\n\tEOF\nsqlx prepare -D sqlite:actual.db";
    assert_heredoc_selects_sqlite(temp.path(), tab_stripped, &["cat", "sqlx"]);

    for tab_stripped in [
        "cat <<- EOF\n\tcargo sqlx prepare -D postgres://ignored\n\tEOF\nsqlx prepare -D sqlite:actual.db",
        "cat <<-EOF\n\tcargo sqlx prepare -D postgres://ignored\n\tEOF\nsqlx prepare -D sqlite:actual.db",
    ] {
        assert_heredoc_selects_sqlite(temp.path(), tab_stripped, &["cat", "sqlx"]);
    }

    let multiple_crlf = "cat <<ONE <<-'TWO'\r\ncargo sqlx prepare -D postgres://first\r\nONE\r\n\tcargo sqlx prepare -D postgres://second\r\n\tTWO\r\nsqlx prepare -D sqlite:actual.db";
    assert_heredoc_selects_sqlite(temp.path(), multiple_crlf, &["cat", "sqlx"]);

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
