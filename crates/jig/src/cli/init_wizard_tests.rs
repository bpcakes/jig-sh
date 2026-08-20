use std::{fs, io::Cursor};

use clap::Parser;
use tempfile::tempdir;

use super::*;
use crate::cli::{Cli, CommandKind};

fn init_opts(args: &[&str]) -> InitOpts {
    match Cli::try_parse_from(args).unwrap().command {
        CommandKind::Init(opts) => opts,
        other => panic!("expected init command, got {other:?}"),
    }
}

#[test]
fn bare_init_guides_preset_database_and_multiple_frontends() {
    let mut opts = init_opts(&["jig", "init", "demo", "--no-vault"]);
    let mut input = Cursor::new("rust-react\npostgres\nweb,admin\n");
    let mut output = Vec::new();

    prepare_init_interaction_with_io(&mut opts, &mut input, &mut output).unwrap();

    assert_eq!(opts.scaffold.preset, Some(ScaffoldPreset::RustReact));
    assert_eq!(opts.scaffold.db, Some(ScaffoldDb::Postgres));
    assert_eq!(opts.scaffold.frontends.len(), 2);
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("Rust API workspace plus shadcn React"));
    assert!(output.contains("shadcn Vite React admin app in admin-panel/"));
}

#[test]
fn bare_init_defaults_to_rust_react_with_no_database_and_web() {
    let mut opts = init_opts(&["jig", "init", "demo", "--no-vault"]);
    let mut input = Cursor::new("\n\n\n");
    let mut output = Vec::new();

    prepare_init_interaction_with_io(&mut opts, &mut input, &mut output).unwrap();

    assert_eq!(opts.scaffold.preset, Some(ScaffoldPreset::RustReact));
    assert_eq!(opts.scaffold.db, Some(ScaffoldDb::None));
    assert_eq!(opts.scaffold.frontends.len(), 1);
}

