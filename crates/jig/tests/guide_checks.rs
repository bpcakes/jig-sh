use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};
use tempfile::TempDir;

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jig"))
        .args(args)
        .env_remove("JIG_REPO_ROOT")
        .env("NO_COLOR", "1")
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn fixture(guide: &str) -> TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".agent")).unwrap();
    let target = json!({"component":"repo", "action":"guides"});
    let repository = json!({
        "components":[{"id":"repo", "root":"."}],
        "actions":[{
            "target":target, "intent":"check", "effects":["read_only", "process"],
            "runner":{"kind":"argv", "program":env!("CARGO_BIN_EXE_jig"), "args":["check", "agent-guides", "--json"]}
        }],
        "profiles":[{"id":"verify", "targets":[target]}], "default_check_profile":"verify"
    });
    let config = json!({
        "_src_path":"embedded:jig-sh", "_commit":"example", "repo_name":"ExampleProject",
        "default_branch":"main", "repository":repository,
        "work":{"gates":[{"id":"guides", "kind":"evidence", "profile":"verify", "conclusion":"success"}]}
    });
    fs::write(
        root.path().join(".jig.toml"),
        toml::to_string(&config).unwrap(),
    )
    .unwrap();
    let mut manifest = repository;
    manifest["contract_version"] = json!(8);
    manifest["tool_namespace"] = json!("jig");
    manifest["required_commands"] = json!([]);
    manifest["tools"] = json!([]);
    fs::write(
        root.path().join(".agent/jig-contract.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    fs::write(root.path().join("AGENTS.md"), guide).unwrap();
    root
}

fn parse(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn piped_checks_distinguish_advice_from_reference_errors() {
    let root = fixture(
        "# Ownership\nKeep changes local. [External](https://example.invalid/unverified)\n",
    );
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/AGENTS.md"), "# Source ownership\n").unwrap();
    let output = run(root.path(), &["check", "agent-guides", "--json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = parse(&output);
    assert_eq!(report["ok"], true);
    assert!(
        report["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["severity"] == "warning")
    );
    assert!(
        report["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "external_reference")
    );
    assert!(output.stderr.is_empty());
    let plain = run(root.path(), &["check", "agent-guides"]);
    assert!(plain.status.success());
    assert!(String::from_utf8_lossy(&plain.stdout).contains("Warnings: 1"));
    assert!(!plain.stdout.contains(&0x1b));
    fs::write(
        root.path().join("AGENTS.md"),
        "# Owner\n[Broken](missing.md)\n",
    )
    .unwrap();
    let broken = run(root.path(), &["check", "agent-guides", "--json"]);
    assert!(!broken.status.success());
    assert_eq!(parse(&broken)["ok"], false);
    assert_eq!(
        parse(&broken)["diagnostics"][0]["code"],
        "reference_missing"
    );
    let plain = run(root.path(), &["check", "agent-guides"]);
    assert!(!plain.status.success());
    let text = String::from_utf8_lossy(&plain.stdout);
    assert!(text.contains("AGENTS.md:2: reference_missing"), "{text}");
    assert!(text.contains("missing.md"), "{text}");
}

#[test]
fn warning_only_check_satisfies_required_gate_and_work_finish() {
    let root = fixture("# ExampleProject ownership\nKeep public behavior stable.\n");
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/AGENTS.md"), "# Source ownership\n").unwrap();
    let advice = run(root.path(), &["check", "agent-guides", "--json"]);
    assert!(advice.status.success());
    let report = parse(&advice);
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(report["diagnostics"][0]["severity"], "warning");
    assert_eq!(report["diagnostics"][0]["guide"], "src/AGENTS.md");
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.name", "ExampleMaintainer"],
        vec!["config", "user.email", "example@example.invalid"],
        vec!["add", "."],
        vec!["commit", "-qm", "Example baseline"],
    ] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(root.path())
                .status()
                .unwrap()
                .success()
        );
    }
    let opened = run(
        root.path(),
        &[
            "work",
            "start",
            "--title",
            "Example guide validation",
            "--body",
            "Validate guide advice.",
            "--print-plan-id",
        ],
    );
    assert!(
        opened.status.success(),
        "{}",
        String::from_utf8_lossy(&opened.stderr)
    );
    let plan = String::from_utf8(opened.stdout).unwrap();
    let plan = plan.trim();
    let checked = run(root.path(), &["work", "check", "--plan-id", plan, "--json"]);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let finished = run(
        root.path(),
        &[
            "work",
            "finish",
            "--plan-id",
            plan,
            "--resolution",
            "Example guide has valid references.",
            "--json",
        ],
    );
    assert!(
        finished.status.success(),
        "{}",
        String::from_utf8_lossy(&finished.stderr)
    );
    assert_eq!(parse(&finished)["ok"], true);
}

#[cfg(unix)]
#[test]
fn human_reference_diagnostics_escape_control_characters_in_guide_names() {
    let root = fixture("# Root\n");
    let directory = root.path().join("example\u{1b}[31m\u{202e}");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("AGENTS.md"), "[Broken](missing.md)\n").unwrap();
    let json = run(root.path(), &["check", "agent-guides", "--json"]);
    assert!(!json.status.success());
    let report = parse(&json);
    assert!(report["diagnostics"].as_array().unwrap().iter().any(|d| {
        d["guide"]
            .as_str()
            .is_some_and(|path| path.contains('\u{1b}'))
    }));
    let plain = run(root.path(), &["check", "agent-guides"]);
    let text = String::from_utf8(plain.stdout).unwrap();
    assert!(!text.contains(['\u{1b}', '\u{202e}']), "{text:?}");
}

#[test]
fn email_and_url_autolinks_are_external_in_guide_and_map_checks() {
    let links = "<maintainer@example.invalid>\n<https://example.invalid/docs>\n[Email](mailto:maintainer@example.invalid)\n";
    let root = fixture(links);
    let output = run(root.path(), &["check", "agent-guides", "--json"]);
    assert!(output.status.success(), "{output:?}");
    let report = parse(&output);
    let diagnostics = report["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 3);
    assert!(
        diagnostics
            .iter()
            .all(|d| d["code"] == "external_reference" && d["severity"] == "info")
    );
    assert_eq!(
        diagnostics[0]["reference"],
        "mailto:maintainer@example.invalid"
    );
    assert_eq!(diagnostics[0]["line"], 1);
    fs::write(
        root.path().join("agent-map.md"),
        format!("[Root](AGENTS.md)\n{links}"),
    )
    .unwrap();
    let map = run(root.path(), &["check", "agent-map", "--json"]);
    assert!(map.status.success(), "{map:?}");
    assert_eq!(parse(&map)["ok"], true);

    // An ordinary file link containing '@' must retain local-path semantics.
    fs::write(
        root.path().join("AGENTS.md"),
        "[File](maintainer@example.invalid)\n",
    )
    .unwrap();
    let local = run(root.path(), &["check", "agent-guides", "--json"]);
    assert!(!local.status.success());
    assert_eq!(parse(&local)["diagnostics"][0]["code"], "reference_missing");
}

#[test]
fn human_owner_errors_identify_each_component_and_guidance_safely() {
    let root = fixture("# ExampleProject\n");
    let components = json!([
        {"id":"example-api", "root":".", "adapters":[], "guidance":"docs/api.md"},
        {"id":"example-worker", "root":".", "adapters":[], "guidance":"docs/worker.md"},
        {"id":"example-invalid", "root":".", "adapters":[], "guidance":"docs/\u{1b}[31mowner.md"}
    ]);
    let profiles = json!([{"id":"verify", "targets":[]}]);
    let config = json!({
        "_src_path":"embedded:jig-sh", "_commit":"example", "repo_name":"ExampleProject",
        "default_branch":"main",
        "repository":{"components":components, "actions":[], "profiles":profiles, "default_check_profile":"verify"}
    });
    fs::write(
        root.path().join(".jig.toml"),
        toml::to_string(&config).unwrap(),
    )
    .unwrap();
    fs::write(root.path().join(".agent/jig-contract.json"), serde_json::to_vec(&json!({
        "contract_version":8, "tool_namespace":"jig", "required_commands":[], "tools":[],
        "components":components, "actions":[], "profiles":profiles, "default_check_profile":"verify"
    })).unwrap()).unwrap();
    let json = run(root.path(), &["check", "agent-guides", "--json"]);
    assert!(!json.status.success());
    let report = parse(&json);
    let diagnostics = report["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 3);
    assert!(diagnostics.iter().any(
        |d| d["component"] == "example-invalid" && d["reference"] == "docs/\u{1b}[31mowner.md"
    ));
    let plain = run(root.path(), &["check", "agent-guides"]);
    assert!(!plain.status.success());
    assert!(plain.stderr.is_empty());
    let text = String::from_utf8(plain.stdout).unwrap();
    for (component, reference) in [
        ("example-api", "docs/api.md"),
        ("example-worker", "docs/worker.md"),
    ] {
        assert!(
            text.lines().any(|line| line.contains(component)
                && line.contains(reference)
                && line.contains("owner_guide_missing")),
            "{text}"
        );
    }
    assert!(
        text.lines()
            .any(|line| line.contains("example-invalid") && line.contains("owner_guide_invalid")),
        "{text}"
    );
    assert!(!text.contains('\u{1b}'), "{text:?}");
}

fn legacy_fixture(epoch: u32) -> TempDir {
    let root = fixture("# ExampleProject\n[Agent map](agent-map.md)\n");
    let mut config = json!({
        "_src_path":"embedded:jig-sh", "_commit":"example", "repo_name":"ExampleProject",
        "default_branch":"main", "rust_crate_roots":["crates"]
    });
    let mut manifest = json!({
        "contract_version":epoch, "tool_namespace":"jig", "required_commands":[], "tools":[]
    });
    if epoch <= 5 {
        // Legacy repositories must declare at least one command-backed tool,
        // even though this test invokes the runtime-owned guide check directly.
        let executable = env!("CARGO_BIN_EXE_jig").replace('\'', "'\\''");
        config["commands"] = json!({
            "guides_command":format!("'{executable}' check agent-guides --json")
        });
        manifest["required_commands"] = json!(["guides_command"]);
        manifest["tools"] = json!([{
            "name":"jig.guides", "kind":"command", "command":"guides_command",
            "description":"Validate ExampleProject guides."
        }]);
    }
    if epoch <= 3 {
        config["jig_version"] = json!("0.2.0-beta.1");
        manifest["jig_version"] = json!("0.2.0-beta.1");
    }
    if epoch >= 6 {
        let repository = json!({
            "components":[{"id":"example-api", "root":"crates/api", "adapters":["rust"]}],
            "actions":[], "profiles":[{"id":"verify", "targets":[]}], "default_check_profile":"verify"
        });
        config["repository"] = repository.clone();
        for (key, value) in repository.as_object().unwrap() {
            manifest[key] = value.clone();
        }
    }
    fs::write(
        root.path().join(".jig.toml"),
        toml::to_string(&config).unwrap(),
    )
    .unwrap();
    fs::write(
        root.path().join(".agent/jig-contract.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    fs::create_dir_all(root.path().join("crates/api")).unwrap();
    root
}

const LEGACY_GUIDE: &str = "## Purpose\nExample API.\n## Key entrypoints\n`src/lib.rs`\n## Edit here for X\nAPI changes.\n## Invariants\nStable API.\n## Common commands\nRun tests.\n";

#[test]
fn legacy_epochs_preserve_guide_policy_without_validating_link_targets() {
    for epoch in 2..=7 {
        let root = legacy_fixture(epoch);
        fs::create_dir_all(root.path().join("target")).unwrap();
        fs::write(
            root.path().join("target/example.md"),
            "Example generated document\n",
        )
        .unwrap();
        fs::create_dir_all(root.path().join("docs")).unwrap();
        fs::write(
            root.path().join("docs/AGENTS.md"),
            "[Unowned](missing.md)\n",
        )
        .unwrap();
        fs::write(
            root.path().join("crates/api/AGENTS.md"),
            format!("{LEGACY_GUIDE}[Missing](missing.md)\n[Ignored](../../target/example.md)\n"),
        )
        .unwrap();
        assert!(!root.path().join("agent-map.md").exists());
        let output = run(root.path(), &["check", "agent-guides", "--json"]);
        assert!(output.status.success(), "epoch {epoch}: {output:?}");
        let result = parse(&output);
        assert_eq!(result["ok"], true, "epoch {epoch}: {result}");
        assert_eq!(result["guide_count"], 1);
        assert!(result.get("diagnostics").is_none());
        for field in ["missing_guides", "missing_sections", "missing_entry_ref"] {
            assert_eq!(result[field], json!([]));
        }

        // Keeping links unchecked must also retain the old positive requirements.
        fs::write(
            root.path().join("crates/api/AGENTS.md"),
            "# API ownership\n",
        )
        .unwrap();
        let output = run(root.path(), &["check", "agent-guides", "--json"]);
        assert!(!output.status.success(), "epoch {epoch}: {output:?}");
        let result = parse(&output);
        assert_eq!(result["missing_sections"].as_array().unwrap().len(), 5);
        assert_eq!(result["missing_entry_ref"].as_array().unwrap().len(), 1);
    }
}

#[cfg(unix)]
#[test]
fn legacy_epochs_do_not_inspect_symlinked_link_targets() {
    use std::os::unix::fs::symlink;

    for epoch in 2..=7 {
        let root = legacy_fixture(epoch);
        let outside = tempfile::tempdir().unwrap();
        fs::write(
            outside.path().join("example.md"),
            "Example external document\n",
        )
        .unwrap();
        symlink(
            outside.path().join("example.md"),
            root.path().join("crates/api/linked.md"),
        )
        .unwrap();
        symlink(
            outside.path(),
            root.path().join("crates/api/linked-directory"),
        )
        .unwrap();
        fs::write(
            root.path().join("crates/api/AGENTS.md"),
            format!("{LEGACY_GUIDE}[Leaf](linked.md)\n[Ancestor](linked-directory/example.md)\n"),
        )
        .unwrap();
        let output = run(root.path(), &["check", "agent-guides", "--json"]);
        assert!(output.status.success(), "epoch {epoch}: {output:?}");
        assert_eq!(parse(&output)["guide_count"], 1);
    }
}
