use std::{fs, process::Command};

use tempfile::tempdir;

use super::*;
use crate::bootstrap::clippy_policy::{
    DEFAULT_RUST_CLIPPY_COMMAND, NESTED_MANIFEST_RUST_CLIPPY_COMMAND,
};
use crate::bootstrap::{ScaffoldDb, ScaffoldOpts, ScaffoldPreset, scaffold};

#[test]
fn common_answer_resolution_preserves_authored_repository_authority() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("answers.toml");
    fs::write(
        &path,
        r#"repo_name = "ExampleProject"
sqlx_enabled = false
schema_dump_enabled = false

[commands]
api_verify_command = "go test ./..."
worker_verify_command = "cargo test -p worker"

[repository]
default_check_profile = "verify"

[[repository.components]]
id = "api"
root = "services/api"
adapters = ["go"]

[[repository.components]]
id = "worker"
root = "services/worker"
adapters = ["rust"]

[[repository.actions]]
target = { component = "api", action = "verify-custom" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "command", command = "api_verify_command" }

[[repository.actions]]
target = { component = "worker", action = "verify-custom" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "command", command = "worker_verify_command" }

[[repository.profiles]]
id = "verify"
targets = [
  { component = "api", action = "verify-custom" },
  { component = "worker", action = "verify-custom" },
]
"#,
    )
    .unwrap();
    let input = AnswerInput::from_file(&path).unwrap();
    let effective = input.effective_opts(&AnswerOpts::default()).unwrap();

    assert_eq!(effective.scaffold_go_component_roots, ["services/api"]);

    let (resolved, notes) =
        AnswerResolution::from_input(input, &AnswerOpts::default(), temp.path(), false)
            .unwrap()
            .into_parts();
    let model =
        crate::bootstrap::repository_model::RepositoryRenderModel::from_answers(&resolved).unwrap();

    assert!(notes.is_empty());
    assert_eq!(
        model
            .components
            .iter()
            .map(|component| component.id.as_str())
            .collect::<Vec<_>>(),
        ["api", "worker"]
    );
    assert_eq!(
        model
            .actions
            .iter()
            .map(|action| action.target.to_string())
            .collect::<Vec<_>>(),
        ["api:verify-custom", "worker:verify-custom"]
    );
    assert_eq!(
        model.required_commands,
        ["api_verify_command", "worker_verify_command"]
    );
    let commands = model.commands_toml().unwrap();
    assert!(commands.contains("api_verify_command = \"go test ./...\""));
    assert!(commands.contains("worker_verify_command = \"cargo test -p worker\""));
}

#[test]
fn go_scaffold_rejects_multiple_authored_go_component_roots() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("answers.toml");
    fs::write(
        &path,
        r#"repo_name = "ExampleProject"

[commands]
api_test_command = "go test ./..."
worker_test_command = "go test ./..."

[repository]
default_check_profile = "verify"

[[repository.components]]
id = "api"
root = "services/api"
adapters = ["go"]

[[repository.components]]
id = "worker"
root = "services/worker"
adapters = ["go"]

[[repository.actions]]
target = { component = "api", action = "test" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "command", command = "api_test_command" }

[[repository.actions]]
target = { component = "worker", action = "test" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "command", command = "worker_test_command" }

[[repository.profiles]]
id = "verify"
targets = [
  { component = "api", action = "test" },
  { component = "worker", action = "test" },
]
"#,
    )
    .unwrap();
    let input = AnswerInput::from_file(&path).unwrap();

    let effective = input.effective_opts(&AnswerOpts::default()).unwrap();
    assert_eq!(
        effective.scaffold_go_component_roots,
        ["services/api", "services/worker"]
    );
    let error = scaffold::InitScaffoldPlan::from_opts(
        &ScaffoldOpts {
            preset: Some(ScaffoldPreset::GoReact),
            db: Some(ScaffoldDb::None),
            ..ScaffoldOpts::default()
        },
        &AnswerOpts {
            go_module: Some("example.com/ExampleProject".into()),
            ..effective
        },
        temp.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("multiple Go component roots"), "{error}");
    assert!(error.contains("services/api"), "{error}");
    assert!(error.contains("services/worker"), "{error}");
}