#[test]
fn go_react_defaults_derive_module_and_web_shape() {
    let mut opts = init_opts(&[
        "jig",
        "init",
        "My Demo",
        "--preset",
        "go-react",
        "--defaults",
        "--no-vault",
    ]);

    prepare_init_interaction_with_io(
        &mut opts,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap();

    assert_eq!(opts.scaffold.db, Some(ScaffoldDb::None));
    assert_eq!(opts.scaffold.frontends.len(), 1);
    assert_eq!(
        opts.answers.go_module.as_deref(),
        Some("example.com/my-demo")
    );
    assert_eq!(
        opts.answers.backend_language,
        Some(crate::bootstrap::BackendLanguage::Go)
    );
    assert_eq!(opts.answers.sqlx_enabled, Some(false));
}

#[test]
fn go_react_defaults_derive_valid_modules_from_edge_case_repo_names() {
    for (repo_name, expected) in [
        (".ExampleProject", "example.com/exampleproject"),
        ("ExampleProject.", "example.com/exampleproject"),
        ("CON", "example.com/app-con"),
    ] {
        let mut opts = init_opts(&[
            "jig",
            "init",
            repo_name,
            "--preset",
            "go-react",
            "--defaults",
            "--no-vault",
        ]);

        prepare_init_interaction_with_io(
            &mut opts,
            &mut Cursor::new(Vec::<u8>::new()),
            &mut Vec::new(),
        )
        .unwrap();

        assert_eq!(opts.answers.go_module.as_deref(), Some(expected));
    }
}

#[test]
fn go_react_strict_mode_requires_module() {
    let mut opts = init_opts(&[
        "jig",
        "init",
        "demo",
        "--preset",
        "go-react",
        "--db",
        "none",
        "--frontends",
        "web",
        "--no-input",
        "--no-vault",
    ]);

    let error = prepare_init_interaction_with_io(
        &mut opts,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("--preset go-react requires --go-module"));
}

#[test]
fn go_react_interactive_module_prompt_retries_without_changing_case() {
    let mut opts = init_opts(&["jig", "init", "demo", "--preset", "go-react", "--no-vault"]);
    let mut input =
        Cursor::new("none\nweb\nexample.com/ExampleProject.\nexample.com/ExampleProject\n");
    let mut output = Vec::new();

    prepare_init_interaction_with_io(&mut opts, &mut input, &mut output).unwrap();

    assert_eq!(
        opts.answers.go_module.as_deref(),
        Some("example.com/ExampleProject")
    );
    let output = String::from_utf8(output).unwrap();
    assert_eq!(output.matches("Invalid --go-module").count(), 1, "{output}");
    assert_eq!(
        output
            .matches("Go module (for example github.com/acme/my-app):")
            .count(),
        2,
        "{output}"
    );
}

#[test]
fn selected_package_manager_must_be_available_before_init_writes() {
    let mut opts = init_opts(&[
        "jig",
        "init",
        "demo",
        "--defaults",
        "--web-package-manager",
        "yarn",
        "--no-vault",
    ]);
    prepare_init_interaction_with_io(
        &mut opts,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap();

    let error = preflight_init_package_manager_with(&opts, |_| false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Selected web package manager 'yarn'"));
    assert!(error.contains("No files were written"));

    preflight_init_package_manager_with(&opts, |program| program == "yarn").unwrap();
}

#[test]
fn harness_only_init_does_not_require_a_web_package_manager() {
    let mut opts = init_opts(&[
        "jig",
        "init",
        "demo",
        "--preset",
        "harness-only",
        "--no-input",
        "--no-vault",
    ]);
    prepare_init_interaction_with_io(
        &mut opts,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap();

    preflight_init_package_manager_with(&opts, |_| false).unwrap();
}

#[test]
fn json_output_does_not_disable_init_wizard() {
    let mut opts = init_opts(&["jig", "--json", "init", "demo", "--no-vault"]);
    let mut input = Cursor::new("harness-only\n");
    let mut output = Vec::new();

    prepare_init_interaction_with_io(&mut opts, &mut input, &mut output).unwrap();

    assert_eq!(opts.scaffold.preset, Some(ScaffoldPreset::HarnessOnly));
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("Scaffold an app?")
    );
}

#[test]
fn numeric_scaffold_aliases_preserve_harness_only_and_append_go() {
    let mut output = Vec::new();
    assert_eq!(
        prompt_scaffold_choice(&mut Cursor::new("2\n"), &mut output).unwrap(),
        ScaffoldChoice::HarnessOnly
    );
    assert_eq!(
        prompt_scaffold_choice(&mut Cursor::new("3\n"), &mut output).unwrap(),
        ScaffoldChoice::GoReact
    );

    let mut header = Vec::new();
    print_project_shape_header(&mut header, &InitPresetMetadata::load()).unwrap();
    let header = String::from_utf8(header).unwrap();
    let rust = header.find("1. rust-react").unwrap();
    let harness = header.find("2. harness-only").unwrap();
    let go = header.find("3. go-react").unwrap();
    assert!(rust < harness && harness < go, "{header}");

    let output = String::from_utf8(output).unwrap();
    assert!(
        output.contains("[1 rust-react / 2 harness-only / 3 go-react]"),
        "{output}"
    );
}

#[test]
fn harness_only_choice_resolves_omitted_sqlx_answers_safely() {
    let mut opts = init_opts(&["jig", "init", "demo", "--no-vault"]);
    let mut input = Cursor::new("harness-only\n");
    let mut output = Vec::new();

    prepare_init_interaction_with_io(&mut opts, &mut input, &mut output).unwrap();

    assert_eq!(opts.scaffold.preset, Some(ScaffoldPreset::HarnessOnly));
    assert_eq!(opts.answers.sqlx_enabled, Some(false));
}

#[test]
fn answers_file_frontends_are_loaded_before_wizard_prompts() {
    let temp = tempdir().unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"repo_name = "demo"
sqlx_enabled = false
schema_dump_enabled = false

[[frontend_apps]]
name = "portal"
dir = "clients/portal"
coverage_threshold = 80
kind = "vite"
role = "spa"
"#,
    )
    .unwrap();
    let mut opts = init_opts(&[
        "jig",
        "init",
        "demo",
        "--preset",
        "rust-react",
        "--answers-file",
        answers_file.to_str().unwrap(),
        "--no-vault",
    ]);
    let mut input = Cursor::new("none\n");
    let mut output = Vec::new();

    prepare_init_interaction_with_io(&mut opts, &mut input, &mut output).unwrap();

    assert_eq!(opts.scaffold.db, Some(ScaffoldDb::None));
    assert!(opts.scaffold.frontends.is_empty());
    assert_eq!(opts.answers.frontend_apps[0].name, "portal");
    assert!(!String::from_utf8(output).unwrap().contains("Frontends?"));
}

#[test]
fn unrelated_answers_file_does_not_prevent_harness_only_sqlx_default() {
    let temp = tempdir().unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(&answers_file, "repo_name = \"demo\"\n").unwrap();
    let mut opts = init_opts(&[
        "jig",
        "init",
        "demo",
        "--answers-file",
        answers_file.to_str().unwrap(),
        "--no-vault",
    ]);
    let mut input = Cursor::new("harness-only\n");
    let mut output = Vec::new();

    prepare_init_interaction_with_io(&mut opts, &mut input, &mut output).unwrap();

    assert_eq!(opts.answers.sqlx_enabled, Some(false));
}

#[test]
fn sqlx_shaped_answers_file_prevents_harness_only_sqlx_default() {
    let temp = tempdir().unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        "repo_name = \"demo\"\nrust_migration_dir = \"migrations\"\n",
    )
    .unwrap();
    let mut opts = init_opts(&[
        "jig",
        "init",
        "demo",
        "--answers-file",
        answers_file.to_str().unwrap(),
        "--no-vault",
    ]);
    let mut input = Cursor::new("harness-only\n");
    let mut output = Vec::new();

    prepare_init_interaction_with_io(&mut opts, &mut input, &mut output).unwrap();

    assert_eq!(opts.answers.sqlx_enabled, None);
    assert_eq!(
        opts.answers.rust_migration_dir.as_deref(),
        Some("migrations")
    );
}

#[test]
fn custom_bare_frontend_name_requires_confirmation() {
    let mut opts = init_opts(&[
        "jig",
        "init",
        "demo",
        "--preset",
        "rust-react",
        "--db",
        "none",
        "--frontends",
        "dashboard",
        "--no-vault",
    ]);
    let mut input = Cursor::new("yes\n");
    let mut output = Vec::new();

    prepare_init_interaction_with_io(&mut opts, &mut input, &mut output).unwrap();

    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("'dashboard' isn't a preset shorthand"));
    assert!(output.contains("custom Vite SPA in dashboard/"));
}

