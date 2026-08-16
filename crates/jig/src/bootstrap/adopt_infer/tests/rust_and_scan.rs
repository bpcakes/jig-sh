use super::*;

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
    let _guard = crate::test_env::lock_env();
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
    assert_eq!(crate_root_from_workspace_member("crates/*"), "crates");
    assert_eq!(crate_root_from_workspace_member("apps/api"), "apps");
    assert_eq!(crate_root_from_workspace_member("."), ".");
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
