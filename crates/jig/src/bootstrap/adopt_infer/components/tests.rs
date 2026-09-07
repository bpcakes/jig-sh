use std::fs;

use tempfile::tempdir;

use super::*;

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn cargo(root: &Path, directory: &str) {
    write(
        root,
        &format!("{directory}/Cargo.toml"),
        "[package]\nname = 'example-library'\nversion = '0.1.0'\n",
    );
}

fn discover(root: &Path) -> ComponentCandidates {
    let mut warnings = Vec::new();
    let scan = RepoScan::collect(root, &mut warnings);
    ComponentCandidates::discover(root, &scan, &[], &[], &mut warnings)
}

fn state(candidates: &ComponentCandidates, root: &str) -> Disposition {
    candidates
        .candidates
        .iter()
        .find(|candidate| candidate.root == root)
        .unwrap()
        .disposition
}

#[test]
fn adopt_components_require_ownership_beyond_raw_manifest_hits() {
    let temp = tempdir().unwrap();
    write(
        temp.path(),
        "Cargo.toml",
        "[workspace]\nmembers=['crates/*']\nexclude=['crates/excluded']\n",
    );
    cargo(temp.path(), "crates/core");
    cargo(temp.path(), "crates/excluded");
    cargo(temp.path(), "fixtures/incidental");
    cargo(temp.path(), "tools/unrelated");
    write(
        temp.path(),
        "packages/raw/package.json",
        "{\"name\":\"example-library\"}",
    );
    write(
        temp.path(),
        "services/raw/go.mod",
        "module example.test/service\n",
    );
    let candidates = discover(temp.path());
    assert_eq!(state(&candidates, "."), Disposition::Included);
    assert_eq!(state(&candidates, "crates/core"), Disposition::Included);
    assert_eq!(state(&candidates, "crates/excluded"), Disposition::Excluded);
    for root in [
        "fixtures/incidental",
        "tools/unrelated",
        "packages/raw",
        "services/raw",
    ] {
        assert_eq!(
            state(&candidates, root),
            Disposition::ReviewRequired,
            "{root}"
        );
    }
    for candidate in &candidates.candidates {
        assert!(jig_contract::ComponentId::parse(&candidate.proposed_id).is_ok());
        assert!(
            candidate
                .evidence
                .iter()
                .all(|source| !source.starts_with('/'))
        );
    }
}