#[test]
fn custom_bare_frontend_name_can_cancel_before_writes() {
    let mut opts = init_opts(&[
        "jig",
        "init",
        "demo",
        "--preset",
        "rust-react",
        "--db",
        "none",
        "--frontends",
        "amdin",
        "--no-vault",
    ]);
    let mut input = Cursor::new("no\n");
    let mut output = Vec::new();

    let error = prepare_init_interaction_with_io(&mut opts, &mut input, &mut output)
        .unwrap_err()
        .to_string();

    assert!(error.contains("cancelled before files were written"));
}

#[test]
fn interactively_selected_rust_react_rejects_backend_name_before_custom_confirmation() {
    let mut opts = init_opts(&["jig", "init", "demo", "--frontends", "API", "--no-vault"]);
    let mut input = Cursor::new("rust-react\nnone\n");
    let mut output = Vec::new();

    let error = prepare_init_interaction_with_io(&mut opts, &mut input, &mut output)
        .unwrap_err()
        .to_string();

    assert!(error.contains("frontend app name 'API'"));
    assert!(error.contains("reserved backend dev app 'api'"));
    assert!(error.contains("choose another frontend name"));
    let output = String::from_utf8(output).unwrap();
    assert!(!output.contains("Custom frontend name"));
    assert!(!output.contains("Continue with the custom frontend name"));
}

