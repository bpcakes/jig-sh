use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
#[cfg(unix)]
use std::process::Command;

use super::super::git::git;
use super::*;
use crate::bootstrap::adopt_infer::scan::{MAX_SCAN_DEPTH, MAX_SCAN_WARNINGS};

fn infer_sqlx(root: &Path, warnings: &mut Vec<String>) -> super::rust_sqlx::SqlxInference {
    let scan = RepoScan::collect(root, warnings);
    super::rust_sqlx::infer_sqlx(root, &scan, warnings)
}

fn infer_package_manager(root: &Path, warnings: &mut Vec<String>) -> Option<String> {
    let scan = RepoScan::collect(root, warnings);
    super::package_manager::infer_package_manager(root, &scan, warnings)
}

fn signal_values(value: &JsonValue) -> Vec<&str> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["value"].as_str().unwrap())
        .collect()
}

#[test]
fn parses_remote_repo_names() {
    assert_eq!(
        repo_name_from_remote_url("git@github.com:owner/demo.git").as_deref(),
        Some("demo")
    );
    assert_eq!(
        repo_name_from_remote_url("https://github.com/owner/demo").as_deref(),
        Some("demo")
    );
    assert_eq!(
        repo_name_from_remote_url("ssh://git@example.com:2222/owner/demo.git").as_deref(),
        Some("demo")
    );
    assert_eq!(
        repo_name_from_remote_url("git@github.com:owner/my.app.git").as_deref(),
        Some("my.app")
    );
}

#[test]
fn remote_repo_name_preserves_dots() {
    let _guard = crate::test_env::lock_env();
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), ["init"]).unwrap();
    git(
        temp.path(),
        ["remote", "add", "origin", "git@github.com:owner/my.app.git"],
    )
    .unwrap();

    assert_eq!(infer_repo_name(temp.path()).as_deref(), Some("my.app"));
}

#[test]
fn fallback_repo_name_is_sanitized() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("Demo App.v2");
    fs::create_dir(&repo).unwrap();

    assert_eq!(infer_repo_name(&repo).as_deref(), Some("Demo-App-v2"));
    assert_eq!(safe_repo_name("@@@"), "repo");
}

#[test]
fn inferred_sqlx_enabled_predicate_respects_explicit_shapes() {
    let mut answers = AnswerOpts::default();
    let empty_shape = AnswerInputShape::default();
    assert!(empty_shape.should_apply_inferred_sqlx_enabled(&answers));

    answers.rust_migration_dir = Some("migrations".into());
    assert!(!empty_shape.should_apply_inferred_sqlx_enabled(&answers));
    answers.rust_migration_dir = None;

    let shape = answer_shape_from_keys(["sqlx_check_command"]);
    assert!(!shape.should_apply_inferred_sqlx_enabled(&answers));
    let shape = answer_shape_from_keys(["schema_dump_command"]);
    assert!(!shape.should_apply_inferred_sqlx_enabled(&answers));
    answers.migration_add_command = Some("scripts/new-migration.sh".into());
    assert!(!empty_shape.should_apply_inferred_sqlx_enabled(&answers));
    answers.migration_add_command = None;

    let shape = answer_shape_from_key_values([("schema_dump_enabled", true)]);
    assert!(!shape.should_apply_inferred_sqlx_enabled(&answers));
    let shape = answer_shape_from_key_values([("schema_dump_enabled", false)]);
    assert!(shape.should_apply_inferred_sqlx_enabled(&answers));
}

fn answer_shape_from_keys(keys: impl IntoIterator<Item = &'static str>) -> AnswerInputShape {
    let table = keys
        .into_iter()
        .map(|key| (key.to_string(), toml::Value::String(String::new())))
        .collect();
    AnswerInputShape::from_table(&table)
}

fn answer_shape_from_key_values(
    pairs: impl IntoIterator<Item = (&'static str, bool)>,
) -> AnswerInputShape {
    let table = pairs
        .into_iter()
        .map(|(key, value)| (key.to_string(), toml::Value::Boolean(value)))
        .collect();
    AnswerInputShape::from_table(&table)
}

#[test]
fn scan_warnings_are_capped_with_omission_notice() {
    let temp = tempfile::tempdir().unwrap();
    let mut warnings = Vec::new();

    for _ in 0..(MAX_SCAN_WARNINGS + 5) {
        push_scan_warning(&mut warnings, temp.path(), "synthetic warning");
    }

    assert_eq!(warnings.len(), MAX_SCAN_WARNINGS);
    assert_eq!(
        warnings.last().map(String::as_str),
        Some("additional inference scan warnings omitted")
    );
}

#[test]
fn generated_dependency_dirs_are_skipped_without_depth_warnings() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(
            temp.path()
                .join("terraform/environments/production/.terraform/providers/registry.terraform.io/hashicorp/aws"),
        )
        .unwrap();
    fs::create_dir_all(
        temp.path()
            .join("Perdify-iOS/PerdifyGRPC/.build/checkouts/swift-nio-transport-services/Sources"),
    )
    .unwrap();

    let mut warnings = Vec::new();
    let scan = RepoScan::collect(temp.path(), &mut warnings);

    assert!(
        !scan
            .dirs_named("registry.terraform.io")
            .any(|path| path.ends_with("registry.terraform.io")),
        "expected .terraform provider tree to be skipped"
    );
    assert!(
        !scan
            .dirs_named("checkouts")
            .any(|path| path.ends_with("checkouts")),
        "expected SwiftPM .build tree to be skipped"
    );
    assert!(
        warnings
            .iter()
            .all(|warning| !warning.contains("maximum inference scan depth reached")),
        "generated dependency dirs should not emit depth warnings: {warnings:?}"
    );
}