#[test]
fn authored_repository_command_maps_fail_closed_before_resolution() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("answers.toml");
    fs::write(
        &path,
        r#"repo_name = "ExampleProject"

[commands]
custom_check_command = 7

[repository]
default_check_profile = "verify"

[[repository.components]]
id = "custom"
root = "."

[[repository.actions]]
target = { component = "custom", action = "check" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "command", command = "custom_check_command" }

[[repository.profiles]]
id = "verify"
targets = [{ component = "custom", action = "check" }]
"#,
    )
    .unwrap();

    let error = AnswerInput::from_opts(&AnswerOpts {
        answers_file: Some(path),
        ..AnswerOpts::default()
    })
    .err()
    .unwrap()
    .to_string();

    assert!(
        error.contains(
            "complete authored [repository] model requires [commands] to be a table of string values"
        ),
        "{error}"
    );
}

#[test]
fn migration_dir_survives_effective_options_and_render_serialization() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("answers.toml");
    fs::write(
        &path,
        r#"repo_name = "ExampleProject"
backend_language = "go"
go_database = "postgres"
migration_dir = "database/migrations"
sqlx_enabled = false
schema_dump_enabled = false
"#,
    )
    .unwrap();

    let input = AnswerInput::from_file(&path).unwrap();
    let effective = input.effective_opts(&AnswerOpts::default()).unwrap();
    assert_eq!(
        effective.migration_dir.as_deref(),
        Some("database/migrations")
    );

    let rendered = RenderAnswers::from_answers_file(&path).unwrap();
    let value = serde_json::to_value(rendered).unwrap();
    assert_eq!(value["migration_dir"], "database/migrations");
}

#[test]
fn divergent_sqlx_migration_answers_are_rejected_before_rendering() {
    let error = RawAnswers {
        repo_name: Some("ExampleProject".into()),
        sqlx_enabled: Some(true),
        migration_dir: Some("database/migrations".into()),
        rust_migration_dir: Some("legacy/migrations".into()),
        schema_dump_enabled: Some(false),
        ..RawAnswers::default()
    }
    .resolve(None)
    .unwrap_err()
    .to_string();

    assert!(error.contains("migration_dir"), "{error}");
    assert!(error.contains("rust_migration_dir"), "{error}");
    assert!(
        error.contains("must identify the same SQLx migration directory"),
        "{error}"
    );
}

#[test]
fn canonical_sqlx_migration_answer_drives_the_legacy_render_value() {
    let rendered = RawAnswers {
        repo_name: Some("ExampleProject".into()),
        sqlx_enabled: Some(true),
        migration_dir: Some("database/migrations".into()),
        rust_migration_dir: None,
        schema_dump_enabled: Some(false),
        ..RawAnswers::default()
    }
    .resolve(None)
    .unwrap();
    let value = serde_json::to_value(rendered).unwrap();

    assert_eq!(value["migration_dir"], "database/migrations");
    assert_eq!(value["rust_migration_dir"], "database/migrations");
}

#[test]
fn canonical_migration_answer_prevents_a_tooling_only_sqlx_default() {
    let answers = AnswerOpts {
        migration_dir: Some("database/migrations".into()),
        ..AnswerOpts::default()
    };

    assert!(!should_default_init_sqlx_disabled(&answers));
}

#[test]
fn go_backend_rejects_rust_sqlx_answers() {
    let error = RawAnswers {
        repo_name: Some("ExampleProject".into()),
        backend_language: Some(BackendLanguage::Go),
        go_database: Some(GoDatabase::Postgres),
        sqlx_enabled: Some(true),
        rust_migration_dir: Some("migrations".into()),
        ..RawAnswers::default()
    }
    .resolve(None)
    .unwrap_err()
    .to_string();

    assert_eq!(
        error,
        "backend_language = \"go\" cannot be combined with sqlx_enabled = true; Go repositories use --go-database and Goose/sqlc, while SQLx is owned by the Rust backend"
    );
}

