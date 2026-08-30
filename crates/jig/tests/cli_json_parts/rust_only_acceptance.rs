fn rust_only_library_descriptor() -> Value {
    json!({
        "name": "rust-library",
        "summary": "Expandable Rust workspace with one library crate.",
        "defaults": [
            "The virtual workspace uses crates/<repo> as its only initial member.",
            "Rust 2024 uses the top-level Jig workspace Rust baseline.",
            "SQLx, schema dumps, application contracts, frontends, and dev apps are disabled."
        ],
        "layout": [
            "Cargo.toml virtual workspace",
            "crates/<repo> library crate"
        ],
        "frontend_shorthands": [],
        "examples": [
            "jig init ./example-library --preset rust-library --no-input --no-vault"
        ],
        "ownership": "The generated Cargo manifests, Rust source, crate guide, and README are project-owned after creation; jig update keeps only the Jig harness current.",
        "non_goals": [
            "The rust-library preset does not create a database, frontend, API, dev app, release workflow, or additional crate layers.",
            "The scaffold does not select a license or enable package publication."
        ]
    })
}

fn rust_only_cli_descriptor() -> Value {
    json!({
        "name": "rust-cli",
        "summary": "Expandable Rust workspace with one command-line binary crate.",
        "defaults": [
            "The virtual workspace uses crates/<repo> as its only initial member.",
            "Rust 2024 uses the top-level Jig workspace Rust baseline.",
            "The starter binary uses only std and prints its package name and version.",
            "SQLx, schema dumps, application contracts, frontends, and dev apps are disabled."
        ],
        "layout": [
            "Cargo.toml virtual workspace",
            "crates/<repo> command-line binary crate"
        ],
        "frontend_shorthands": [],
        "examples": [
            "jig init ./example-cli --preset rust-cli --no-input --no-vault",
            "cargo run -p example-cli"
        ],
        "ownership": "The generated Cargo manifests, Rust source, crate guide, and README are project-owned after creation; jig update keeps only the Jig harness current.",
        "non_goals": [
            "The rust-cli preset does not create a database, frontend, API, dev app, release workflow, library target, or additional crate layers.",
            "The scaffold does not select a license, enable package publication, or choose an argument parser or logging framework."
        ]
    })
}

fn assert_exact_json_object_keys(value: &Value, expected: &[&str]) {
    let actual = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(actual, expected.iter().copied().collect());
}

