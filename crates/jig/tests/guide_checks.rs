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
    // Shell runner is the preexisting legacy contract. Quote the one fixed binary
    // path; guide content is never a command argument or executable input.
    let executable = env!("CARGO_BIN_EXE_jig").replace('\'', "'\\''");
    let config = json!({
        "_src_path":"embedded:jig-sh", "_commit":"example", "repo_name":"ExampleProject",
        "jig_version":"0.2.0-beta.1", "default_branch":"main", "rust_crate_roots":[],
        "commands":{"guides_command":format!("'{executable}' check agent-guides --json")},
        "work":{"gates":[{"id":"guides","kind":"check","tool":"jig.guides","required":true}]}
    });
    fs::write(
        root.path().join(".jig.toml"),
        toml::to_string(&config).unwrap(),
    )
    .unwrap();
    fs::write(root.path().join(".agent/jig-contract.json"), serde_json::to_vec(&json!({
        "contract_version":3,"jig_version":"0.2.0-beta.1","tool_namespace":"jig",
        "required_commands":["guides_command"],
        "tools":[{"name":"jig.guides","kind":"command","command":"guides_command","description":"Validate ExampleProject guides."}]
    })).unwrap()).unwrap();
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
