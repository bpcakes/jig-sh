use std::io::Cursor;

use clap::Parser;

use super::*;
use crate::cli::{Cli, CommandKind};

fn init_opts(args: &[&str]) -> InitOpts {
    match Cli::try_parse_from(args).unwrap().command {
        CommandKind::Init(opts) => opts,
        other => panic!("expected init command, got {other:?}"),
    }
}

fn prepare(opts: &mut InitOpts) -> String {
    let mut output = Vec::new();
    prepare_init_interaction_with_io(opts, &mut Cursor::new(Vec::<u8>::new()), &mut output)
        .unwrap();
    String::from_utf8(output).unwrap()
}

#[test]
fn guided_choice_numbers_exact_names_and_legacy_aliases_cover_the_public_family() {
    for (answer, expected) in [
        ("1", ScaffoldChoice::RustReact),
        ("rust-react", ScaffoldChoice::RustReact),
        ("app", ScaffoldChoice::RustReact),
        ("yes", ScaffoldChoice::RustReact),
        ("y", ScaffoldChoice::RustReact),
        ("2", ScaffoldChoice::HarnessOnly),
        ("harness", ScaffoldChoice::HarnessOnly),
        ("harness-only", ScaffoldChoice::HarnessOnly),
        ("no", ScaffoldChoice::HarnessOnly),
        ("n", ScaffoldChoice::HarnessOnly),
        ("3", ScaffoldChoice::GoReact),
        ("go", ScaffoldChoice::GoReact),
        ("go-react", ScaffoldChoice::GoReact),
        ("4", ScaffoldChoice::RustLibrary),
        ("rust-library", ScaffoldChoice::RustLibrary),
        ("5", ScaffoldChoice::RustCli),
        ("rust-cli", ScaffoldChoice::RustCli),
    ] {
        let mut output = Vec::new();
        let actual =
            prompt_scaffold_choice(&mut Cursor::new(format!("{answer}\n")), &mut output).unwrap();

        assert_eq!(actual, expected, "answer {answer:?}");
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "Project shape? [1 rust-react / 2 harness-only / 3 go-react / 4 rust-library / 5 rust-cli] (1): ",
            "answer {answer:?}"
        );
    }
}

#[test]
fn guided_header_appends_descriptor_backed_rust_only_rows_in_exact_order() {
    let mut output = Vec::new();
    print_project_shape_header(&mut output, &InitPresetMetadata::load()).unwrap();

    assert_eq!(
        String::from_utf8(output).unwrap(),
        format!(
            "Project shape\n  1. rust-react — {}\n  2. harness-only — Jig harness without starter application code.\n  3. go-react — Go 1.26, chi, Huma, pgx/sqlc/Goose, and React.\n  4. rust-library — {}\n  5. rust-cli — {}\n",
            ScaffoldPreset::RustReact.descriptor().summary(),
            ScaffoldPreset::RustLibrary.descriptor().summary(),
            ScaffoldPreset::RustCli.descriptor().summary(),
        )
    );
}

#[test]
fn invalid_guided_choice_retries_with_the_complete_family() {
    let mut output = Vec::new();
    let choice =
        prompt_scaffold_choice(&mut Cursor::new("unknown\nrust-cli\n"), &mut output).unwrap();
    let output = String::from_utf8(output).unwrap();

    assert_eq!(choice, ScaffoldChoice::RustCli);
    assert_eq!(output.matches("Project shape? [").count(), 2, "{output}");
    assert!(output.contains(
        "Enter 1, 2, 3, 4, 5, rust-react, harness-only, go-react, rust-library, or rust-cli."
    ));
}

