use std::fs;

use tempfile::tempdir;

use crate::doctor::version_authority::go_module_version_authority;
use crate::doctor::version_authority::{
    GO_MODULE_AUTHORITY_MAX_BYTES, NumericVersion, numeric_version_authority,
    select_go_module_version_requirement,
};

#[test]
fn numeric_version_authority_bounds_and_validates_single_token_files() {
    for (contents, expected) in [
        (Vec::new(), "bounded version token"),
        (vec![b'1'; 129], "bounded version token"),
        (vec![0xff], "valid UTF-8"),
        (b"1.27.0 1.28.0\n".to_vec(), "exactly one version token"),
    ] {
        let temp = tempdir().unwrap();
        let authority = temp.path().join("version");
        fs::write(&authority, contents).unwrap();

        let error = numeric_version_authority(&authority, "Go", true, "1.26.0").unwrap_err();

        assert!(error.contains(expected), "{error:?}");
    }

    let temp = tempdir().unwrap();
    let authority = temp.path().join("version");
    fs::write(&authority, "1.27\n").unwrap();
    assert_eq!(
        numeric_version_authority(&authority, "Go", true, "1.26.0").unwrap(),
        Some(NumericVersion {
            major: 1,
            minor: 27,
            patch: 0,
        })
    );
}
#[test]
fn go_module_authority_prefers_a_newer_toolchain_and_accepts_default() {
    let temp = tempdir().unwrap();
    let authority = temp.path().join("go.mod");
    fs::write(
        &authority,
        "module example.com/ExampleProject\n\ngo \"1.27\"\ntoolchain go1.28.2 // local floor\n",
    )
    .unwrap();
    assert_eq!(
        go_module_version_authority(&authority).unwrap(),
        Some(NumericVersion {
            major: 1,
            minor: 28,
            patch: 2,
        })
    );

    fs::write(
        &authority,
        "module example.com/ExampleProject\n\ngo 1.27\ntoolchain default\n",
    )
    .unwrap();
    assert_eq!(
        go_module_version_authority(&authority).unwrap(),
        Some(NumericVersion {
            major: 1,
            minor: 27,
            patch: 0,
        })
    );
}
#[test]
fn go_module_selector_uses_the_highest_authority_and_preserves_partial_patch_semantics() {
    let temp = tempdir().unwrap();
    let api = temp.path().join("api.mod");
    let worker = temp.path().join("worker.mod");
    fs::write(&api, "module example.com/Api\n\ngo 1.26\n").unwrap();
    fs::write(
        &worker,
        "module example.com/Worker\n\ngo 1.26.1\ntoolchain go1.27.3\n",
    )
    .unwrap();

    let (_, selected) = select_go_module_version_requirement(&[api.clone(), worker.clone()])
        .unwrap()
        .unwrap();
    assert_eq!(selected.selector, "1.27.3");

    fs::write(&api, "module example.com/Api\n\ngo 1.27\n").unwrap();
    fs::write(&worker, "module example.com/Worker\n\ngo 1.27.0\n").unwrap();
    let (_, selected) = select_go_module_version_requirement(&[worker, api])
        .unwrap()
        .unwrap();
    assert_eq!(selected.selector, "1.27");
}
#[test]
fn go_module_authority_rejects_ambiguous_and_malformed_directives() {
    for (contents, expected) in [
        (
            "module example.com/ExampleProject\n",
            "must declare one numeric go version",
        ),
        ("go 1.27\ngo 1.28\n", "go version more than once"),
        (
            "go 1.27\ntoolchain go1.28\ntoolchain go1.29\n",
            "toolchain more than once",
        ),
        ("go 1.28\ntoolchain go1.27\n", "below required go version"),
        ("go stable\n", "exact numeric version"),
        ("go 1.27 extra\n", "invalid go directive"),
    ] {
        let temp = tempdir().unwrap();
        let authority = temp.path().join("go.mod");
        fs::write(&authority, contents).unwrap();

        let error = go_module_version_authority(&authority).unwrap_err();
        assert!(error.contains(expected), "{contents:?}: {error}");
    }
}
#[test]
fn go_module_authority_bounds_and_validates_the_file() {
    for (contents, expected) in [
        (Vec::new(), "non-empty bounded go.mod"),
        (
            vec![b'x'; GO_MODULE_AUTHORITY_MAX_BYTES as usize + 1],
            "non-empty bounded go.mod",
        ),
        (vec![0xff], "valid UTF-8"),
    ] {
        let temp = tempdir().unwrap();
        let authority = temp.path().join("go.mod");
        fs::write(&authority, contents).unwrap();

        let error = go_module_version_authority(&authority).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
}
#[cfg(unix)]
#[test]
fn go_module_authority_rejects_symlinks_without_following_them() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let target = temp.path().join("target.mod");
    let authority = temp.path().join("go.mod");
    fs::write(&target, "module example.com/ExampleProject\n\ngo 1.27.0\n").unwrap();
    symlink(&target, &authority).unwrap();

    let error = go_module_version_authority(&authority).unwrap_err();
    assert!(error.contains("real regular file"), "{error}");
}
#[cfg(unix)]
#[test]
fn numeric_version_authority_rejects_symlinks_without_following_them() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let target = temp.path().join("target-version");
    let authority = temp.path().join("version");
    fs::write(&target, "1.27.0\n").unwrap();
    symlink(&target, &authority).unwrap();

    let error = numeric_version_authority(&authority, "Go", true, "1.26.0").unwrap_err();

    assert!(error.contains("real regular file"), "{error:?}");
}
