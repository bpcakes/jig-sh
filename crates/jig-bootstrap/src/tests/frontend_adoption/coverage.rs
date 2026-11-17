use std::fs;
use std::process::Command;

use serde_json::{Value, json};

const CHECKER: &str =
    include_str!("../../embedded_template_snapshots/scripts/enforce-coverage.cjs.jinja");

fn check(summary: Option<Value>, threshold: &str, expected: bool) {
    check_role(summary, threshold, "spa", expected);
}

fn check_role(summary: Option<Value>, threshold: &str, role: &str, expected: bool) {
    let temp = tempfile::tempdir().unwrap();
    let script = temp.path().join("enforce-coverage.cjs");
    let report = temp.path().join("coverage-summary.json");
    fs::write(&script, CHECKER).unwrap();
    let original = summary.map(|value| serde_json::to_vec(&value).unwrap());
    if let Some(bytes) = &original {
        fs::write(&report, bytes).unwrap();
    }
    let output = Command::new("node")
        .arg(&script)
        .arg(role)
        .env("COVERAGE_DIR", temp.path())
        .env("COVERAGE_THRESHOLD", threshold)
        .output()
        .expect("Node is required for generated frontend tests");
    assert_eq!(
        output.status.success(),
        expected,
        "threshold={threshold}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(&report).ok(),
        original,
        "checking must not manufacture or modify coverage"
    );
}

fn summary(pct: Value) -> Value {
    json!({"total": {"lines": {"pct": pct}, "functions": {"pct": pct}, "statements": {"pct": pct}, "branches": {"pct": pct}}})
}

#[test]
fn coverage_requires_complete_numeric_measurements_even_at_zero_threshold() {
    for pct in [
        json!("not-a-number"),
        json!("80"),
        Value::Null,
        json!(true),
        json!(-1),
        json!(101),
    ] {
        check(Some(summary(pct)), "80", false);
    }
    for value in [Value::Null, json!({}), json!({"total": {}})] {
        check(Some(value), "0", false);
    }
    let mut incomplete = summary(json!(100));
    incomplete["total"]
        .as_object_mut()
        .unwrap()
        .remove("branches");
    check(Some(incomplete), "0", false);
    check(None, "0", false);
}

#[test]
fn coverage_validates_threshold_and_enforces_every_metric() {
    for threshold in ["", " ", "NaN", "Infinity", "-1", "101"] {
        check(Some(summary(json!(100))), threshold, false);
    }
    check(Some(summary(json!(80))), "80", true);
    check(Some(summary(json!(0))), "0", true);
    for metric in ["lines", "functions", "statements", "branches"] {
        let mut value = summary(json!(100));
        value["total"][metric]["pct"] = json!(79.9);
        check(Some(value), "80", false);
    }
}

#[test]
fn astro_coverage_exemption_is_explicit_and_only_at_zero_threshold() {
    check_role(None, "0", "astro", true);
    check_role(None, "80", "astro", false);
    check_role(None, "0", "spa", false);
    check_role(None, "NaN", "astro", false);
}

#[cfg(unix)]
#[test]
fn rendered_checker_passes_the_configured_astro_coverage_policy() {
    use crate::tests::{materialize_template_worktree, run_init};
    use crate::{AnswerOpts, FrontendApp, InitOpts, ScaffoldOpts};

    let temp = tempfile::tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("ExampleProject");
    let cases = [
        ("site", "astro", 0, true),
        ("tested-site", "astro", 80, false),
        ("web", "spa", 0, false),
    ];
    run_init(InitOpts {
        path: repo.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("ExampleProject".into()),
            sqlx_enabled: Some(false),
            web_package_manager: Some("npm".into()),
            frontend_apps: cases
                .iter()
                .map(|(name, role, threshold, _)| FrontendApp {
                    name: (*name).into(),
                    dir: format!("apps/{name}"),
                    coverage_threshold: *threshold,
                    kind: if *role == "astro" { "env-port" } else { "vite" }.into(),
                    role: (*role).into(),
                })
                .collect(),
            ..AnswerOpts::default()
        },
    })
    .unwrap();
    let run_checker = |args: &[&str]| {
        Command::new("bash")
            .arg("scripts/check-webapps.sh")
            .args(args)
            .current_dir(&repo)
            .env("npm_config_cache", temp.path().join("npm-cache"))
            .env("npm_config_audit", "false")
            .env("npm_config_fund", "false")
            .env("npm_config_offline", "true")
            .output()
            .unwrap()
    };
    for (name, _, _, expected) in cases {
        let app = repo.join("apps").join(name);
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("package.json"), serde_json::to_vec(&json!({
            "name": name, "version": "1.0.0", "private": true,
            "scripts": {"lint": "node -e ''", "typecheck": "node -e ''", "build:bundle": "node -e ''", "test:coverage": "node -e ''"}
        })).unwrap()).unwrap();
        let app_dir = format!("apps/{name}");
        let bootstrap = run_checker(&["dependencies-bootstrap", &app_dir]);
        assert!(
            bootstrap.status.success(),
            "{name}: {}\n{}",
            String::from_utf8_lossy(&bootstrap.stdout),
            String::from_utf8_lossy(&bootstrap.stderr)
        );
        let output = run_checker(&["app-check", &app_dir, "coverage"]);
        assert_eq!(
            output.status.success(),
            expected,
            "{name}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if expected {
            assert!(String::from_utf8_lossy(&output.stdout).contains("Coverage is disabled"));
        } else {
            assert!(String::from_utf8_lossy(&output.stderr).contains("Coverage check failed"));
        }
        assert!(!app.join("coverage/coverage-summary.json").exists());
    }
}
