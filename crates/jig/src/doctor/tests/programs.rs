use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use tempfile::tempdir;

use super::support::write_test_executable;
use crate::doctor::cargo_sqlx::cargo_sqlx_command_has_inline_config;
use crate::doctor::programs::{
    ProgramOrigin, RequiredProgramAmbiguity, required_command_programs_for_shell, resolve_program,
};
use crate::doctor::programs::{command_program, command_programs, command_programs_for_shell};
use crate::doctor::shell_analysis::executable_basename;
use crate::test_env::{CurrentDirGuard, lock_env};

#[test]
fn program_resolution_distinguishes_unset_and_explicitly_empty_path() {
    let _env = lock_env();
    let repo = tempdir().unwrap();
    let invocation = tempdir().unwrap();
    let _cwd = CurrentDirGuard::set(invocation.path());
    let program = "doctor-empty-path-tool";
    #[cfg(unix)]
    write_test_executable(&repo.path().join(program), "#!/bin/sh\nexit 0\n");
    #[cfg(not(unix))]
    fs::write(repo.path().join(program), "executable\n").unwrap();
    assert_eq!(resolve_program(repo.path(), program, None), None);
    let resolution = resolve_program(repo.path(), program, Some(OsStr::new(""))).unwrap();
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
fn executable_basename_preserves_utf8_names() {
    assert_eq!(executable_basename("💩a"), Some("💩a"));
    assert_eq!(executable_basename("💩.tool"), Some("💩.tool"));
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
        jig_repository::shell::OPTIONAL_CARGO_COMMAND_PREFIX,
        jig_repository::shell::OPTIONAL_CARGO_COMMAND_ELSE,
        jig_repository::shell::OPTIONAL_CARGO_COMMAND_SUFFIX,
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