#[cfg(unix)]
#[test]
fn generated_go_format_check_propagates_parser_failures_and_ignores_ignored_files() {
    use std::os::unix::fs::PermissionsExt;

    let rendered = RawAnswers {
        repo_name: Some("demo".into()),
        backend_language: Some(BackendLanguage::Go),
        go_database: Some(GoDatabase::None),
        sqlx_enabled: Some(false),
        schema_dump_enabled: Some(false),
        ..RawAnswers::default()
    }
    .resolve(None)
    .unwrap();

    assert!(rendered.go_fmt_check_command.contains("set -o pipefail"));
    assert!(rendered.go_fmt_check_command.contains("git ls-files"));
    assert!(rendered.go_fmt_check_command.contains("--exclude-standard"));
    assert!(rendered.go_fmt_check_command.contains("[ -f \"$file\" ]"));
    assert!(
        rendered
            .go_fmt_check_command
            .contains("xargs -0 gofmt -l --")
    );
    assert!(rendered.go_fmt_check_command.contains(") || exit $?"));

    let temp = tempdir().unwrap();
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let gofmt = bin.join("gofmt");
    fs::write(
        &gofmt,
        r#"#!/usr/bin/env bash
set -euo pipefail
test "$1" = "-l"
test "$2" = "--"
shift 2
test "$#" -eq 1
test "$1" = "current.go"
exit "${FAKE_GOFMT_EXIT:-0}"
"#,
    )
    .unwrap();
    fs::set_permissions(&gofmt, fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(
        std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();

    fs::write(temp.path().join("current.go"), "package example\n").unwrap();
    fs::write(temp.path().join("deleted.go"), "package example\n").unwrap();
    fs::write(temp.path().join("ignored.go"), "not valid Go\n").unwrap();
    fs::write(temp.path().join(".gitignore"), "ignored.go\n").unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(temp.path())
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("git")
            .args(["add", "current.go", "deleted.go", ".gitignore"])
            .current_dir(temp.path())
            .status()
            .unwrap()
            .success()
    );
    fs::remove_file(temp.path().join("deleted.go")).unwrap();

    let output = Command::new("bash")
        .args(["-c", &rendered.go_fmt_check_command])
        .env("PATH", &path)
        .current_dir(temp.path())
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");

    let parser_failure = Command::new("bash")
        .args(["-c", &rendered.go_fmt_check_command])
        .env("PATH", path)
        .env("FAKE_GOFMT_EXIT", "7")
        .current_dir(temp.path())
        .output()
        .unwrap();
    assert!(!parser_failure.status.success(), "{parser_failure:?}");
}

#[test]
fn recopy_normalizes_exact_former_generated_sqlx_default() {
    let metadata_dir = "db/sqlx metadata";
    let former_default = format!(
        "SQLX_OFFLINE=false SQLX_OFFLINE_DIR={} cargo sqlx prepare --check --workspace -- --workspace --all-targets",
        shell_quote(metadata_dir)
    );
    let mut raw = RawAnswers {
        repo_name: Some("demo".into()),
        sqlx_enabled: Some(true),
        rust_migration_dir: Some("migrations".into()),
        rust_sqlx_metadata_dir: Some(metadata_dir.into()),
        schema_dump_enabled: Some(false),
        sqlx_check_command: Some(former_default.clone()),
        ..RawAnswers::default()
    };

    raw.normalize_legacy_generated_cargo_command_defaults();
    assert_eq!(raw.sqlx_check_command, None);
    let rendered = raw.resolve(None).unwrap();
    assert_eq!(
        rendered.sqlx_check_command,
        format!(
            "CARGO=cargo SQLX_OFFLINE=false SQLX_OFFLINE_DIR={} sqlx prepare --check --workspace -- --workspace --all-targets",
            shell_quote(metadata_dir)
        )
    );

    let mut customized = RawAnswers {
        rust_sqlx_metadata_dir: Some(metadata_dir.into()),
        sqlx_check_command: Some(format!("{former_default} --custom")),
        ..RawAnswers::default()
    };
    customized.normalize_legacy_generated_cargo_command_defaults();
    assert_eq!(
        customized.sqlx_check_command.as_deref(),
        Some(format!("{former_default} --custom").as_str())
    );
}

#[test]
fn recopy_upgrades_only_exact_generated_rust_clippy_defaults() {
    let former_default = "cargo clippy --workspace --all-targets --locked -- -D warnings";
    let pre_all_features_default = concat!(
        "cargo clippy --workspace --all-targets --locked -- ",
        "-D warnings -D clippy::mod_module_files"
    );
    let current_default = concat!(
        "cargo clippy --workspace --all-targets --all-features --locked -- ",
        "-D warnings -D clippy::mod_module_files"
    );
    let mut raw = RawAnswers {
        repo_name: Some("ExampleProject".into()),
        sqlx_enabled: Some(false),
        schema_dump_enabled: Some(false),
        rust_clippy_command: Some(former_default.into()),
        ..RawAnswers::default()
    };

    raw.normalize_legacy_generated_cargo_command_defaults();
    assert_eq!(raw.rust_clippy_command, None);
    assert!(
        raw.resolve(None)
            .unwrap()
            .rust_clippy_command
            .contains(current_default)
    );

    let mut opted_out = RawAnswers {
        rust_clippy_command: Some(pre_all_features_default.into()),
        ..RawAnswers::default()
    };
    opted_out.normalize_legacy_generated_cargo_command_defaults();
    assert_eq!(
        opted_out.rust_clippy_command.as_deref(),
        Some(pre_all_features_default)
    );

    let custom = format!("{former_default} --custom");
    let mut customized = RawAnswers {
        rust_clippy_command: Some(custom.clone()),
        ..RawAnswers::default()
    };
    customized.normalize_legacy_generated_cargo_command_defaults();
    assert_eq!(
        customized.rust_clippy_command.as_deref(),
        Some(custom.as_str())
    );

    let mut commands = Some(BTreeMap::from([(
        "api_clippy_command".into(),
        former_default.into(),
    )]));
    let migrations = normalize_generated_clippy_defaults(&mut RawAnswers::default(), &mut commands);
    assert_eq!(
        commands.unwrap()["api_clippy_command"],
        DEFAULT_RUST_CLIPPY_COMMAND
    );
    let warnings = migrations.warnings();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("commands.api_clippy_command"));
    assert!(warnings[0].contains("--all-features"));

    let mut opted_out_commands = Some(BTreeMap::from([(
        "api_clippy_command".into(),
        pre_all_features_default.into(),
    )]));
    let migrations =
        normalize_generated_clippy_defaults(&mut RawAnswers::default(), &mut opted_out_commands);
    assert_eq!(
        opted_out_commands.unwrap()["api_clippy_command"],
        pre_all_features_default
    );
    assert!(migrations.warnings().is_empty());

    let legacy_optional = optional_cargo_command(former_default, "clippy");
    let mut optional_commands = Some(BTreeMap::from([(
        "api_clippy_command".into(),
        legacy_optional,
    )]));
    normalize_generated_clippy_defaults(&mut RawAnswers::default(), &mut optional_commands);
    assert_eq!(
        optional_commands.unwrap()["api_clippy_command"],
        optional_cargo_command(DEFAULT_RUST_CLIPPY_COMMAND, "clippy")
    );

    let mut optional_raw = RawAnswers {
        rust_clippy_command: Some(optional_cargo_command(former_default, "clippy")),
        ..RawAnswers::default()
    };
    optional_raw.normalize_legacy_generated_cargo_command_defaults();
    assert_eq!(optional_raw.rust_clippy_command, None);

    let legacy_nested_cargo =
        "cargo clippy --manifest-path \"$jig_manifest\" --all-targets -- -D warnings";
    let legacy_nested = format!(
        "( found=0; rc=0; jig_manifest=api/Cargo.toml; if [ -f \"$jig_manifest\" ]; then found=1; {legacy_nested_cargo} || rc=$?; fi; if [ \"$found\" -eq 0 ]; then printf '%s\\n' 'No Cargo.toml found; skipping cargo clippy.'; fi; exit \"$rc\" )"
    );
    let current_nested =
        legacy_nested.replace(legacy_nested_cargo, NESTED_MANIFEST_RUST_CLIPPY_COMMAND);
    let mut nested_raw = RawAnswers {
        rust_clippy_command: Some(legacy_nested.clone()),
        ..RawAnswers::default()
    };
    nested_raw.normalize_legacy_generated_cargo_command_defaults();
    assert_eq!(
        nested_raw.rust_clippy_command.as_deref(),
        Some(current_nested.as_str())
    );

    let mut nested_commands = Some(BTreeMap::from([(
        "api_clippy_command".into(),
        legacy_nested,
    )]));
    normalize_generated_clippy_defaults(&mut RawAnswers::default(), &mut nested_commands);
    assert_eq!(
        nested_commands.unwrap()["api_clippy_command"],
        current_nested
    );

    let opted_out_nested = current_nested.replace("--all-features ", "");
    let mut opted_out_nested_commands = Some(BTreeMap::from([(
        "api_clippy_command".into(),
        opted_out_nested.clone(),
    )]));
    let migrations = normalize_generated_clippy_defaults(
        &mut RawAnswers::default(),
        &mut opted_out_nested_commands,
    );
    assert_eq!(
        opted_out_nested_commands.unwrap()["api_clippy_command"],
        opted_out_nested
    );
    assert!(migrations.warnings().is_empty());
}

