use std::process::Command;

use super::*;
use crate::bootstrap::template_source::{TemplateRenderSource, prepare_template_source_from_base};

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
if [ "$1" = "--no-replace-objects" ]; then
  shift
fi
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
fn committed_local_template_ignores_ambient_git_dir() {
    let _guard = lock_env();
    let template = materialize_template_git_worktree();
    let external = materialize_template_git_worktree();
    fs::write(external.path().join("external-only"), "external\n").unwrap();
    git(external.path(), ["add", "external-only"]).unwrap();
    git(external.path(), ["commit", "-m", "external commit"]).unwrap();

    let local_commit = git_stdout(template.path(), ["rev-parse", "HEAD"]).unwrap();
    let external_commit = git_stdout(external.path(), ["rev-parse", "HEAD"]).unwrap();
    assert_ne!(local_commit, external_commit);
    let status = Command::new("git")
        .arg("-C")
        .arg(external.path())
        .args(["config", "core.worktree"])
        .arg(template.path())
        .status()
        .unwrap();
    assert!(status.success());

    let _git_dir = EnvVarGuard::set("GIT_DIR", external.path().join(".git"));
    let prepared = prepare_template_source_from_base(
        &template.path().display().to_string(),
        Some(TemplateMode::Committed),
        None,
        template.path().parent().unwrap(),
    )
    .unwrap();

    assert_eq!(prepared.vcs_ref(), Some(local_commit.as_str()));
    let TemplateRenderSource::Filesystem(render_root) = prepared.render_source() else {
        panic!("local template must render from its worktree");
    };
    assert_eq!(
        fs::canonicalize(render_root).unwrap(),
        fs::canonicalize(template.path()).unwrap()
    );
    assert_eq!(
        git_stdout(external.path(), ["rev-parse", "HEAD"]).unwrap(),
        external_commit
    );
}

#[test]
fn committed_local_template_ignores_repository_replace_refs() {
    let template = materialize_template_git_worktree();
    let original = git_stdout(template.path(), ["rev-parse", "HEAD"]).unwrap();
    let guide = template.path().join("templates/project/AGENTS.md.jinja");
    fs::write(&guide, "replacement tree\n").unwrap();
    git(
        template.path(),
        ["add", "templates/project/AGENTS.md.jinja"],
    )
    .unwrap();
    git(template.path(), ["commit", "-m", "replacement object"]).unwrap();
    let replacement = git_stdout(template.path(), ["rev-parse", "HEAD"]).unwrap();
    git(template.path(), ["reset", "--hard", &original]).unwrap();
    git(template.path(), ["replace", &original, &replacement]).unwrap();

    let prepared = prepare_template_source_from_base(
        &template.path().display().to_string(),
        Some(TemplateMode::Committed),
        None,
        template.path().parent().unwrap(),
    )
    .unwrap();

    assert_eq!(prepared.vcs_ref(), Some(original.as_str()));
    assert_ne!(fs::read_to_string(guide).unwrap(), "replacement tree\n");
}

#[cfg(unix)]
#[test]
fn remote_template_clone_ignores_ambient_object_directory() {
    let _guard = lock_env();
    let template = materialize_template_git_worktree();
    let temp = tempdir().unwrap();
    let external_objects = temp.path().join("external-objects");
    fs::create_dir(&external_objects).unwrap();
    let remote = format!("file://{}", template.path().display());

    let _object_directory = EnvVarGuard::set("GIT_OBJECT_DIRECTORY", &external_objects);
    let prepared = prepare_template_source_from_base(&remote, None, None, temp.path()).unwrap();

    assert_eq!(fs::read_dir(&external_objects).unwrap().count(), 0);
    let TemplateRenderSource::Filesystem(render_root) = prepared.render_source() else {
        panic!("remote template must render from its checkout");
    };
    assert!(render_root.join(".git/objects").is_dir());
}

#[cfg(unix)]
#[test]
fn remote_template_clone_preserves_transport_policy_but_scrubs_repository_redirects() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let template = materialize_template_git_worktree();
    let temp = tempdir().unwrap();
    let log = temp.path().join("git-environment.log");
    let wrapper = temp.path().join("git-wrapper");
    fs::write(
        &wrapper,
        format!(
            r#"#!/bin/sh
phase=known
[ "${{2:-}}" = clone ] && phase=clone
printf '%s|%s|%s|%s|%s|%s|%s|%s|%s\n' \
  "$phase" \
  "${{GIT_SSL_CAINFO-unset}}" \
  "${{GIT_PROXY_COMMAND-unset}}" \
  "${{GIT_HTTP_PROXY_AUTHMETHOD-unset}}" \
  "${{GIT_SSL_NO_VERIFY-unset}}" \
  "${{GIT_DIR-unset}}" \
  "${{GIT_OBJECT_DIRECTORY-unset}}" \
  "${{GIT_INDEX_FILE-unset}}" \
  "${{GIT_TRACE-unset}}" >> "{log}"
exec git "$@"
"#,
            log = log.display(),
        ),
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let remote = format!("file://{}", template.path().display());

    let _git_bin = EnvVarGuard::set(GIT_BIN_ENV, &wrapper);
    let _ca = EnvVarGuard::set("GIT_SSL_CAINFO", "/trusted/ca.pem");
    let _proxy = EnvVarGuard::set("GIT_PROXY_COMMAND", "/trusted/proxy-command");
    let _proxy_auth = EnvVarGuard::set("GIT_HTTP_PROXY_AUTHMETHOD", "basic");
    let _no_verify = EnvVarGuard::set("GIT_SSL_NO_VERIFY", "1");
    let _git_dir = EnvVarGuard::set("GIT_DIR", temp.path().join("redirected.git"));
    let _objects = EnvVarGuard::set("GIT_OBJECT_DIRECTORY", temp.path().join("objects"));
    let _index = EnvVarGuard::set("GIT_INDEX_FILE", temp.path().join("index"));
    let _trace = EnvVarGuard::set("GIT_TRACE", "1");

    let prepared = prepare_template_source_from_base(&remote, None, None, temp.path()).unwrap();
    assert!(matches!(
        prepared.render_source(),
        TemplateRenderSource::Filesystem(_)
    ));

    let entries = fs::read_to_string(log).unwrap();
    let clone = entries
        .lines()
        .find(|entry| entry.starts_with("clone|"))
        .expect("clone invocation was logged");
    assert_eq!(
        clone,
        "clone|/trusted/ca.pem|/trusted/proxy-command|basic|1|unset|unset|unset|unset"
    );
    let known = entries
        .lines()
        .find(|entry| entry.starts_with("known|"))
        .expect("known-repository invocation was logged");
    assert_eq!(
        known, "known|unset|unset|unset|unset|unset|unset|unset|unset",
        "transport policy must not broaden strict known-repository commands"
    );
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

    let error_chain = format!("{error:#}");
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
if [ "$1" = "--no-replace-objects" ]; then
  shift
fi
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