#[test]
fn gitignored_paths_are_excluded_from_repo_scan() {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), ["init"]).unwrap();
    fs::write(temp.path().join(".gitignore"), "ignored/\n").unwrap();
    fs::create_dir_all(temp.path().join("ignored/deep")).unwrap();
    fs::create_dir_all(temp.path().join("visible")).unwrap();
    fs::write(temp.path().join("ignored/deep/package.json"), "{}").unwrap();
    fs::write(temp.path().join("visible/package.json"), "{}").unwrap();

    let mut warnings = Vec::new();
    let scan = RepoScan::collect(temp.path(), &mut warnings);

    let package_paths = scan
        .named_files("package.json")
        .map(|path| {
            path.strip_prefix(temp.path())
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(package_paths, vec!["visible/package.json"]);
    assert!(
        warnings
            .iter()
            .all(|warning| !warning.contains("maximum inference scan depth reached")),
        "gitignored paths should not consume depth budget: {warnings:?}"
    );
}

#[test]
fn scan_depth_warnings_are_reported_once() {
    let temp = tempfile::tempdir().unwrap();
    for branch in ["left", "right"] {
        let mut path = temp.path().join(branch);
        for level in 0..(MAX_SCAN_DEPTH + 2) {
            path = path.join(format!("level-{level}"));
        }
        fs::create_dir_all(path).unwrap();
    }

    let mut warnings = Vec::new();
    RepoScan::collect(temp.path(), &mut warnings);

    assert_eq!(
        warnings
            .iter()
            .filter(|warning| warning.contains("maximum inference scan depth reached"))
            .count(),
        1,
        "expected one depth warning, got {warnings:?}"
    );
}

#[test]
fn crate_roots_follow_workspace_member_parents() {
    assert_eq!(
        crate_root_from_workspace_member("crates/*").as_deref(),
        Some("crates")
    );
    assert_eq!(
        crate_root_from_workspace_member("apps/api").as_deref(),
        Some("apps")
    );
    assert_eq!(crate_root_from_workspace_member(".").as_deref(), Some("."));
}

#[test]
fn single_crate_root_is_inferred_as_repo_root() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    let mut warnings = Vec::new();

    assert_eq!(
        infer_rust_crate_roots(temp.path(), &mut warnings),
        vec!["."]
    );
}

#[test]
fn workspace_without_usable_members_reports_workspace_source() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    let mut warnings = Vec::new();

    let inference = infer_rust_crate_roots_with_metadata(temp.path(), &mut warnings);

    assert_eq!(inference.roots, vec!["."]);
    assert_eq!(
        inference.sources,
        vec!["Cargo.toml [workspace] (no usable workspace members)"]
    );

    let inference = infer_adopt_answers(temp.path());
    assert_eq!(
        inference
            .metadata
            .get("rust_crate_roots")
            .unwrap()
            .confidence
            .as_str(),
        "low"
    );
}

#[test]
fn single_crate_repo_uses_crate_stack_label() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.detected_stack_label(), "Rust crate");
    assert!(inference.summary().contains("Rust crate (.)"));
    assert_eq!(
        inference.report()["metadata"]["rust_crate_roots"]["sources"][0],
        "Cargo.toml [package]"
    );
}

#[test]
fn nested_crates_without_root_workspace_are_inferred_from_scan() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("crates/api/src")).unwrap();
    fs::write(
        temp.path().join("crates/api/Cargo.toml"),
        r#"[package]
name = "api"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(temp.path().join("crates/api/src/lib.rs"), "").unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.rust_crate_roots, vec!["crates"]);
    assert_eq!(
        inference.rust_crate_root_source_kind,
        RustCrateRootSourceKind::ScannedPackages
    );
    assert!(inference.summary().contains("Rust crate (crates)"));
    assert!(
        inference
            .rust_test_command
            .as_deref()
            .unwrap()
            .contains("jig_manifest=crates/api/Cargo.toml")
    );
    assert!(
        inference
            .adoption_review(
                &AnswerOpts::default(),
                &AnswerOpts::default(),
                &AnswerInputShape::default()
            )
            .items
            .iter()
            .any(|item| item.contains("nested crates detected without a root Cargo.toml"))
    );
}

#[test]
fn depth_one_nested_crate_without_root_workspace_uses_repo_root_scan() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("api/src")).unwrap();
    fs::create_dir_all(temp.path().join("examples/demo/src")).unwrap();
    fs::write(
        temp.path().join("api/Cargo.toml"),
        r#"[package]
name = "api"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(temp.path().join("api/src/lib.rs"), "").unwrap();
    fs::write(
        temp.path().join("examples/demo/Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(temp.path().join("examples/demo/src/lib.rs"), "").unwrap();

    let inference = infer_adopt_answers(temp.path());
    let command = inference.rust_test_command.as_deref().unwrap();

    assert_eq!(inference.rust_crate_roots, vec!["."]);
    assert!(command.contains("jig_manifest=api/Cargo.toml"));
    assert!(!command.contains("examples/demo/Cargo.toml"));
    assert!(
        !inference
            .rust_clippy_command
            .as_deref()
            .unwrap()
            .contains("--locked")
    );
    assert!(inference.rust_test_locked_command.is_none());
    assert!(
        inference
            .warnings
            .iter()
            .any(|warning| warning.contains("did not infer rust_test_locked_command"))
    );
}

#[test]
fn nested_crate_scan_filters_non_production_package_names() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("api/src")).unwrap();
    fs::create_dir_all(temp.path().join("svc/src")).unwrap();
    fs::write(
        temp.path().join("api/Cargo.toml"),
        r#"[package]
name = "fixture"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(temp.path().join("api/src/lib.rs"), "").unwrap();
    fs::write(
        temp.path().join("svc/Cargo.toml"),
        r#"[package]
name = "svc"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(temp.path().join("svc/src/lib.rs"), "").unwrap();

    let inference = infer_adopt_answers(temp.path());
    let command = inference.rust_test_command.as_deref().unwrap();

    assert_eq!(inference.rust_crate_roots, vec!["."]);
    assert!(command.contains("jig_manifest=svc/Cargo.toml"));
    assert!(!command.contains("api/Cargo.toml"));
}

#[test]
fn mixed_depth_nested_crate_scan_collapses_roots_to_repo_root() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("api/src")).unwrap();
    fs::create_dir_all(temp.path().join("crates/worker/src")).unwrap();
    fs::write(
        temp.path().join("api/Cargo.toml"),
        r#"[package]
name = "api"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(temp.path().join("api/src/lib.rs"), "").unwrap();
    fs::write(
        temp.path().join("crates/worker/Cargo.toml"),
        r#"[package]
name = "worker"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(temp.path().join("crates/worker/src/lib.rs"), "").unwrap();

    let inference = infer_adopt_answers(temp.path());
    let command = inference.rust_test_command.as_deref().unwrap();

    assert_eq!(inference.rust_crate_roots, vec!["."]);
    assert!(command.contains("jig_manifest=api/Cargo.toml"));
    assert!(command.contains("jig_manifest=crates/worker/Cargo.toml"));
}

#[test]
fn deeply_nested_crate_scan_uses_parent_directories_as_roots() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("services/auth/api/src")).unwrap();
    fs::create_dir_all(temp.path().join("services/billing/api/src")).unwrap();
    fs::write(
        temp.path().join("services/auth/api/Cargo.toml"),
        r#"[package]
name = "auth-api"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(temp.path().join("services/auth/api/src/lib.rs"), "").unwrap();
    fs::write(
        temp.path().join("services/billing/api/Cargo.toml"),
        r#"[package]
name = "billing-api"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(temp.path().join("services/billing/api/src/lib.rs"), "").unwrap();

    let inference = infer_adopt_answers(temp.path());
    let command = inference.rust_test_command.as_deref().unwrap();

    assert_eq!(
        inference.rust_crate_roots,
        vec!["services/auth", "services/billing"]
    );
    assert!(command.contains("jig_manifest=services/auth/api/Cargo.toml"));
    assert!(command.contains("jig_manifest=services/billing/api/Cargo.toml"));
}

#[test]
fn nested_crate_review_notes_when_wrapper_commands_take_precedence() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("api/src")).unwrap();
    fs::write(
        temp.path().join("api/Cargo.toml"),
        r#"[package]
name = "api"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(temp.path().join("api/src/lib.rs"), "").unwrap();
    fs::write(
            temp.path().join("Makefile"),
            "fmt-check:\n\tcargo fmt -- --check\nclippy:\n\tcargo clippy\ntest:\n\tcargo test\ntest-locked:\n\tcargo test --locked\n",
        )
        .unwrap();

    let inference = infer_adopt_answers(temp.path());
    let review = inference.adoption_review(
        &AnswerOpts::default(),
        &AnswerOpts::default(),
        &AnswerInputShape::default(),
    );

    assert_eq!(inference.rust_test_command.as_deref(), Some("make test"));
    assert!(
        review
            .items
            .iter()
            .any(|item| item.contains("wrapper Rust commands took precedence"))
    );
    assert!(
        !review
            .items
            .iter()
            .any(|item| item.contains("generated Rust commands cover inferred manifests"))
    );
    assert!(
        !inference
            .warnings
            .iter()
            .any(|warning| warning.contains("did not infer rust_test_locked_command"))
    );
}

#[test]
#[cfg(unix)]
fn nested_manifest_command_preserves_failing_cargo_status() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("crates/api/src")).unwrap();
    fs::write(
        temp.path().join("crates/api/Cargo.toml"),
        r#"[package]
name = "api"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(temp.path().join("crates/api/src/lib.rs"), "").unwrap();
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let cargo_path = bin_dir.join("cargo");
    fs::write(&cargo_path, "#!/bin/sh\nexit 42\n").unwrap();
    fs::set_permissions(&cargo_path, fs::Permissions::from_mode(0o755)).unwrap();

    let inference = infer_adopt_answers(temp.path());
    let existing_path = std::env::var_os("PATH").unwrap_or_default();
    let path = format!("{}:{}", bin_dir.display(), existing_path.to_string_lossy());
    let status = Command::new("sh")
        .arg("-c")
        .arg(inference.rust_test_command.as_deref().unwrap())
        .current_dir(temp.path())
        .env("PATH", path)
        .status()
        .unwrap();

    assert_eq!(status.code(), Some(42));
}