#[test]
fn file_input_reports_only_effective_custom_clippy_commands_missing_the_policy_lint() {
    let former_default = "cargo clippy --workspace --all-targets --locked -- -D warnings";
    let pre_all_features_default = concat!(
        "cargo clippy --workspace --all-targets --locked -- ",
        "-D warnings -D clippy::mod_module_files"
    );
    let custom = format!("{former_default} --custom");
    let mut custom_commands = Some(BTreeMap::from([(
        "api_clippy_command".into(),
        custom.clone(),
    )]));
    let migrations =
        normalize_generated_clippy_defaults(&mut RawAnswers::default(), &mut custom_commands);
    assert_eq!(custom_commands.unwrap()["api_clippy_command"], custom);
    let warnings = migrations.warnings();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("Could not verify `clippy::mod_module_files`"));
    assert!(warnings[0].contains("commands.api_clippy_command"));

    let custom_with_lint = format!("{pre_all_features_default} --custom");
    let mut custom_with_lint_commands = Some(BTreeMap::from([(
        "api_clippy_command".into(),
        custom_with_lint.clone(),
    )]));
    let migrations = normalize_generated_clippy_defaults(
        &mut RawAnswers::default(),
        &mut custom_with_lint_commands,
    );
    assert_eq!(
        custom_with_lint_commands.unwrap()["api_clippy_command"],
        custom_with_lint
    );
    assert!(migrations.warnings().is_empty());

    let unrelated_legacy = optional_cargo_command(former_default, "clippy");
    let mut unrelated_commands = Some(BTreeMap::from([(
        "unrelated_command".into(),
        unrelated_legacy.clone(),
    )]));
    let migrations =
        normalize_generated_clippy_defaults(&mut RawAnswers::default(), &mut unrelated_commands);
    assert_eq!(
        unrelated_commands.unwrap()["unrelated_command"],
        unrelated_legacy
    );
    assert!(migrations.warnings().is_empty());
}

