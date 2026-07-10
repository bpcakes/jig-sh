use super::*;
use crate::bootstrap::template_source::prepare_template_source_from_base;

#[test]
fn adopt_without_template_uses_official_template_release_tag_and_records_metadata() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    write_test_crate_guide(&repo);
    let template = materialize_template_git_worktree();
    let fake_commit = "0123456789abcdef0123456789abcdef01234567";

    let log_path = temp.path().join("commands.log");
    let git_path = temp.path().join("git-stub.sh");
    fs::write(
        &git_path,
        format!(
            r#"#!/bin/sh
printf 'git %s\n' "$*" >> "{log_path}"
if [ "$1" = "clone" ]; then
  mkdir -p "$4"
  cp -R "{template}/." "$4"
  exit 0
fi
if [ "$1" = "rev-parse" ]; then
  printf '{fake_commit}\n'
  exit 0
fi
exit 0
"#,
            log_path = log_path.display(),
            template = template.path().display(),
            fake_commit = fake_commit,
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, &git_path);

    run_adopt(AdoptOpts {
        path: repo.clone(),
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
    .unwrap();

    let commands = fs::read_to_string(log_path).unwrap();
    assert!(commands.contains("git clone --quiet https://github.com/bpcakes/jig-sh.git"));
    assert!(commands.contains(&format!(
        "git checkout --quiet v{}",
        env!("CARGO_PKG_VERSION")
    )));

    let answers = read_answers_toml(&repo.join(".jig.toml")).unwrap();
    assert_eq!(
        answers.get("_src_path").and_then(TomlValue::as_str),
        Some(OFFICIAL_TEMPLATE_SOURCE)
    );
    assert_eq!(
        answers.get("_commit").and_then(TomlValue::as_str),
        Some(fake_commit)
    );
}

#[test]
fn omitted_template_preserves_explicit_vcs_ref() {
    let vcs_ref = Some("main".to_string());
    let request = resolve_initial_template_request(None, &vcs_ref).unwrap();

    assert_eq!(request.template, OFFICIAL_TEMPLATE_SOURCE);
    assert_eq!(request.vcs_ref.as_deref(), Some("main"));
    assert!(request.used_default);
}

#[test]
fn explicit_official_template_url_still_uses_release_pin() {
    let template = Some(OFFICIAL_TEMPLATE_SOURCE.to_string());
    let no_ref = None;
    let request = resolve_initial_template_request_with_policy(
        template.as_deref(),
        &no_ref,
        BuildTemplatePinPolicy::Released,
    )
    .unwrap();

    assert_eq!(request.template, OFFICIAL_TEMPLATE_SOURCE);
    assert_eq!(
        request.vcs_ref.as_deref(),
        Some(official_template_ref().as_str())
    );
    assert!(request.used_default);

    assert!(is_official_template_source(
        "https://github.com/bpcakes/jig-sh"
    ));
    assert!(!is_official_template_source(
        "https://github.com/bpcakes/jig-sh.git.git"
    ));
}

#[test]
fn unreleased_build_uses_embedded_template_without_ref() {
    let no_ref = None;
    let request = resolve_initial_template_request_with_policy(
        None,
        &no_ref,
        BuildTemplatePinPolicy::Unreleased,
    )
    .unwrap();

    assert_eq!(request.template, EMBEDDED_TEMPLATE_SOURCE);
    assert_eq!(request.vcs_ref.as_deref(), None);
    assert!(request.used_default);
}

#[test]
fn run_adopt_uses_embedded_template_for_unreleased_build_policy() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    write_test_crate_guide(&repo);

    with_test_build_template_pin_policy(BuildTemplatePinPolicy::Unreleased, || {
        run_adopt(AdoptOpts {
            path: repo.clone(),
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
        .unwrap()
    });

    let answers = read_answers_toml(&repo.join(".jig.toml")).unwrap();
    assert_eq!(
        answers.get("_src_path").and_then(TomlValue::as_str),
        Some(EMBEDDED_TEMPLATE_SOURCE)
    );
    assert_eq!(answers.get("_commit").and_then(TomlValue::as_str), Some(""));
    assert!(repo.join("scripts/jig").exists());
    assert!(repo.join("scripts/install-jig.sh").exists());
    let installer = fs::read_to_string(repo.join("scripts/install-jig.sh")).unwrap();
    assert!(installer.contains("resolve_installed_jig_for_embedded_source"));
    assert!(installer.contains(r#"[[ "$source" == "embedded:jig-sh" ]]"#));
    assert!(installer.contains("no same-version jig binary was found on PATH"));
    assert!(installer.contains("JIG_INSTALL_ALLOW_EMBEDDED_SOURCE_FALLBACK=1"));
}

#[test]
fn update_uses_stored_embedded_template_by_default() {
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    write_test_crate_guide(&repo);

    with_test_build_template_pin_policy(BuildTemplatePinPolicy::Unreleased, || {
        run_adopt(AdoptOpts {
            path: repo.clone(),
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
        .unwrap()
    });
    fs::write(repo.join("scripts/install-jig.sh"), "# locally changed\n").unwrap();

    run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let answers = read_answers_toml(&repo.join(".jig.toml")).unwrap();
    assert_eq!(
        answers.get("_src_path").and_then(TomlValue::as_str),
        Some(EMBEDDED_TEMPLATE_SOURCE)
    );
    assert!(
        fs::read_to_string(repo.join("scripts/install-jig.sh"))
            .unwrap()
            .contains("embedded:jig-sh")
    );
}

#[test]
fn embedded_template_source_rejects_mode_and_vcs_ref() {
    let temp = tempdir().unwrap();

    let mode_error = prepare_template_source_from_base(
        EMBEDDED_TEMPLATE_SOURCE,
        Some(TemplateMode::Committed),
        None,
        temp.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(mode_error.contains("--template-mode only applies"));

    let ref_error = prepare_template_source_from_base(
        EMBEDDED_TEMPLATE_SOURCE,
        None,
        Some("main"),
        temp.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(ref_error.contains("--vcs-ref only applies"));
}

#[test]
fn update_rejects_explicit_switch_from_committed_source_to_embedded_source() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    write_test_crate_guide(&repo);
    let template = materialize_template_git_worktree();
    adopt_repo_for_test(&repo, template.path(), TemplateMode::Committed);

    let error = run_update(UpdateOpts {
        path: repo,
        template: Some(EMBEDDED_TEMPLATE_SOURCE.into()),
        template_mode: None,
        recopy: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("cannot switch template source paths"));
}

#[test]
fn unreleased_build_rejects_canonical_official_url_without_ref() {
    for template in [
        "https://github.com/bpcakes/jig-sh",
        "https://github.com/bpcakes/jig-sh.git",
    ] {
        let no_ref = None;
        let error = resolve_initial_template_request_with_policy(
            Some(template),
            &no_ref,
            BuildTemplatePinPolicy::Unreleased,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("unreleased or dirty local source"));
        assert!(error.contains(&official_template_ref()));
    }
}

#[test]
fn unreleased_build_allows_explicit_official_ref() {
    let vcs_ref = Some("main".to_string());
    let request = resolve_initial_template_request_with_policy(
        None,
        &vcs_ref,
        BuildTemplatePinPolicy::Unreleased,
    )
    .unwrap();

    assert_eq!(request.template, OFFICIAL_TEMPLATE_SOURCE);
    assert_eq!(request.vcs_ref.as_deref(), Some("main"));
    assert!(request.used_default);
}

#[test]
fn unreleased_build_allows_explicit_official_release_tag() {
    let vcs_ref = Some("v0.1.0".to_string());
    let request = resolve_initial_template_request_with_policy(
        None,
        &vcs_ref,
        BuildTemplatePinPolicy::Unreleased,
    )
    .unwrap();

    assert_eq!(request.template, OFFICIAL_TEMPLATE_SOURCE);
    assert_eq!(request.vcs_ref.as_deref(), Some("v0.1.0"));
    assert!(request.used_default);
}

#[test]
fn unreleased_build_allows_explicit_official_ref_for_canonical_urls() {
    for template in [
        "https://github.com/bpcakes/jig-sh",
        "https://github.com/bpcakes/jig-sh.git",
    ] {
        let vcs_ref = Some("main".to_string());
        let request = resolve_initial_template_request_with_policy(
            Some(template),
            &vcs_ref,
            BuildTemplatePinPolicy::Unreleased,
        )
        .unwrap();

        assert_eq!(request.template, OFFICIAL_TEMPLATE_SOURCE);
        assert_eq!(request.vcs_ref.as_deref(), Some("main"));
        assert!(request.used_default);
    }
}

#[test]
fn build_template_pin_policy_env_parser_handles_all_values() {
    assert_eq!(
        build_template_pin_policy_from_env(Some("released")),
        BuildTemplatePinPolicy::Released
    );
    assert_eq!(
        build_template_pin_policy_from_env(Some("unreleased")),
        BuildTemplatePinPolicy::Unreleased
    );
    assert_eq!(
        build_template_pin_policy_from_env(Some("unknown")),
        BuildTemplatePinPolicy::Unknown
    );
    assert_eq!(
        build_template_pin_policy_from_env(None),
        BuildTemplatePinPolicy::Unknown
    );
}

#[test]
fn unknown_build_uses_release_pin_for_packaged_installs() {
    let no_ref = None;
    let request = resolve_initial_template_request_with_policy(
        None,
        &no_ref,
        BuildTemplatePinPolicy::Unknown,
    )
    .unwrap();

    assert_eq!(request.template, OFFICIAL_TEMPLATE_SOURCE);
    assert_eq!(
        request.vcs_ref.as_deref(),
        Some(official_template_ref().as_str())
    );
    assert!(request.used_default);
}

#[test]
fn unreleased_build_allows_non_official_template_source() {
    let template = Some("/path/to/jig-sh".to_string());
    let no_ref = None;
    let request = resolve_initial_template_request_with_policy(
        template.as_deref(),
        &no_ref,
        BuildTemplatePinPolicy::Unreleased,
    )
    .unwrap();

    assert_eq!(request.template, "/path/to/jig-sh");
    assert_eq!(request.vcs_ref.as_deref(), None);
    assert!(!request.used_default);
}

#[test]
fn omitted_template_uses_release_tag_for_package_version() {
    assert_eq!(official_template_ref_for_version("1.2.3"), "v1.2.3");
    assert_eq!(
        official_template_ref_for_version("1.2.3-rc.1"),
        "v1.2.3-rc.1"
    );
}

#[test]
fn default_template_mode_rejects_local_only_mode_before_clone() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    write_test_crate_guide(&repo);

    // The default template is remote, so this must fail before any git clone can start.
    let error = run_adopt(AdoptOpts {
        path: repo,
        template: None,
        template_mode: Some(TemplateMode::Committed),
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
    .unwrap_err();

    let error_chain = format!("{:#}", error);
    assert!(error_chain.contains("--template-mode only applies to local git template paths."));
    assert!(error_chain.contains("Omit --template-mode for remote templates"));
}

#[test]
fn default_template_resolution_errors_explain_offline_and_ref_overrides() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    write_test_crate_guide(&repo);
    let template = materialize_template_git_worktree();

    let git_path = temp.path().join("git-stub.sh");
    fs::write(
        &git_path,
        format!(
            r#"#!/bin/sh
if [ "$1" = "clone" ]; then
  mkdir -p "$4"
  cp -R "{template}/." "$4"
  exit 0
fi
if [ "$1" = "checkout" ]; then
  echo "missing release tag" >&2
  exit 1
fi
exit 0
"#,
            template = template.path().display(),
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&git_path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, &git_path);

    let error = run_adopt(AdoptOpts {
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
    assert!(error.contains("requires network access"));
    assert!(error.contains("prerelease or development version"));
    assert!(error.contains("--template <local-path>"));
    assert!(error.contains("--vcs-ref <ref>"));
}

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