#[test]
fn nested_fixture_crates_without_root_workspace_are_not_inferred_as_app_roots() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("tests/fixtures/demo/src")).unwrap();
    fs::write(
        temp.path().join("tests/fixtures/demo/Cargo.toml"),
        r#"[package]
name = "demo-fixture"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(temp.path().join("tests/fixtures/demo/src/lib.rs"), "").unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert!(inference.rust_crate_roots.is_empty());
    assert_eq!(
        inference.rust_crate_root_source_kind,
        RustCrateRootSourceKind::None
    );
    assert_eq!(
        inference.detected_stack_label(),
        "no application stack detected"
    );
}

#[test]
fn workspace_glob_segment_match_supports_multiple_stars() {
    assert!(segment_matches("*-app-*", "demo-app-web"));
    assert!(segment_matches("app-*-web", "app-demo-web"));
    assert!(!segment_matches("app-*-web", "app-demo-api"));
}

#[test]
fn sqlx_detection_includes_cargo_sqlx_commands() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
    fs::write(
        temp.path().join(".github/workflows/sqlx.yml"),
        "steps:\n  - run: cargo sqlx prepare --check\n",
    )
    .unwrap();

    let mut warnings = Vec::new();
    let sqlx = infer_sqlx(temp.path(), &mut warnings);

    assert!(sqlx.enabled.value);
    assert_eq!(
        sqlx.migration_dir
            .as_ref()
            .map(|value| value.value.as_str()),
        Some("migrations")
    );
    assert_eq!(
        sqlx.check_command
            .as_ref()
            .map(|value| value.value.as_str()),
        Some(
            "SQLX_OFFLINE=false SQLX_OFFLINE_DIR='.sqlx' cargo sqlx prepare --check -- --all-targets"
        )
    );
    assert!(
        sqlx.signals
            .iter()
            .any(|signal| signal == "cargo sqlx command")
    );
}

#[test]
fn sqlx_check_command_uses_workspace_flag_for_cargo_workspaces() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[workspace]
members = ["crates/*"]

[workspace.dependencies]
sqlx = "0.8"
"#,
    )
    .unwrap();

    let mut warnings = Vec::new();
    let sqlx = infer_sqlx(temp.path(), &mut warnings);

    assert_eq!(
        sqlx.check_command
            .as_ref()
            .map(|value| value.value.as_str()),
        Some(
            "SQLX_OFFLINE=false SQLX_OFFLINE_DIR='.sqlx' cargo sqlx prepare --check --workspace -- --all-targets"
        )
    );
}

#[test]
fn sqlx_detection_ignores_benign_cargo_sqlx_mentions() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::write(
        temp.path().join("src/lib.rs"),
        "/// Example: sqlx::migrate!();\n// sqlx::migrate!();\n/* sqlx::migrate!(); */\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("notes.toml"),
        "# run cargo sqlx prepare manually if needed\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("script.sh"),
        "# cargo sqlx prepare --check\nnpm test # cargo sqlx prepare --check\n",
    )
    .unwrap();
    fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
    fs::write(
        temp.path().join(".github/workflows/test.yml"),
        "steps:\n  - run: npm test # cargo sqlx prepare --check\n",
    )
    .unwrap();

    let mut warnings = Vec::new();
    let sqlx = infer_sqlx(temp.path(), &mut warnings);

    assert!(!sqlx.enabled.value);
    assert!(
        sqlx.signals
            .iter()
            .any(|signal| { signal.contains("no SQLx signals detected") })
    );
}

#[test]
fn root_named_like_skipped_dir_is_still_scanned() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("target");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();

    let inference = infer_adopt_answers(&root);

    assert_eq!(inference.rust_crate_roots, vec!["."]);
}