#[test]
fn explicit_custom_frontend_kind_needs_no_confirmation() {
    let mut opts = init_opts(&[
        "jig",
        "init",
        "demo",
        "--preset",
        "rust-react",
        "--db",
        "none",
        "--frontends",
        "dashboard:spa",
        "--no-vault",
    ]);
    let mut input = Cursor::new(Vec::<u8>::new());
    let mut output = Vec::new();

    prepare_init_interaction_with_io(&mut opts, &mut input, &mut output).unwrap();

    assert!(output.is_empty());
}

#[test]
fn preset_frontend_metadata_names_are_recognized_as_shorthands() {
    let metadata = InitPresetMetadata::load();

    for (name, _) in metadata.frontends {
        let frontend = parse_scaffold_frontend(&name).unwrap();
        assert_eq!(frontend.custom_default_name_notice(), None, "{name}");
    }

    for alias in [
        "web",
        "admin",
        "admin-panel",
        "landing",
        "marketing",
        "astro",
    ] {
        let frontend = parse_scaffold_frontend(alias).unwrap();
        assert_eq!(frontend.custom_default_name_notice(), None, "{alias}");
    }
}

#[test]
fn defaults_resolve_rust_react_none_and_web_without_reading_input() {
    let mut opts = init_opts(&["jig", "init", "demo", "--defaults", "--no-vault"]);
    let mut input = Cursor::new(Vec::<u8>::new());
    let mut output = Vec::new();

    prepare_init_interaction_with_io(&mut opts, &mut input, &mut output).unwrap();

    assert_eq!(opts.scaffold.preset, Some(ScaffoldPreset::RustReact));
    assert_eq!(opts.scaffold.db, Some(ScaffoldDb::None));
    assert_eq!(opts.scaffold.frontends.len(), 1);
    assert!(output.is_empty());
}

#[test]
fn defaults_preserve_explicit_project_shape_choices() {
    let mut opts = init_opts(&[
        "jig",
        "init",
        "demo",
        "--defaults",
        "--preset",
        "rust-react",
        "--db",
        "sqlite",
        "--frontends",
        "landing,admin",
        "--no-vault",
    ]);
    let mut input = Cursor::new(Vec::<u8>::new());
    let mut output = Vec::new();

    prepare_init_interaction_with_io(&mut opts, &mut input, &mut output).unwrap();

    assert_eq!(opts.scaffold.preset, Some(ScaffoldPreset::RustReact));
    assert_eq!(opts.scaffold.db, Some(ScaffoldDb::Sqlite));
    assert_eq!(opts.scaffold.frontend_list.len(), 2);
    assert!(opts.scaffold.frontends.is_empty());
}

#[test]
fn defaults_keep_answers_file_frontends_instead_of_adding_web() {
    let temp = tempdir().unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"[[frontend_apps]]
name = "portal"
dir = "clients/portal"
coverage_threshold = 80
kind = "vite"
role = "spa"
"#,
    )
    .unwrap();
    let mut opts = init_opts(&[
        "jig",
        "init",
        "demo",
        "--defaults",
        "--answers-file",
        answers_file.to_str().unwrap(),
        "--no-vault",
    ]);
    let mut input = Cursor::new(Vec::<u8>::new());
    let mut output = Vec::new();

    prepare_init_interaction_with_io(&mut opts, &mut input, &mut output).unwrap();

    assert_eq!(opts.scaffold.preset, Some(ScaffoldPreset::RustReact));
    assert_eq!(opts.scaffold.db, Some(ScaffoldDb::None));
    assert!(!opts.scaffold.has_frontends());
    assert_eq!(opts.answers.frontend_apps[0].name, "portal");
}