#[test]
fn adopt_components_select_exact_normalized_roots_without_renaming_candidates() {
    let temp = tempdir().unwrap();
    cargo(temp.path(), "crates/core");
    cargo(temp.path(), "fixtures/incidental");
    let mut candidates = discover(temp.path());
    let ids = candidates
        .candidates
        .iter()
        .map(|candidate| candidate.proposed_id.clone())
        .collect::<Vec<_>>();
    candidates
        .select(
            temp.path(),
            &ComponentSelectionOpts {
                include: vec!["./fixtures/incidental/".into()],
                exclude: vec!["crates/core".into()],
            },
        )
        .unwrap();
    assert_eq!(
        state(&candidates, "fixtures/incidental"),
        Disposition::Included
    );
    assert_eq!(state(&candidates, "crates/core"), Disposition::Excluded);
    assert_eq!(
        ids,
        candidates
            .candidates
            .iter()
            .map(|candidate| candidate.proposed_id.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn adopt_components_reject_invalid_selections_atomically() {
    let temp = tempdir().unwrap();
    cargo(temp.path(), "crates/core");
    let original = discover(temp.path());
    for excluded in [
        "../escape",
        "/absolute",
        "C:\\absolute",
        "crates/*",
        "unknown",
    ] {
        let mut candidates = original.clone();
        assert!(
            candidates
                .select(
                    temp.path(),
                    &ComponentSelectionOpts {
                        include: vec!["crates/core".into()],
                        exclude: vec![excluded.into()],
                    }
                )
                .is_err(),
            "{excluded}"
        );
        assert_eq!(candidates.report(), original.report());
    }
    let mut candidates = original.clone();
    let error = candidates
        .select(
            temp.path(),
            &ComponentSelectionOpts {
                include: vec!["crates/core".into()],
                exclude: vec!["./crates/core/".into()],
            },
        )
        .unwrap_err();
    assert!(error.to_string().contains("both included and excluded"));
    assert_eq!(candidates.report(), original.report());
}

#[test]
fn adopt_components_do_not_assume_membership_with_unsupported_exclusions() {
    let temp = tempdir().unwrap();
    write(
        temp.path(),
        "Cargo.toml",
        "[workspace]\nmembers=['crates/*']\nexclude=['crates/{core,other}']\n",
    );
    cargo(temp.path(), "crates/core");
    assert_eq!(
        state(&discover(temp.path()), "crates/core"),
        Disposition::ReviewRequired
    );
}

#[cfg(unix)]
#[test]
fn adopt_components_git_scan_does_not_follow_replaced_directory_symlinks() {
    use std::os::unix::fs::symlink;
    use std::process::Command;
    let temp = tempdir().unwrap();
    let outside = tempdir().unwrap();
    cargo(temp.path(), "linked");
    write(
        outside.path(),
        "Cargo.toml",
        "[package]\nname='outside-fixture'\nversion='0.1.0'\n",
    );
    for args in [vec!["init", "-q"], vec!["add", "linked/Cargo.toml"]] {
        assert!(
            Command::new("git")
                .current_dir(temp.path())
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    fs::remove_dir_all(temp.path().join("linked")).unwrap();
    symlink(outside.path(), temp.path().join("linked")).unwrap();
    let mut warnings = Vec::new();
    let scan = RepoScan::collect(temp.path(), &mut warnings);
    assert!(scan.named_files("Cargo.toml").next().is_none());
    assert!(discover(temp.path()).candidates.is_empty());
}

#[test]
fn adopt_components_unselected_sqlx_manifest_cannot_enable_root_backend_capabilities() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        root,
        "Cargo.toml",
        "[package]\nname = \"ExampleProject\"\nversion = \"0.1.0\"\n",
    );
    write(
        root,
        "fixtures/incidental/Cargo.toml",
        "[package]\nname = \"incidental\"\nversion = \"0.1.0\"\n[dependencies]\nsqlx = \"0.9\"\n",
    );
    let mut inference = crate::bootstrap::adopt_infer::infer_adopt_answers(root);
    inference
        .select_components(root, &ComponentSelectionOpts::default())
        .unwrap();
    assert_eq!(inference.report()["sqlx_enabled"], false);
    assert_eq!(
        inference.report()["metadata"]["sqlx_enabled"]["value"],
        false
    );
    let candidates = inference.report()["component_candidates"]
        .as_array()
        .unwrap()
        .clone();
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate["root"] == "fixtures/incidental"
                && candidate["disposition"] == "review_required")
    );
}

#[test]
fn adopt_components_invalid_package_workspace_is_review_required() {
    let temp = tempfile::tempdir().unwrap();
    write(temp.path(), "package.json", r#"{"workspaces":"apps/*"}"#);
    let candidates = discover(temp.path());
    assert_eq!(candidates.candidates.len(), 1);
    assert_eq!(
        candidates.candidates[0].disposition,
        Disposition::ReviewRequired
    );
}

#[test]
fn adopt_components_preserved_custom_id_has_one_consistent_candidate() {
    let temp = tempdir().unwrap();
    cargo(temp.path(), ".");
    let mut candidates = discover(temp.path());
    let model: crate::bootstrap::repository_model::AuthoredRepositoryModel =
        serde_json::from_value(json!({
            "default_check_profile":"verify", "affected_ignore":[],
            "components":[{"id":"service","root":".","adapters":["rust"]}],
            "actions":[],"profiles":[]
        }))
        .unwrap();
    candidates.preserve(&model);
    assert_eq!(candidates.candidates.len(), 1);
    assert_eq!(candidates.candidates[0].proposed_id, "service");
    assert_eq!(candidates.candidates[0].disposition, Disposition::Included);
}

#[test]
fn adopt_components_no_root_backend_clears_sqlx_metadata_and_signals() {
    let temp = tempdir().unwrap();
    write(
        temp.path(),
        "Cargo.toml",
        "[package]\nname='example-service'\nversion='0.1.0'\n[dependencies]\nsqlx='0.9'\n",
    );
    write(temp.path(), "migrations/001_example.sql", "SELECT 1;\n");
    let mut inference = crate::bootstrap::adopt_infer::infer_adopt_answers(temp.path());
    inference
        .select_components(
            temp.path(),
            &ComponentSelectionOpts {
                include: Vec::new(),
                exclude: vec![".".into()],
            },
        )
        .unwrap();
    let report = inference.report();
    assert_eq!(report["sqlx_enabled"], false);
    assert_eq!(report["metadata"]["sqlx_enabled"]["value"], false);
    for key in [
        "rust_migration_dir",
        "rust_migration_dirs",
        "rust_sqlx_metadata_dir",
        "sqlx_check_command",
    ] {
        assert!(report["metadata"].get(key).is_none(), "{report}");
    }
    assert!(
        !report["signals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|signal| signal.as_str().unwrap().contains("SQLx dependency"))
    );
}