#[test]
fn nested_package_manager_conflicts_are_reported_as_warnings() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::create_dir_all(temp.path().join("packages/api")).unwrap();
    fs::write(temp.path().join("apps/web/package-lock.json"), "{}").unwrap();
    fs::write(temp.path().join("packages/api/pnpm-lock.yaml"), "").unwrap();

    let mut warnings = Vec::new();
    let manager = infer_package_manager(temp.path(), &mut warnings);

    assert_eq!(manager.as_deref(), Some("npm"));
    assert!(
        warnings
            .iter()
            .any(|warning| { warning.contains("multiple package manager lockfiles detected") })
    );
}

#[test]
fn root_package_manager_conflicts_are_reported_as_warnings() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("bun.lock"), "").unwrap();
    fs::write(temp.path().join("package-lock.json"), "{}").unwrap();

    let mut warnings = Vec::new();
    let manager = infer_package_manager(temp.path(), &mut warnings);

    assert_eq!(manager.as_deref(), Some("bun"));
    assert!(
        warnings.iter().any(|warning| {
            warning.contains("multiple root package manager lockfiles detected")
        })
    );
}

#[test]
fn npm_shrinkwrap_is_inferred_and_precedes_package_lock() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("npm-shrinkwrap.json"), "{}").unwrap();

    let mut warnings = Vec::new();
    let scan = RepoScan::collect(temp.path(), &mut warnings);
    let inference = super::package_manager::infer_package_manager_with_metadata(
        temp.path(),
        &scan,
        &mut warnings,
    );

    assert_eq!(inference.value.as_deref(), Some("npm"));
    assert_eq!(inference.sources, vec!["npm-shrinkwrap.json"]);
    assert!(warnings.is_empty());

    fs::write(temp.path().join("package-lock.json"), "{}").unwrap();
    let scan = RepoScan::collect(temp.path(), &mut warnings);
    let inference = super::package_manager::infer_package_manager_with_metadata(
        temp.path(),
        &scan,
        &mut warnings,
    );
    assert_eq!(inference.value.as_deref(), Some("npm"));
    assert_eq!(inference.sources, vec!["npm-shrinkwrap.json"]);

    fs::remove_file(temp.path().join("npm-shrinkwrap.json")).unwrap();
    let scan = RepoScan::collect(temp.path(), &mut warnings);
    let inference = super::package_manager::infer_package_manager_with_metadata(
        temp.path(),
        &scan,
        &mut warnings,
    );
    assert_eq!(inference.value.as_deref(), Some("npm"));
    assert_eq!(inference.sources, vec!["package-lock.json"]);

    fs::remove_file(temp.path().join("package-lock.json")).unwrap();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::write(temp.path().join("apps/web/npm-shrinkwrap.json"), "{}").unwrap();
    let scan = RepoScan::collect(temp.path(), &mut warnings);
    let inference = super::package_manager::infer_package_manager_with_metadata(
        temp.path(),
        &scan,
        &mut warnings,
    );
    assert_eq!(inference.value.as_deref(), Some("npm"));
    assert_eq!(inference.sources, vec!["apps/web/npm-shrinkwrap.json"]);
}

#[test]
fn default_branch_prefers_known_origin_refs_over_current_branch() {
    let _guard = crate::test_env::lock_env();
    let temp = tempfile::tempdir().unwrap();
    let global_config = temp.path().join("global-gitconfig");
    fs::write(&global_config, "").unwrap();
    let _global_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_GLOBAL", global_config);
    let _no_system_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_NOSYSTEM", "1");
    git(temp.path(), ["init", "-b", "feature"]).unwrap();
    git(temp.path(), ["config", "user.name", "Fixture"]).unwrap();
    git(temp.path(), ["config", "user.email", "fixture@example.com"]).unwrap();
    fs::write(temp.path().join("README.md"), "demo\n").unwrap();
    git(temp.path(), ["add", "README.md"]).unwrap();
    git(temp.path(), ["commit", "-m", "init"]).unwrap();
    git(
        temp.path(),
        ["update-ref", "refs/remotes/origin/main", "HEAD"],
    )
    .unwrap();

    let mut warnings = Vec::new();
    assert_eq!(
        infer_default_branch(temp.path(), &mut warnings).as_deref(),
        Some("main")
    );
}

#[test]
fn default_branch_warns_when_multiple_origin_candidates_exist() {
    let _guard = crate::test_env::lock_env();
    let temp = tempfile::tempdir().unwrap();
    let global_config = temp.path().join("global-gitconfig");
    fs::write(&global_config, "").unwrap();
    let _global_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_GLOBAL", global_config);
    let _no_system_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_NOSYSTEM", "1");
    git(temp.path(), ["init", "-b", "feature"]).unwrap();
    git(temp.path(), ["config", "user.name", "Fixture"]).unwrap();
    git(temp.path(), ["config", "user.email", "fixture@example.com"]).unwrap();
    fs::write(temp.path().join("README.md"), "demo\n").unwrap();
    git(temp.path(), ["add", "README.md"]).unwrap();
    git(temp.path(), ["commit", "-m", "init"]).unwrap();
    git(
        temp.path(),
        ["update-ref", "refs/remotes/origin/main", "HEAD"],
    )
    .unwrap();
    git(
        temp.path(),
        ["update-ref", "refs/remotes/origin/master", "HEAD"],
    )
    .unwrap();

    let mut warnings = Vec::new();
    assert_eq!(
        infer_default_branch(temp.path(), &mut warnings).as_deref(),
        Some("main")
    );
    assert!(
        warnings.iter().any(|warning| {
            warning.contains("multiple origin default branch candidates detected")
        })
    );
}

#[test]
fn default_branch_does_not_infer_unknown_current_branch() {
    let _guard = crate::test_env::lock_env();
    let temp = tempfile::tempdir().unwrap();
    let global_config = temp.path().join("global-gitconfig");
    fs::write(&global_config, "").unwrap();
    let _global_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_GLOBAL", global_config);
    let _no_system_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_NOSYSTEM", "1");
    git(temp.path(), ["init", "-b", "feature"]).unwrap();
    git(temp.path(), ["config", "user.name", "Fixture"]).unwrap();
    git(temp.path(), ["config", "user.email", "fixture@example.com"]).unwrap();
    fs::write(temp.path().join("README.md"), "demo\n").unwrap();
    git(temp.path(), ["add", "README.md"]).unwrap();
    git(temp.path(), ["commit", "-m", "init"]).unwrap();

    let mut warnings = Vec::new();
    assert_eq!(infer_default_branch(temp.path(), &mut warnings), None);
    assert!(warnings.iter().any(|warning| {
        warning.contains("current branch feature is not a known default branch name")
    }));
}