#[test]
fn rust_only_presets_have_exact_process_descriptors() {
    let output = jig().args(["--json", "presets"]).output().unwrap();
    assert!(
        output.status.success(),
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_exact_json_object_keys(&report, &["command", "ok", "presets"]);
    assert_eq!(report["ok"], true);
    assert_eq!(report["command"], "presets");
    assert_eq!(
        report["presets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|preset| preset["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "rust-react",
            "go-react",
            "harness-only",
            "rust-library",
            "rust-cli",
        ]
    );
    assert_eq!(report["presets"][3], rust_only_library_descriptor());
    assert_eq!(report["presets"][4], rust_only_cli_descriptor());
}

#[test]
fn rust_only_init_process_reports_are_exact_and_generated_jig_checks_pass() {
    let template_parent = tempdir().unwrap();
    let template = template_parent.path().join("ExampleProject-template");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let clone = Command::new("git")
        .args(["clone", "--quiet", "--local", "--no-hardlinks"])
        .arg(&workspace)
        .arg(&template)
        .status()
        .unwrap();
    assert!(clone.success());

    let destinations = tempdir().unwrap();
    for (preset, requested, package, source) in [
        (
            "rust-library",
            "ExampleLibraryProcess",
            "examplelibraryprocess",
            "src/lib.rs",
        ),
        (
            "rust-cli",
            "ExampleCliProcess",
            "examplecliprocess",
            "src/main.rs",
        ),
    ] {
        let destination = destinations.path().join(requested);
        let output = jig()
            .args([
                "--json",
                "init",
                destination.to_str().unwrap(),
                "--preset",
                preset,
                "--template",
                template.to_str().unwrap(),
                "--template-mode",
                "committed",
                "--no-input",
                "--no-vault",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{preset} init failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_exact_json_object_keys(
            &report,
            &[
                "answers_file",
                "command",
                "destination",
                "git_initialized",
                "next_steps",
                "notes",
                "ok",
                "render_mode",
                "render_report",
                "scaffold",
                "template",
                "vault",
            ],
        );
        assert_eq!(report["ok"], true);
        assert_eq!(report["command"], "init");
        assert_eq!(report["render_mode"], "copy");
        assert_eq!(report["destination"], destination.display().to_string());
        assert_eq!(report["answers_file"], ".jig.toml");
        assert_eq!(report["git_initialized"], true);
        assert_eq!(
            report["vault"],
            json!({
                "requested": false,
                "initialized": false,
                "created": false,
                "skipped_reason": "disabled"
            })
        );
        assert_eq!(
            report["scaffold"],
            json!({
                "preset": preset,
                "repo_name": package,
                "repo_name_sanitized_from": requested,
                "db": "none",
                "frontends": [],
                "frontend_notices": [],
                "files_created": [
                    "Cargo.toml",
                    "README.md",
                    format!("crates/{package}/Cargo.toml"),
                    format!("crates/{package}/AGENTS.md"),
                    format!("crates/{package}/{source}"),
                ],
                "files_modified": [],
                "files_unchanged": [],
            })
        );
        assert_exact_json_object_keys(
            &report["render_report"],
            &[
                "active_managed_paths",
                "backups",
                "commands_detected_or_skipped",
                "conflicts",
                "dry_run",
                "files_created",
                "files_modified",
                "files_removed",
                "files_unchanged",
                "managed_blocks_inserted",
                "managed_blocks_rendered",
                "retired_managed_paths",
                "suggested_jig_toml_edits",
                "todos",
            ],
        );
        assert!(report["next_steps"].is_array());
        assert!(report["notes"].is_array());

        for check in ["contract", "agent-map", "agent-guides"] {
            let output = jig()
                .current_dir(&destination)
                .args(["check", check])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{preset} jig check {check} failed with {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

#[test]
fn rust_only_invalid_process_invocations_emit_one_json_object_without_stderr() {
    let temp = tempdir().unwrap();
    for (preset, destination_name, extra, expected_kind, expected_flag) in [
        (
            "rust-library",
            "ExampleLibraryPolicyError",
            &["--db", "postgres"][..],
            "command_failed",
            "--db",
        ),
        (
            "rust-cli",
            "ExampleCliPolicyError",
            &["--frontend", "web"][..],
            "command_failed",
            "--frontend",
        ),
        (
            "rust-library",
            "ExampleLibraryUsageError",
            &["--db"][..],
            "usage",
            "--db",
        ),
        (
            "rust-cli",
            "ExampleCliUsageError",
            &["--frontend"][..],
            "usage",
            "--frontend",
        ),
    ] {
        let destination = temp.path().join(destination_name);
        let mut command = jig();
        command.args([
            "--json",
            "init",
            destination.to_str().unwrap(),
            "--preset",
            preset,
            "--template",
            "/missing/ExampleProject-template",
            "--no-input",
            "--no-vault",
        ]);
        command.args(extra);
        let output = command.output().unwrap();

        assert!(!output.status.success(), "invalid {preset} invocation succeeded");
        assert!(output.stderr.is_empty(), "{preset} contaminated stderr");
        let objects = serde_json::Deserializer::from_slice(&output.stdout)
            .into_iter::<Value>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(objects.len(), 1, "{preset} emitted multiple JSON objects");
        let error = &objects[0];
        assert_exact_json_object_keys(error, &["error", "exit_status", "ok"]);
        assert_eq!(error["ok"], false);
        assert_eq!(error["error"]["kind"], expected_kind);
        let message = error["error"]["message"].as_str().unwrap();
        assert!(message.contains(expected_flag), "{message}");
        if expected_kind == "command_failed" {
            assert!(message.contains(preset), "{message}");
            assert!(!message.contains("Failed to inspect template source"), "{message}");
        }
        assert!(!destination.exists());
    }
}
