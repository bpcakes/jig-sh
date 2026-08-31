use super::*;

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
if [ "$1" = "rev-parse" ] && [ "$2" = "--show-toplevel" ]; then
  printf '{repo}\n'
  exit 0
fi
if [ "$1" = "rev-parse" ] && [ "$2" = "--git-path" ]; then
  printf '.git/jig\n'
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
            repo = repo.display(),
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
    let launcher = fs::read_to_string(repo.join("scripts/jig")).unwrap();
    assert!(launcher.contains("--repository-scope"));
    assert!(launcher.contains("jig_capability_only_requested"));
    let installer = fs::read_to_string(repo.join("scripts/install-jig.sh")).unwrap();
    assert!(installer.contains("resolve_compatible_path_jig"));
    assert!(installer.contains("--capability-only"));
    assert!(installer.contains("JIG_INSTALL_ALLOW_PATH_BINARY"));
    assert!(installer.contains("Using explicitly allowed PATH Jig binary"));
    assert!(installer.contains(r#"[[ "$source" == "embedded:jig-sh" ]]"#));
    assert!(installer.contains("no contract-compatible Jig binary was found"));
    assert!(installer.contains("JIG_INSTALL_ALLOW_EMBEDDED_SOURCE_FALLBACK=1"));
    assert!(installer.contains("JIG_INSTALL_ALLOW_UNPINNED_REMOTE=1"));
    assert!(installer.contains("local cargo_args=(--git \"$SRC_PATH\")"));
    assert!(!installer.contains("git_ref_args"));
    assert!(installer.contains("--no-textconv HEAD -- Cargo.toml Cargo.lock crates"));
    assert!(installer.contains("ls-files --others --exclude-standard -z"));
    assert!(installer.contains("hash-object --no-filters"));
}
