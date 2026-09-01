use super::*;

pub(super) fn assert_minimal_report(output: &serde_json::Value) {
    assert_eq!(output["harness_footprint"], "minimal");
    assert_eq!(output["ok"], true);
    let generated_gates = output["adoption_profile"]["generated_gates"]
        .as_array()
        .unwrap();
    assert!(
        generated_gates
            .iter()
            .all(|gate| gate.as_str().unwrap().starts_with("jig "))
    );
    assert!(generated_gates.iter().any(|gate| gate == "jig bootstrap"));
    let commands = output["render_report"]["commands_detected_or_skipped"]
        .as_array()
        .unwrap();
    assert!(
        commands
            .iter()
            .all(|command| !command.as_str().unwrap().contains("scripts/jig"))
    );
    assert!(commands.iter().any(|command| {
        command
            .as_str()
            .unwrap()
            .contains("bootstrap_command configured; run jig bootstrap")
    }));
}

pub(super) fn assert_minimal_files(repo: &Path) {
    let config = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    assert!(config.contains("harness_footprint = \"minimal\""));
    for path in [
        ".agent/jig-contract.json",
        ".agent/PLANS.md",
        ".agent/plans/.gitkeep",
        ".agent/state/.gitkeep",
        ".agent/.cache/.gitignore",
        managed_paths::MANIFEST_PATH,
        ".gitignore",
        ".gitattributes",
    ] {
        assert!(repo.join(path).is_file(), "missing minimal path {path}");
    }
    for path in [
        "scripts/jig",
        "scripts/install-jig.sh",
        ".mcp.json",
        "AGENTS.md",
        "agent-map.md",
        ".github/workflows/rust-tests.yml",
        ".github/workflows/repo-policy.yml",
        ".github/workflows/agent-map-check.yml",
    ] {
        assert!(
            !repo.join(path).exists(),
            "unexpected full-harness path {path}"
        );
    }
}

pub(super) fn assert_minimal_manifest(repo: &Path, output: &serde_json::Value) {
    let manifest_paths = managed_manifest_paths(repo);
    let reported_paths = |value: &serde_json::Value| {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|path| path.as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        manifest_paths,
        reported_paths(&output["adoption_profile"]["managed_files"])
    );
    assert_eq!(
        manifest_paths,
        reported_paths(&output["render_report"]["active_managed_paths"])
    );
    assert!(
        output["render_report"]["retired_managed_paths"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(manifest_paths.windows(2).all(|paths| paths[0] < paths[1]));
    assert!(manifest_paths.iter().all(|path| repo.join(path).is_file()));
    assert!(
        manifest_paths
            .iter()
            .any(|path| path == managed_paths::MANIFEST_PATH)
    );
    assert!(manifest_paths.iter().all(|path| path != "AGENTS.md"));
}

pub(super) fn assert_minimal_guidance(output: &serde_json::Value) {
    assert!(
        output["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|note| note.as_str().unwrap().contains("Minimal adoption"))
    );
    assert!(
        output["next_steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step.as_str().unwrap().contains("jig loop"))
    );
}

pub(super) fn assert_minimal_contract(repo: &Path) {
    let ctx = crate::context::RepoContext::load_from(repo).unwrap();
    assert_eq!(ctx.repo_name(), "demo");
    assert!(!ctx.required_commands().is_empty());
    assert_eq!(crate::policy::contract_check(&ctx).exit_status, 0);
}