#[test]
fn default_branch_infers_known_local_head_without_origin() {
    let _guard = crate::test_env::lock_env();
    let temp = tempfile::tempdir().unwrap();
    let global_config = temp.path().join("global-gitconfig");
    fs::write(&global_config, "").unwrap();
    let _global_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_GLOBAL", global_config);
    let _no_system_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_NOSYSTEM", "1");
    git(temp.path(), ["init", "-b", "main"]).unwrap();
    git(temp.path(), ["config", "user.name", "Fixture"]).unwrap();
    git(temp.path(), ["config", "user.email", "fixture@example.com"]).unwrap();
    fs::write(temp.path().join("README.md"), "demo\n").unwrap();
    git(temp.path(), ["add", "README.md"]).unwrap();
    git(temp.path(), ["commit", "-m", "init"]).unwrap();

    let mut warnings = Vec::new();
    assert_eq!(
        infer_default_branch(temp.path(), &mut warnings).as_deref(),
        Some("main")
    );
}

#[test]
fn default_branch_ignores_malformed_origin_head() {
    let _guard = crate::test_env::lock_env();
    let temp = tempfile::tempdir().unwrap();
    let global_config = temp.path().join("global-gitconfig");
    fs::write(&global_config, "").unwrap();
    let _global_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_GLOBAL", global_config);
    let _no_system_config = crate::test_env::EnvVarGuard::set("GIT_CONFIG_NOSYSTEM", "1");
    git(temp.path(), ["init", "-b", "feature"]).unwrap();
    git(temp.path(), ["config", "user.name", "Fixture"]).unwrap();
    git(temp.path(), ["config", "user.email", "fixture@example.com"]).unwrap();
    fs::write(temp.path().join("README.md"), "demo\n").unwrap();
    git(temp.path(), ["add", "README.md"]).unwrap();
    git(temp.path(), ["commit", "-m", "init"]).unwrap();
    git(
        temp.path(),
        [
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/heads/feature",
        ],
    )
    .unwrap();

    let mut warnings = Vec::new();
    assert_eq!(infer_default_branch(temp.path(), &mut warnings), None);
}

#[test]
fn sqlx_detection_reports_nested_and_multiple_migration_dirs() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("crates/api/migrations/20240101_init")).unwrap();
    fs::create_dir_all(temp.path().join("services/billing/migrations")).unwrap();
    fs::write(
        temp.path()
            .join("crates/api/migrations/20240101_init/up.sql"),
        "select 1;",
    )
    .unwrap();
    fs::write(
        temp.path().join("services/billing/migrations/0001.sql"),
        "select 1;",
    )
    .unwrap();

    let mut warnings = Vec::new();
    let sqlx = infer_sqlx(temp.path(), &mut warnings);

    assert!(sqlx.enabled.value);
    assert_eq!(
        sqlx.migration_dirs.value,
        vec![
            "crates/api/migrations".to_string(),
            "services/billing/migrations".to_string(),
        ]
    );
    assert_eq!(
        sqlx.migration_dir
            .as_ref()
            .map(|value| value.value.as_str()),
        Some("crates/api/migrations")
    );
    assert!(sqlx.signals.iter().any(|signal| {
        signal
            == "migration directories detected: crates/api/migrations, services/billing/migrations"
    }));
    assert!(warnings.iter().any(|warning| {
        warning.contains("multiple migration directories detected")
            && warning.contains("crates/api/migrations")
    }));
}

#[test]
fn migration_dir_ignores_non_migration_sql_snippets() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("migrations/archive")).unwrap();
    fs::write(
        temp.path().join("migrations/archive/old_dump.sql"),
        "select 1;",
    )
    .unwrap();
    fs::write(temp.path().join("migrations/README.sql"), "notes").unwrap();

    let mut warnings = Vec::new();
    let sqlx = infer_sqlx(temp.path(), &mut warnings);

    assert!(!sqlx.enabled.value);
    assert!(sqlx.migration_dirs.value.is_empty());
}

#[test]
fn missing_workspace_glob_is_a_warning_not_a_failure() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("package.json"),
        r#"{"workspaces":["missing/*"]}"#,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert!(inference.frontend_apps.is_empty());
    assert!(
        inference
            .warnings
            .iter()
            .any(|warning| warning.contains("could not read directory")),
        "expected scan warning, got {:?}",
        inference.warnings
    );
}

#[test]
fn absent_optional_frontend_workspace_conventions_do_not_warn() {
    let temp = tempfile::tempdir().unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert!(inference.frontend_apps.is_empty());
    assert!(
        inference.warnings.iter().all(|warning| {
            !warning.contains("pnpm-workspace.yaml")
                && !warning.contains("could not read directory")
        }),
        "absent optional frontend conventions should stay quiet: {:?}",
        inference.warnings
    );
}

#[test]
fn empty_pnpm_workspace_is_reported_as_warning() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("pnpm-workspace.yaml"), "packages:\n").unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert!(inference.warnings.iter().any(|warning| {
        warning.contains("pnpm-workspace.yaml did not declare supported packages globs")
    }));
}

#[test]
fn pnpm_workspace_flow_style_globs_are_supported() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::create_dir_all(temp.path().join("fixtures/demo")).unwrap();
    fs::write(
        temp.path().join("pnpm-workspace.yaml"),
        "packages: [\"apps/*\"]\n",
    )
    .unwrap();
    let app_package = r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#;
    fs::write(temp.path().join("apps/web/package.json"), app_package).unwrap();
    fs::write(temp.path().join("fixtures/demo/package.json"), app_package).unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.frontend_apps.len(), 1);
    assert_eq!(inference.frontend_apps[0].dir, "apps/web");
}