#[test]
fn defaults_take_precedence_when_no_input_is_also_set() {
    let mut opts = init_opts(&[
        "jig",
        "init",
        "demo",
        "--defaults",
        "--no-input",
        "--no-vault",
    ]);
    let mut input = Cursor::new(Vec::<u8>::new());
    let mut output = Vec::new();

    prepare_init_interaction_with_io(&mut opts, &mut input, &mut output).unwrap();

    assert_eq!(opts.scaffold.preset, Some(ScaffoldPreset::RustReact));
    assert_eq!(opts.scaffold.db, Some(ScaffoldDb::None));
    assert_eq!(opts.scaffold.frontends.len(), 1);
}

#[test]
fn no_input_requires_an_explicit_complete_project_shape() {
    let mut missing_preset = init_opts(&["jig", "init", "demo", "--no-input", "--no-vault"]);
    let error = prepare_init_interaction_with_io(
        &mut missing_preset,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("--no-input was supplied"));
    assert!(error.contains("application preset"));

    let mut missing_db = init_opts(&[
        "jig",
        "init",
        "demo",
        "--preset",
        "rust-react",
        "--frontends",
        "web",
        "--no-input",
        "--no-vault",
    ]);
    let error = prepare_init_interaction_with_io(
        &mut missing_db,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("requires an explicit --db choice"));

    let mut missing_frontend = init_opts(&[
        "jig",
        "init",
        "demo",
        "--preset",
        "rust-react",
        "--db",
        "none",
        "--no-input",
        "--no-vault",
    ]);
    let error = prepare_init_interaction_with_io(
        &mut missing_frontend,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("requires --frontend/--frontends"));
}

#[test]
fn no_input_accepts_answers_file_frontends_as_an_explicit_decision() {
    let temp = tempdir().unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        r#"[[frontend_apps]]
name = "portal"
dir = "portal"
coverage_threshold = 80
kind = "vite"
role = "spa"
"#,
    )
    .unwrap();
    let mut opts = init_opts(&[
        "jig",
        "init",
        "demo",
        "--preset",
        "rust-react",
        "--db",
        "none",
        "--answers-file",
        answers_file.to_str().unwrap(),
        "--no-input",
        "--no-vault",
    ]);

    prepare_init_interaction_with_io(
        &mut opts,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap();

    assert_eq!(opts.answers.frontend_apps[0].name, "portal");
    assert!(!opts.scaffold.has_frontends());
}

