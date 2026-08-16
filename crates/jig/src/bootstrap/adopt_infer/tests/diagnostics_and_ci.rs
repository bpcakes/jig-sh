use super::*;

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
    fs::create_dir_all(temp.path().join("crates/example-db/migrations")).unwrap();
    fs::write(
        temp.path()
            .join("crates/example-db/migrations/20260101000000_init.sql"),
        "select 1;",
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());

    assert_eq!(inference.sqlx_enabled, Some(true));
    assert_eq!(
        inference.rust_migration_dir.as_deref(),
        Some("crates/example-db/migrations")
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
        r"jobs:
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
",
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
fn github_runner_tie_break_recognizes_mixed_case_ubuntu_label() {
    let runners = BTreeMap::from([
        ("Ubuntu-24.04".to_string(), 1),
        ("macos-latest".to_string(), 1),
    ]);

    assert_eq!(
        select_github_runner(&runners).as_deref(),
        Some("Ubuntu-24.04")
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
fn windows_runner_is_reported_as_excluded_when_supported_runner_is_available() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".github/workflows")).unwrap();
    fs::write(
        temp.path().join(".github/workflows/test.yml"),
        "jobs:\n  linux:\n    runs-on: macos-latest\n  windows:\n    runs-on: windows-latest\n",
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());
    let report = inference.report();

    assert_eq!(inference.ci_github_runner.as_deref(), Some("macos-latest"));
    assert_eq!(report["metadata"]["ci_github_runner"]["confidence"], "high");
    assert!(
        report["metadata"]["ci_github_runner"]["warnings"][0]
            .as_str()
            .is_some_and(|warning| {
                warning.contains("were excluded from generated-check runner inference")
            })
    );
    assert!(inference.warnings.iter().any(|warning| {
        warning.contains("were excluded from generated-check runner inference")
    }));
    assert!(
        inference
            .warnings
            .iter()
            .all(|warning| !warning.contains("using ubuntu-latest because"))
    );
}

#[test]
fn windows_only_runner_falls_back_to_supported_host_with_warning() {
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
        "jobs:\n  test:\n    runs-on: windows-latest\n  lint:\n    runs-on: Windows-2025\n",
    )
    .unwrap();

    let inference = infer_adopt_answers(temp.path());
    let report = inference.report();

    assert_eq!(inference.sqlx_enabled, Some(true));
    assert_eq!(inference.ci_github_runner.as_deref(), Some("ubuntu-latest"));
    assert_eq!(report["metadata"]["ci_github_runner"]["confidence"], "low");
    assert!(
        report["metadata"]["ci_github_runner"]["warnings"][0]
            .as_str()
            .is_some_and(|warning| warning.contains("was synthesized"))
    );
    assert_eq!(report["metadata"]["ci_github_runner"]["sources"], json!([]));
    assert!(inference.warnings.iter().any(|warning| {
        warning.contains("Windows GitHub Actions runners are unsupported by Jig")
    }));
    assert!(
        inference
            .warnings
            .iter()
            .all(|warning| !warning.contains("multiple GitHub Actions runners detected"))
    );
}

#[test]
fn supported_runner_is_preferred_over_more_common_windows_runner() {
    let runners = BTreeMap::from([("macos-latest".to_string(), 1), ("windows".to_string(), 3)]);

    assert_eq!(
        select_github_runner(&runners).as_deref(),
        Some("macos-latest")
    );
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