#[test]
fn frontend_roles_distinguish_astro_from_other_non_vite_apps() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("apps/docs")).unwrap();
    fs::create_dir_all(temp.path().join("apps/storefront")).unwrap();
    fs::write(
        temp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - apps/*\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("apps/docs/package.json"),
        r#"{
  "scripts": {
    "dev": "astro dev",
    "lint": "eslint .",
    "typecheck": "astro check",
    "build:bundle": "astro build",
    "test:coverage": "vitest run --coverage"
  },
  "devDependencies": { "astro": "^5.0.0" }
}"#,
    )
    .unwrap();
    fs::write(
        temp.path().join("apps/storefront/package.json"),
        r#"{
  "scripts": {
    "dev": "next dev",
    "lint": "next lint",
    "typecheck": "tsc --noEmit",
    "build:bundle": "next build",
    "test:coverage": "vitest run --coverage"
  },
  "dependencies": { "next": "^15.0.0" }
}"#,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.frontend_apps.len(), 2);
    assert_eq!(inference.frontend_apps[0].dir, "apps/docs");
    assert_eq!(inference.frontend_apps[0].kind, "env-port");
    assert_eq!(inference.frontend_apps[0].role, "astro");
    assert_eq!(inference.frontend_apps[1].dir, "apps/storefront");
    assert_eq!(inference.frontend_apps[1].kind, "env-port");
    assert_eq!(inference.frontend_apps[1].role, "spa");
}

#[test]
fn legacy_admin_panel_vite_package_infers_the_shared_admin_role() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("apps/admin-panel")).unwrap();
    fs::write(
        temp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - apps/*\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("apps/admin-panel/package.json"),
        r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.frontend_apps.len(), 1);
    assert_eq!(inference.frontend_apps[0].name, "admin-panel");
    assert_eq!(inference.frontend_apps[0].kind, "vite");
    assert_eq!(inference.frontend_apps[0].role, "admin");
}

#[test]
fn frontend_role_inference_is_dev_script_authoritative() {
    let temp = tempfile::tempdir().unwrap();
    for app in ["vite-with-astro", "astro-script", "astro-fallback"] {
        fs::create_dir_all(temp.path().join("apps").join(app)).unwrap();
    }
    fs::write(
        temp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - apps/*\n",
    )
    .unwrap();

    let package = |dev: &str, dependencies: &str| {
        format!(
            r#"{{
  "scripts": {{
    "dev": "{dev}",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "build",
    "test:coverage": "vitest run --coverage"
  }},
  "dependencies": {dependencies}
}}"#
        )
    };
    fs::write(
        temp.path().join("apps/vite-with-astro/package.json"),
        package("vite", r#"{ "astro": "^5.0.0" }"#),
    )
    .unwrap();
    fs::write(
        temp.path().join("apps/astro-script/package.json"),
        package("astro dev", "{}"),
    )
    .unwrap();
    fs::write(
        temp.path().join("apps/astro-fallback/package.json"),
        package("custom-dev-server", r#"{ "astro": "^5.0.0" }"#),
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());
    let app = |dir: &str| {
        inference
            .frontend_apps
            .iter()
            .find(|app| app.dir == dir)
            .unwrap()
    };

    assert_eq!(app("apps/vite-with-astro").kind, "vite");
    assert_eq!(app("apps/vite-with-astro").role, "spa");
    assert_eq!(app("apps/astro-script").kind, "env-port");
    assert_eq!(app("apps/astro-script").role, "astro");
    assert_eq!(app("apps/astro-fallback").kind, "env-port");
    assert_eq!(app("apps/astro-fallback").role, "spa");
}

#[test]
fn frontend_profiles_include_preferred_dev_ports_from_scripts() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("apps/admin")).unwrap();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::write(
        temp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - apps/*\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("apps/admin/package.json"),
        r#"{
  "scripts": {
    "dev": "cross-env PORT=3001 vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#,
    )
    .unwrap();
    fs::write(
        temp.path().join("apps/web/package.json"),
        r#"{
  "scripts": {
    "dev": "vite --host 127.0.0.1 --port=5174",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#,
    )
    .unwrap();

    let report = infer_adopt_answers(temp.path()).report();
    let profiles = report["frontend_profiles"].as_array().unwrap();

    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0]["dir"], "apps/admin");
    assert_eq!(profiles[0]["preferred_dev_port"], 3001);
    assert_eq!(profiles[1]["dir"], "apps/web");
    assert_eq!(profiles[1]["preferred_dev_port"], 5174);
    assert_eq!(
        report["metadata"]["frontend_profiles"]["confidence"],
        "medium"
    );
}

#[test]
fn invalid_numeric_frontend_dev_ports_are_reported_as_warnings() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("web")).unwrap();
    fs::write(
        temp.path().join("web/package.json"),
        r#"{
  "scripts": {
    "dev": "vite --port=999999",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert!(
        inference
            .warnings()
            .iter()
            .any(|warning| warning.contains("preferred_dev_port was not inferred")),
        "expected invalid frontend dev-port warning, got {:?}",
        inference.warnings()
    );
    assert_eq!(inference.frontend_profiles[0].preferred_dev_port, None);
    assert!(
        inference.report()["metadata"]["frontend_profiles"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning
                .as_str()
                .unwrap()
                .contains("preferred_dev_port was not inferred"))
    );
}

#[test]
fn frontend_dev_port_scan_continues_after_invalid_literal() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("web")).unwrap();
    fs::write(
        temp.path().join("web/package.json"),
        r#"{
  "scripts": {
    "dev": "vite --port 999999 --port 5174",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(
        inference.frontend_profiles[0].preferred_dev_port,
        Some(5174)
    );
    assert!(
        inference
            .warnings()
            .iter()
            .any(|warning| warning.contains("preferred_dev_port was not inferred"))
    );
}

#[test]
fn frontend_packages_missing_ci_scripts_are_reported_as_warnings() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("web")).unwrap();
    fs::write(
        temp.path().join("web/package.json"),
        r#"{"scripts":{"dev":"vite","lint":"eslint ."}}"#,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert!(inference.frontend_apps.is_empty());
    assert!(inference.warnings.iter().any(|warning| {
        warning.contains("missing required CI scripts")
            && warning.contains("typecheck")
            && warning.contains("build:bundle")
            && warning.contains("test:coverage")
    }));
}

#[test]
fn fallback_frontend_scan_ignores_non_conventional_package_dirs() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("examples/demo")).unwrap();
    fs::write(
        temp.path().join("examples/demo/package.json"),
        r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert!(inference.frontend_apps.is_empty());
}

#[test]
fn declared_workspaces_limit_frontend_app_candidates() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::create_dir_all(temp.path().join("fixtures/demo")).unwrap();
    fs::write(
        temp.path().join("package.json"),
        r#"{"private":true,"workspaces":["apps/*"]}"#,
    )
    .unwrap();
    let app_package = r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#;
    fs::write(temp.path().join("apps/web/package.json"), app_package).unwrap();
    fs::write(temp.path().join("fixtures/demo/package.json"), app_package).unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.frontend_apps.len(), 1);
    assert_eq!(inference.frontend_apps[0].dir, "apps/web");
}

#[test]
fn workspace_exclusion_globs_remove_frontend_candidates() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("packages/web")).unwrap();
    fs::create_dir_all(temp.path().join("packages/private")).unwrap();
    fs::write(
        temp.path().join("package.json"),
        r#"{"private":true,"workspaces":["packages/*","!packages/private"]}"#,
    )
    .unwrap();
    let app_package = r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#;
    fs::write(temp.path().join("packages/web/package.json"), app_package).unwrap();
    fs::write(
        temp.path().join("packages/private/package.json"),
        app_package,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.frontend_apps.len(), 1);
    assert_eq!(inference.frontend_apps[0].dir, "packages/web");
}

#[test]
fn pnpm_workspace_exclusion_globs_remove_frontend_candidates() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("packages/web")).unwrap();
    fs::create_dir_all(temp.path().join("packages/private")).unwrap();
    fs::write(
        temp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\n  - !packages/private\n",
    )
    .unwrap();
    let app_package = r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#;
    fs::write(temp.path().join("packages/web/package.json"), app_package).unwrap();
    fs::write(
        temp.path().join("packages/private/package.json"),
        app_package,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.frontend_apps.len(), 1);
    assert_eq!(inference.frontend_apps[0].dir, "packages/web");
}

#[test]
fn quoted_pnpm_workspace_exclusion_globs_remove_frontend_candidates() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("packages/web")).unwrap();
    fs::create_dir_all(temp.path().join("packages/private")).unwrap();
    fs::write(
        temp.path().join("pnpm-workspace.yaml"),
        "packages: [\"packages/*\", \"!packages/private\"]\n",
    )
    .unwrap();
    let app_package = r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#;
    fs::write(temp.path().join("packages/web/package.json"), app_package).unwrap();
    fs::write(
        temp.path().join("packages/private/package.json"),
        app_package,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.frontend_apps.len(), 1);
    assert_eq!(inference.frontend_apps[0].dir, "packages/web");
}

#[test]
fn declared_workspaces_skip_root_frontend_app_candidate() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    let app_package = r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#;
    fs::write(
        temp.path().join("package.json"),
        r#"{
  "private": true,
  "workspaces": ["apps/*"],
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#,
    )
    .unwrap();
    fs::write(temp.path().join("apps/web/package.json"), app_package).unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.frontend_apps.len(), 1);
    assert_eq!(inference.frontend_apps[0].dir, "apps/web");
}

#[test]
fn explicit_empty_workspaces_suppress_frontend_fallback_scan() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("apps/web")).unwrap();
    fs::write(
        temp.path().join("package.json"),
        r#"{"private":true,"workspaces":[]}"#,
    )
    .unwrap();
    fs::write(
        temp.path().join("apps/web/package.json"),
        r#"{
  "scripts": {
    "dev": "vite",
    "lint": "eslint .",
    "typecheck": "tsc --noEmit",
    "build:bundle": "vite build",
    "test:coverage": "vitest run --coverage"
  }
}"#,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert!(inference.frontend_apps.is_empty());
}

#[test]
fn sqlx_metadata_dir_alone_enables_sqlx_inference() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".sqlx")).unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.sqlx_enabled, Some(true));
    assert_eq!(inference.rust_sqlx_metadata_dir.as_deref(), Some(".sqlx"));
    assert!(
        inference
            .signals
            .iter()
            .any(|signal| signal == "SQLx metadata directory .sqlx")
    );
}

#[test]
fn sqlx_detection_warns_when_default_paths_are_synthesized() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"

[dependencies]
sqlx = "0.8"
"#,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.sqlx_enabled, Some(true));
    assert!(inference.warnings.iter().any(|warning| {
        warning.contains("SQLx was detected but migration and metadata directories were not")
    }));
}

#[test]
fn sqlx_migration_dir_without_metadata_reports_metadata_warning_only() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"

[dependencies]
sqlx = "0.8"
"#,
    )
    .unwrap();
    fs::create_dir_all(temp.path().join("crates/perdify-db/migrations")).unwrap();
    fs::write(
        temp.path()
            .join("crates/perdify-db/migrations/20260101000000_init.sql"),
        "select 1;",
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.sqlx_enabled, Some(true));
    assert_eq!(
        inference.rust_migration_dir.as_deref(),
        Some("crates/perdify-db/migrations")
    );
    assert!(
        inference
            .warnings
            .iter()
            .any(|warning| { warning.contains("SQLx metadata directory was not detected") })
    );
    assert!(
        inference
            .warnings
            .iter()
            .all(|warning| { !warning.contains("migration and metadata directories were not") })
    );
}

#[test]
fn oversized_cargo_toml_reports_scan_warning() {
    let temp = tempfile::tempdir().unwrap();
    let mut manifest = String::from("[package]\nname = \"demo\"\nversion = \"0.1.0\"\n");
    manifest.push_str(&"#".repeat((MAX_SCAN_FILE_BYTES as usize) + 1));
    fs::write(temp.path().join("Cargo.toml"), manifest).unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert!(inference.warnings.iter().any(|warning| {
        warning.contains("could not read TOML for inference") && warning.contains("is larger than")
    }));
}

#[test]
fn oversized_text_scan_file_reports_warning() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("src")).unwrap();
    fs::write(
        temp.path().join("src/lib.rs"),
        "x".repeat((MAX_SCAN_FILE_BYTES as usize) + 1),
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert!(inference.warnings.iter().any(|warning| {
        warning.contains("could not read text for inference") && warning.contains("is larger than")
    }));
}

#[test]
fn unreadable_yaml_inference_file_reports_warning() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
    fs::write(
        temp.path().join(".github/workflows/test.yml"),
        "x".repeat((MAX_SCAN_FILE_BYTES as usize) + 1),
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert!(inference.warnings.iter().any(|warning| {
        warning.contains("could not read YAML for inference") && warning.contains("is larger than")
    }));
}

#[test]
fn malformed_package_json_reports_scan_warning() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("package.json"), "{not json").unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert!(inference.warnings.iter().any(|warning| {
        warning.contains("could not read JSON for inference") && warning.contains("Failed to parse")
    }));
}

#[cfg(unix)]
#[test]
fn unreadable_src_bin_reports_crate_target_warning() {
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    struct PermissionGuard(PathBuf);

    impl Drop for PermissionGuard {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
        }
    }

    let temp = tempfile::tempdir().unwrap();
    let src_bin = temp.path().join("src/bin");
    fs::create_dir_all(&src_bin).unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    fs::set_permissions(&src_bin, fs::Permissions::from_mode(0o000)).unwrap();
    let _permission_guard = PermissionGuard(src_bin);

    let inference = infer_adopt_answers(temp.path());

    assert!(
        inference
            .warnings()
            .iter()
            .any(|warning| warning.contains("could not read src/bin")),
        "expected src/bin read warning, got {:?}",
        inference.warnings()
    );
}

#[test]
fn github_runner_is_inferred_from_workflows() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
    fs::write(
        temp.path().join(".github/workflows/test.yml"),
        "jobs:\n  test:\n    runs-on: ubuntu-24.04\n",
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.ci_github_runner.as_deref(), Some("ubuntu-24.04"));
}

#[test]
fn github_ci_shape_reports_checks_lockfiles_cache_and_matrix() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
    fs::write(
        temp.path().join(".github/workflows/ci.yml"),
        r#"jobs:
  rust:
    name: cargo locked
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-24.04, windows-latest]
        toolchain: [stable, nightly]
    steps:
      - uses: actions/checkout@v6
      - uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          toolchain: ${{ matrix.toolchain }}
          cache: false
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --workspace --locked
  web:
    name: web checks
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/setup-node@v5
        with:
          cache: pnpm
      - run: pnpm install --frozen-lockfile
"#,
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());
    let report = inference.report();
    let shape = &report["ci_shape"];

    assert_eq!(inference.ci_github_runner.as_deref(), Some("ubuntu-24.04"));
    assert_eq!(shape["workflow_files"][0], ".github/workflows/ci.yml");
    assert_eq!(shape["generated_jig_checks_role"], "supplement_existing_ci");
    assert!(signal_values(&shape["required_checks"]).contains(&"cargo locked"));
    assert!(signal_values(&shape["required_checks"]).contains(&"web checks"));
    assert!(
        signal_values(&shape["lockfile_behavior"])
            .contains(&"Cargo lockfile enforced with --locked")
    );
    assert!(signal_values(&shape["lockfile_behavior"]).contains(&"pnpm frozen lockfile install"));
    assert!(signal_values(&shape["cache_strategy"]).contains(&"Swatinem/rust-cache"));
    assert!(signal_values(&shape["cache_strategy"]).contains(&"setup-node dependency cache: pnpm"));
    assert!(
        signal_values(&shape["cache_strategy"]).contains(&"setup-rust-toolchain cache disabled")
    );
    assert!(signal_values(&shape["matrix"]["os"]).contains(&"windows-latest"));
    assert!(signal_values(&shape["matrix"]["toolchain"]).contains(&"nightly"));
    assert_eq!(report["metadata"]["ci_shape"]["confidence"], "medium");
}

#[test]
fn github_ci_shape_marks_existing_jig_checks_as_replace_role() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
    fs::write(
            temp.path().join(".github/workflows/jig.yml"),
            "jobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: scripts/jig check test\n",
        )
        .unwrap();

    let report = infer_adopt_answers(temp.path()).report();
    let shape = &report["ci_shape"];

    assert_eq!(
        shape["generated_jig_checks_role"],
        "replace_existing_jig_ci"
    );
    assert!(signal_values(&shape["existing_jig_checks"]).contains(&"scripts/jig check test"));
}

#[test]
fn github_runner_strips_inline_comments() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
    fs::write(
        temp.path().join(".github/workflows/test.yml"),
        "jobs:\n  test:\n    runs-on: ubuntu-latest # primary runner\n",
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.ci_github_runner.as_deref(), Some("ubuntu-latest"));
}

#[test]
fn github_runner_single_item_array_is_inferred() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
    fs::write(
        temp.path().join(".github/workflows/test.yml"),
        "jobs:\n  test:\n    runs-on: [ubuntu-latest]\n",
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.ci_github_runner.as_deref(), Some("ubuntu-latest"));
}

#[test]
fn github_runner_tie_break_prefers_newer_ubuntu_label() {
    let runners = BTreeMap::from([
        ("ubuntu-22.04".to_string(), 1),
        ("ubuntu-24.04".to_string(), 1),
    ]);

    assert_eq!(
        select_github_runner(&runners).as_deref(),
        Some("ubuntu-24.04")
    );
}

#[test]
fn multiple_github_runners_are_reported_as_warnings() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
    fs::write(
        temp.path().join(".github/workflows/a.yml"),
        "jobs:\n  test:\n    runs-on: macos-latest\n",
    )
    .unwrap();
    fs::write(
        temp.path().join(".github/workflows/b.yml"),
        "jobs:\n  test:\n    runs-on: ubuntu-24.04\n",
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.ci_github_runner.as_deref(), Some("ubuntu-24.04"));
    assert!(
        inference
            .warnings
            .iter()
            .any(|warning| { warning.contains("multiple GitHub Actions runners detected") })
    );
}

#[test]
fn sqlx_with_windows_runner_reports_posix_command_warning() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"

[dependencies]
sqlx = "0.8"
"#,
    )
    .unwrap();
    fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
    fs::write(
        temp.path().join(".github/workflows/test.yml"),
        "jobs:\n  test:\n    runs-on: windows-latest\n",
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.sqlx_enabled, Some(true));
    assert_eq!(
        inference.ci_github_runner.as_deref(),
        Some("windows-latest")
    );
    assert!(inference.warnings.iter().any(|warning| {
        warning.contains("SQLx check command inference uses POSIX shell syntax")
    }));
}

#[test]
fn multiline_github_runner_sequence_is_inferred() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
    fs::write(
        temp.path().join(".github/workflows/test.yml"),
        "jobs:\n  test:\n    runs-on:\n      - ubuntu-24.04\n",
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.ci_github_runner.as_deref(), Some("ubuntu-24.04"));
}

#[test]
fn composite_github_runner_labels_are_reported_as_warnings() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
    fs::write(
        temp.path().join(".github/workflows/test.yml"),
        "jobs:\n  test:\n    runs-on: [self-hosted, linux]\n",
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert!(inference.ci_github_runner.is_none());
    assert!(
        inference
            .warnings
            .iter()
            .any(|warning| { warning.contains("unsupported composite runs-on labels") })
    );
}

#[test]
fn dynamic_github_runner_is_reported_as_warning() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
    fs::write(
        temp.path().join(".github/workflows/test.yml"),
        "jobs:\n  test:\n    runs-on: ${{ matrix.runner }}\n",
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert!(inference.ci_github_runner.is_none());
    assert!(
        inference
            .warnings
            .iter()
            .any(|warning| { warning.contains("unsupported dynamic runs-on expression") })
    );
}

#[test]
fn empty_github_runner_is_reported_as_warning() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
    fs::write(
        temp.path().join(".github/workflows/test.yml"),
        "jobs:\n  test:\n    runs-on: \"\"\n",
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert!(inference.ci_github_runner.is_none());
    assert!(
        inference
            .warnings
            .iter()
            .any(|warning| { warning.contains("empty runs-on value") })
    );
}

#[test]
fn reusable_workflow_inputs_named_runs_on_are_not_inferred_as_job_runners() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
    fs::write(
            temp.path().join(".github/workflows/test.yml"),
            "jobs:\n  call:\n    uses: owner/repo/.github/workflows/test.yml@main\n    with:\n      runs-on: ubuntu-latest\n",
        )
        .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert!(inference.ci_github_runner.is_none());
}
