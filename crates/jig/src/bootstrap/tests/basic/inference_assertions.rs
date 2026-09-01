use super::*;

pub(super) fn assert_inferred_detection(output: &serde_json::Value) {
    let report = &output["detection_report"];
    assert_eq!(report["repo_name"], "inferred-demo");
    assert_eq!(report["rust_crate_roots"][0], "crates");
    assert_eq!(report["sqlx_enabled"], true);
    assert_eq!(report["rust_migration_dir"], "migrations");
    assert_eq!(report["web_package_manager"], "pnpm");
    assert_eq!(report["frontend_apps"][0]["dir"], "web");
    assert_eq!(report["metadata"]["sqlx_enabled"]["confidence"], "high");
    let sources = report["metadata"]["sqlx_enabled"]["sources"]
        .as_array()
        .unwrap();
    assert!(
        sources
            .iter()
            .any(|source| source.as_str().unwrap().contains("workspace.dependencies"))
    );
    assert!(
        sources
            .iter()
            .any(|source| source.as_str() == Some("migrations/0001_init.sql"))
    );
}

pub(super) fn assert_inferred_profile(output: &serde_json::Value) {
    let profile = &output["adoption_profile"];
    assert_eq!(
        profile["detected_stack"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["Rust workspace", "SQLx", "pnpm", "Vite", "GitHub Actions"]
    );
    assert_eq!(
        profile["ci_shape"]["workflow_files"][0],
        ".github/workflows/rust.yml"
    );
    assert_eq!(
        profile["ci_shape"]["generated_jig_checks_role"],
        "supplement_existing_ci"
    );
    assert!(
        !output["adoption_review"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.as_str().unwrap().contains("overrides:"))
    );
}

pub(super) fn assert_inferred_gates(output: &serde_json::Value) {
    let gates = output["adoption_profile"]["generated_gates"]
        .as_array()
        .unwrap();
    assert!(gates.iter().any(|gate| gate == "scripts/jig check sqlx"));
    assert!(!gates.iter().any(|gate| gate == "scripts/jig check schema"));
    assert!(
        gates
            .iter()
            .any(|gate| gate == "scripts/jig check typescript-coverage")
    );
    assert!(
        gates
            .iter()
            .all(|gate| gate.as_str().unwrap().starts_with("scripts/jig "))
    );
    assert!(
        output["render_report"]["commands_detected_or_skipped"]
            .as_array()
            .unwrap()
            .iter()
            .all(|command| command.as_str().unwrap().contains("scripts/jig"))
    );
}

pub(super) fn assert_inferred_ownership(output: &serde_json::Value) {
    let profile = &output["adoption_profile"];
    assert!(
        profile["managed_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == ".jig.toml")
    );
    for path in ["scripts/check-agent-guides.sh", ".jig.toml"] {
        assert!(
            !profile["retired_managed_files"]
                .as_array()
                .unwrap()
                .iter()
                .any(|retired| retired == path)
        );
    }
    assert!(
        !profile["managed_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "scripts/check-agent-guides.sh")
    );
    assert!(
        profile["assumptions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|assumption| assumption
                .as_str()
                .unwrap()
                .contains("online cargo sqlx prepare"))
    );
}

pub(super) fn assert_inferred_config(repo: &Path) -> String {
    let answers = fs::read_to_string(repo.join(".jig.toml")).unwrap();
    for expected in [
        "repo_name = \"inferred-demo\"",
        "default_branch = \"main\"",
        "ci_github_runner = \"ubuntu-24.04\"",
        "sqlx_enabled = true",
        "rust_crate_roots = [\"crates\"]",
        "rust_migration_dir = \"migrations\"",
        "rust_sqlx_metadata_dir = \".sqlx\"",
        "schema_dump_enabled = false",
        "sqlx_check_command = ",
        "cargo sqlx prepare --check",
        "web_package_manager = \"pnpm\"",
        "[[frontend_apps]]",
        "name = \"web\"",
        "dir = \"web\"",
        "argv = [\"pnpm\", \"run\", \"dev\"]",
    ] {
        assert!(
            answers.contains(expected),
            "missing inferred config: {expected}"
        );
    }
    assert!(!answers.contains("schema_dump_command"));
    answers
}

pub(super) fn assert_rendered_work_gates(output: &serde_json::Value, answers: &str) {
    let generated_gates = output["adoption_profile"]["generated_gates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|gate| gate.as_str().unwrap())
        .collect::<Vec<_>>();
    let tools = answers.lines().filter_map(|line| {
        line.trim()
            .strip_prefix("tool = \"")
            .and_then(|value| value.strip_suffix('"'))
    });
    for tool in tools {
        let expected = match tool {
            "jig.contract_check" => "scripts/jig check contract",
            "jig.test" => "scripts/jig check test",
            "jig.typescript_lint" => "scripts/jig check typescript-lint",
            "jig.typescript_typecheck" => "scripts/jig check typescript-typecheck",
            "jig.typescript_build" => "scripts/jig check typescript-build",
            "jig.typescript_coverage" => "scripts/jig check typescript-coverage",
            "jig.sqlx_check" => "scripts/jig check sqlx",
            "jig.schema_check" => "scripts/jig check schema",
            "jig.schema_dump" => "scripts/jig sqlx schema dump",
            other => panic!("unmapped rendered work gate tool {other}"),
        };
        assert!(
            generated_gates.contains(&expected),
            "generated_gates missing rendered work gate command {expected}"
        );
    }
}
