use super::*;

#[test]
fn default_template_clone_errors_get_official_template_context() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    write_test_crate_guide(&repo);

    let git_path = temp.path().join("git-stub.sh");
    fs::write(
        &git_path,
        r#"#!/bin/sh
if [ "$1" = "--no-replace-objects" ]; then
  shift
fi
if [ "$1" = "clone" ]; then
  echo "network unavailable" >&2
  exit 1
fi
exit 0
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, &git_path);

    let error = run_adopt(AdoptOpts {
        components: Default::default(),
        path: repo,
        template: None,
        template_mode: None,
        vcs_ref: None,
        force: false,
        write: true,
        minimal: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some("demo".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("Failed to resolve the official Jig template"));
    assert!(error.contains(OFFICIAL_TEMPLATE_SOURCE));
    assert!(error.contains(&official_template_ref()));
    assert!(error.contains("requires network access"));
}

#[test]
fn default_template_resolution_error_for_explicit_ref_does_not_blame_release_tag() {
    let vcs_ref = Some("main".to_string());
    let request = resolve_initial_template_request(None, &vcs_ref).unwrap();
    let error = default_template_failure_context(&request);

    assert!(error.contains("at main"));
    assert!(error.contains("selected ref must exist"));
    assert!(!error.contains("matching release tag"));
    assert!(!error.contains("prerelease or development version"));
}