#[test]
fn guided_harness_and_rust_only_choices_do_not_prompt_for_application_shape() {
    for (choice, preset) in [
        ("2", ScaffoldPreset::HarnessOnly),
        ("4", ScaffoldPreset::RustLibrary),
        ("5", ScaffoldPreset::RustCli),
    ] {
        let mut opts = init_opts(&["jig", "init", "ExampleProject", "--no-vault"]);
        let mut output = Vec::new();
        prepare_init_interaction_with_io(
            &mut opts,
            &mut Cursor::new(format!("{choice}\nunused\n")),
            &mut output,
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();

        assert_eq!(opts.scaffold.preset, Some(preset), "choice {choice}");
        assert_eq!(opts.scaffold.db, None, "choice {choice}");
        assert!(!opts.scaffold.has_frontends(), "choice {choice}");
        assert!(!output.contains("Database?"), "{output}");
        assert!(!output.contains("Frontends?"), "{output}");
        assert!(!output.contains("Go module"), "{output}");
    }
}

#[test]
fn guided_application_choices_still_prompt_from_typed_capabilities() {
    let mut rust = init_opts(&["jig", "init", "ExampleProject", "--no-vault"]);
    let mut rust_output = Vec::new();
    prepare_init_interaction_with_io(
        &mut rust,
        &mut Cursor::new("1\nnone\nweb\n"),
        &mut rust_output,
    )
    .unwrap();
    let rust_output = String::from_utf8(rust_output).unwrap();
    assert_eq!(rust.scaffold.preset, Some(ScaffoldPreset::RustReact));
    assert_eq!(rust.scaffold.db, Some(ScaffoldDb::None));
    assert_eq!(rust.scaffold.frontends.len(), 1);
    assert!(rust_output.contains("Database?"), "{rust_output}");
    assert!(rust_output.contains("Frontends?"), "{rust_output}");
    assert!(!rust_output.contains("Go module"), "{rust_output}");

    let mut go = init_opts(&["jig", "init", "ExampleProject", "--no-vault"]);
    let mut go_output = Vec::new();
    prepare_init_interaction_with_io(
        &mut go,
        &mut Cursor::new("3\nnone\nweb\n\n"),
        &mut go_output,
    )
    .unwrap();
    let go_output = String::from_utf8(go_output).unwrap();
    assert_eq!(go.scaffold.preset, Some(ScaffoldPreset::GoReact));
    assert_eq!(go.scaffold.db, Some(ScaffoldDb::None));
    assert_eq!(go.scaffold.frontends.len(), 1);
    assert_eq!(
        go.answers.go_module.as_deref(),
        Some("example.com/exampleproject")
    );
    assert!(go_output.contains("Database?"), "{go_output}");
    assert!(go_output.contains("Frontends?"), "{go_output}");
    assert!(go_output.contains("Go module"), "{go_output}");
}

#[test]
fn defaults_preserve_the_implicit_shape_and_respect_every_explicit_preset() {
    for (preset_name, preset) in [
        (None, ScaffoldPreset::RustReact),
        (Some("rust-react"), ScaffoldPreset::RustReact),
        (Some("harness-only"), ScaffoldPreset::HarnessOnly),
        (Some("go-react"), ScaffoldPreset::GoReact),
        (Some("rust-library"), ScaffoldPreset::RustLibrary),
        (Some("rust-cli"), ScaffoldPreset::RustCli),
    ] {
        let mut args = vec!["jig", "init", "ExampleProject", "--defaults", "--no-vault"];
        if let Some(preset_name) = preset_name {
            args.extend(["--preset", preset_name]);
        }
        let mut opts = init_opts(&args);

        assert!(prepare(&mut opts).is_empty());
        assert_eq!(opts.scaffold.preset, Some(preset), "{preset:?}");
        if matches!(preset, ScaffoldPreset::RustReact | ScaffoldPreset::GoReact) {
            assert_eq!(opts.scaffold.db, Some(ScaffoldDb::None), "{preset:?}");
            assert_eq!(opts.scaffold.frontends.len(), 1, "{preset:?}");
        } else {
            assert_eq!(opts.scaffold.db, None, "{preset:?}");
            assert!(!opts.scaffold.has_frontends(), "{preset:?}");
        }
        if preset == ScaffoldPreset::GoReact {
            assert_eq!(
                opts.answers.go_module.as_deref(),
                Some("example.com/exampleproject")
            );
        }
    }
}

#[test]
fn strict_and_non_terminal_modes_accept_every_complete_explicit_shape() {
    let cases: &[(&[&str], ScaffoldPreset)] = &[
        (
            &[
                "--preset",
                "rust-react",
                "--db",
                "none",
                "--frontend",
                "web",
            ],
            ScaffoldPreset::RustReact,
        ),
        (&["--preset", "harness-only"], ScaffoldPreset::HarnessOnly),
        (
            &[
                "--preset",
                "go-react",
                "--db",
                "none",
                "--frontend",
                "web",
                "--go-module",
                "example.com/ExampleProject",
            ],
            ScaffoldPreset::GoReact,
        ),
        (&["--preset", "rust-library"], ScaffoldPreset::RustLibrary),
        (&["--preset", "rust-cli"], ScaffoldPreset::RustCli),
    ];

    for (shape, preset) in cases {
        let mut strict_args = vec!["jig", "init", "ExampleProject"];
        strict_args.extend_from_slice(shape);
        strict_args.extend(["--no-input", "--no-vault"]);
        let mut strict = init_opts(&strict_args);
        assert!(prepare(&mut strict).is_empty(), "{preset:?}");
        assert_eq!(strict.scaffold.preset, Some(*preset));

        let mut non_terminal_args = vec!["jig", "init", "ExampleProject"];
        non_terminal_args.extend_from_slice(shape);
        non_terminal_args.push("--no-vault");
        let mut non_terminal = init_opts(&non_terminal_args);
        prepare_init_interaction_with_terminal(
            &mut non_terminal,
            false,
            &mut Cursor::new(Vec::<u8>::new()),
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(non_terminal.scaffold.preset, Some(*preset));
    }
}

#[test]
fn strict_missing_preset_diagnostic_enumerates_every_complete_shape() {
    for (extra, mode) in [
        (Some("--no-input"), "--no-input was supplied"),
        (None, "stdin or stderr is not a terminal"),
    ] {
        let mut args = vec!["jig", "init", "ExampleProject", "--no-vault"];
        if let Some(extra) = extra {
            args.push(extra);
        }
        let mut opts = init_opts(&args);
        let error = prepare_init_interaction_with_terminal(
            &mut opts,
            false,
            &mut Cursor::new(Vec::<u8>::new()),
            &mut Vec::new(),
        )
        .unwrap_err()
        .to_string();

        assert_eq!(
            error,
            format!(
                "Init cannot prompt because {mode}; pass --preset rust-react with explicit database and frontend choices, pass --preset go-react with explicit database, frontend, and Go module choices, pass --preset harness-only, --preset rust-library, or --preset rust-cli, or use --defaults"
            )
        );
    }
}

#[test]
fn missing_package_manager_probe_is_skipped_only_for_non_web_presets() {
    for preset in ["harness-only", "rust-library", "rust-cli"] {
        let mut opts = init_opts(&[
            "jig",
            "init",
            "ExampleProject",
            "--preset",
            preset,
            "--web-package-manager",
            "yarn",
            "--no-input",
            "--no-vault",
        ]);
        let _ = prepare(&mut opts);
        preflight_init_package_manager_with(&opts, |_| {
            panic!("{preset} must not probe a package-manager executable")
        })
        .unwrap();
    }

    for shape in [
        &[
            "--preset",
            "rust-react",
            "--db",
            "none",
            "--frontend",
            "web",
        ][..],
        &[
            "--preset",
            "go-react",
            "--db",
            "none",
            "--frontend",
            "web",
            "--go-module",
            "example.com/ExampleProject",
        ][..],
    ] {
        let mut args = vec!["jig", "init", "ExampleProject"];
        args.extend_from_slice(shape);
        args.extend(["--web-package-manager", "yarn", "--no-input", "--no-vault"]);
        let mut opts = init_opts(&args);
        let _ = prepare(&mut opts);
        assert!(
            preflight_init_package_manager_with(&opts, |_| false)
                .unwrap_err()
                .to_string()
                .contains("Selected web package manager 'yarn'")
        );
    }
}