#[test]
fn file_input_does_not_confuse_lint_mentions_with_policy_enforcement() {
    let legacy_nested_cargo =
        "cargo clippy --manifest-path \"$jig_manifest\" --all-targets -- -D warnings";
    let mixed_nested = format!(
        "( found=0; rc=0; jig_manifest=api/Cargo.toml; if [ -f \"$jig_manifest\" ]; then found=1; {legacy_nested_cargo} || rc=$?; fi; jig_manifest=worker/Cargo.toml; if [ -f \"$jig_manifest\" ]; then found=1; {NESTED_MANIFEST_RUST_CLIPPY_COMMAND} || rc=$?; fi; if [ \"$found\" -eq 0 ]; then printf '%s\\n' 'No Cargo.toml found; skipping cargo clippy.'; fi; exit \"$rc\" )"
    );
    for command in [
        "cargo clippy --workspace -- -A clippy::mod_module_files".into(),
        "cargo clippy --workspace -- -D clippy::mod_module_files -A clippy::mod_module_files"
            .into(),
        "cargo clippy --workspace -- -D clippy::mod_module_files $EXTRA_FLAGS".into(),
        "printf '%s' clippy::mod_module_files".into(),
        mixed_nested,
    ] {
        let mut commands = Some(BTreeMap::from([(
            "api_clippy_command".into(),
            command.clone(),
        )]));
        let diagnostics =
            normalize_generated_clippy_defaults(&mut RawAnswers::default(), &mut commands);

        assert_eq!(commands.unwrap()["api_clippy_command"], command);
        let warnings = diagnostics.warnings();
        assert_eq!(warnings.len(), 1, "{command}");
        assert!(
            warnings[0].contains("Could not verify `clippy::mod_module_files`"),
            "{command}"
        );
    }
}

#[test]
fn file_input_checks_every_literal_or_aliased_clippy_action() {
    let mut raw = toml::from_str::<RawAnswers>(
        r#"
[repository]
default_check_profile = "verify"

[[repository.actions]]
target = { component = "api", action = "lint-rust" }
intent = "check"
effects = ["read_only", "process"]
legacy_aliases = ["jig.clippy"]
runner = { kind = "command", command = "primary_clippy_command" }

[[repository.actions]]
target = { component = "worker", action = "clippy" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "command", command = "worker_clippy_command" }
"#,
    )
    .unwrap();
    let legacy = "cargo clippy --workspace --all-targets --locked -- -D warnings";
    let custom = "cargo clippy --workspace -- -A clippy::mod_module_files";
    let mut commands = Some(BTreeMap::from([
        ("primary_clippy_command".into(), legacy.into()),
        ("worker_clippy_command".into(), custom.into()),
        ("unrelated_command".into(), legacy.into()),
    ]));

    let diagnostics = normalize_generated_clippy_defaults(&mut raw, &mut commands);

    let commands = commands.unwrap();
    assert_eq!(
        commands["primary_clippy_command"],
        DEFAULT_RUST_CLIPPY_COMMAND
    );
    assert_eq!(commands["worker_clippy_command"], custom);
    assert_eq!(commands["unrelated_command"], legacy);
    let warnings = diagnostics.warnings();
    assert_eq!(warnings.len(), 2);
    assert!(warnings[0].contains("commands.primary_clippy_command"));
    assert!(warnings[1].contains("commands.worker_clippy_command"));
}