#[test]
fn harness_only_is_explicit_and_rejects_scaffold_options() {
    let mut opts = init_opts(&[
        "jig",
        "init",
        "demo",
        "--preset",
        "harness-only",
        "--no-input",
        "--no-vault",
    ]);
    prepare_init_interaction_with_io(
        &mut opts,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(opts.answers.sqlx_enabled, Some(false));

    let mut invalid = init_opts(&[
        "jig",
        "init",
        "demo",
        "--preset",
        "harness-only",
        "--db",
        "none",
        "--no-input",
        "--no-vault",
    ]);
    let error = prepare_init_interaction_with_io(
        &mut invalid,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("harness-only cannot be combined with --db"));
}

#[test]
fn empty_migration_dir_is_omitted_consistently_in_noninteractive_harness_init() {
    for mode in ["--defaults", "--no-input"] {
        let mut opts = init_opts(&[
            "jig",
            "init",
            "demo",
            "--preset",
            "harness-only",
            "--rust-migration-dir",
            "",
            mode,
            "--no-vault",
        ]);

        prepare_init_interaction_with_io(
            &mut opts,
            &mut Cursor::new(Vec::<u8>::new()),
            &mut Vec::new(),
        )
        .unwrap();

        assert_eq!(opts.answers.sqlx_enabled, Some(false), "mode {mode}");
        assert_eq!(opts.answers.rust_migration_dir, None, "mode {mode}");
    }

    let temp = tempdir().unwrap();
    let answers_file = temp.path().join("answers.toml");
    fs::write(
        &answers_file,
        "repo_name = \"demo\"\nrust_migration_dir = \"\"\n",
    )
    .unwrap();
    let mut opts = init_opts(&[
        "jig",
        "init",
        "demo",
        "--preset",
        "harness-only",
        "--answers-file",
        answers_file.to_str().unwrap(),
        "--no-input",
        "--no-vault",
    ]);

    prepare_init_interaction_with_io(
        &mut opts,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(opts.answers.sqlx_enabled, Some(false));
    assert_eq!(opts.answers.rust_migration_dir, None);
}

#[test]
fn implicit_non_terminal_mode_is_strict_but_accepts_resolved_input() {
    let mut unresolved = init_opts(&["jig", "init", "demo", "--no-vault"]);
    let error = prepare_init_interaction_with_terminal(
        &mut unresolved,
        false,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("stdin or stderr is not a terminal"));
    assert!(error.contains("--preset harness-only"));

    let mut resolved = init_opts(&[
        "jig",
        "init",
        "demo",
        "--preset",
        "rust-react",
        "--db",
        "none",
        "--frontends",
        "web",
        "--no-vault",
    ]);
    prepare_init_interaction_with_terminal(
        &mut resolved,
        false,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap();
}

#[test]
fn minimal_answers_resolve_to_harness_only_in_every_interaction_mode() {
    let temp = tempdir().unwrap();
    let minimal = temp.path().join("minimal.toml");
    fs::write(
        &minimal,
        "repo_name = \"demo\"\nharness_footprint = \"minimal\"\n",
    )
    .unwrap();

    for flags in [
        vec![],
        vec!["--defaults"],
        vec!["--no-input"],
        vec!["--preset", "harness-only", "--no-input"],
    ] {
        let mut args = vec![
            "jig",
            "init",
            "demo",
            "--answers-file",
            minimal.to_str().unwrap(),
            "--no-vault",
        ];
        args.extend(flags);
        let mut opts = init_opts(&args);
        let mut output = Vec::new();

        prepare_init_interaction_with_io(
            &mut opts,
            &mut Cursor::new(Vec::<u8>::new()),
            &mut output,
        )
        .unwrap();

        assert_eq!(opts.scaffold.preset, Some(ScaffoldPreset::HarnessOnly));
        assert_eq!(opts.answers.sqlx_enabled, Some(false));
        assert!(output.is_empty());
    }

    let mut non_terminal = init_opts(&[
        "jig",
        "init",
        "demo",
        "--answers-file",
        minimal.to_str().unwrap(),
        "--no-vault",
    ]);
    prepare_init_interaction_with_terminal(
        &mut non_terminal,
        false,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(
        non_terminal.scaffold.preset,
        Some(ScaffoldPreset::HarnessOnly)
    );
}

#[test]
fn malformed_answers_and_minimal_rust_react_conflicts_fail_during_preflight() {
    let temp = tempdir().unwrap();
    let malformed = temp.path().join("malformed.toml");
    fs::write(&malformed, "repo_name = [\n").unwrap();
    let destination = temp.path().join("destination");
    let mut malformed_opts = init_opts(&[
        "jig",
        "init",
        destination.to_str().unwrap(),
        "--answers-file",
        malformed.to_str().unwrap(),
        "--defaults",
        "--no-vault",
    ]);
    let error = prepare_init_interaction_with_io(
        &mut malformed_opts,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("Failed to parse"));
    assert!(!destination.exists());

    let minimal = temp.path().join("minimal.toml");
    fs::write(&minimal, "harness_footprint = \"minimal\"\n").unwrap();
    let mut conflict = init_opts(&[
        "jig",
        "init",
        destination.to_str().unwrap(),
        "--preset",
        "rust-react",
        "--db",
        "none",
        "--frontends",
        "web",
        "--answers-file",
        minimal.to_str().unwrap(),
        "--no-input",
        "--no-vault",
    ]);
    let error = prepare_init_interaction_with_io(
        &mut conflict,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut Vec::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("cannot combine harness_footprint = \"minimal\""));
    assert!(error.contains("Rust React scaffold"));
    assert!(!destination.exists());
}
