use super::*;
use crate::bootstrap::template_source::{
    PreparedTemplateSource, StoredTemplateState, test_final_update_template_state,
    test_resolve_update_template_source,
};

#[test]
fn init_rejects_vcs_ref_for_non_git_local_template() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let destination = temp.path().join("repo");
    let template = materialize_template_worktree();

    fs::remove_dir_all(template.path().join(".git")).ok();

    let error = run_init(InitOpts {
        path: destination,
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: Some("main".into()),
        force: false,
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

    assert!(
        error.contains("--vcs-ref only applies to remote templates or local git template paths")
    );
}

#[test]
fn update_allows_embedded_source_to_switch_to_committed_checkout() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    write_test_crate_guide(&repo);
    let template = materialize_template_git_worktree();
    commit_template_root_guide(template.path(), "# Checkout Marker\n", "template update");

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
        .unwrap();
    });

    run_update(UpdateOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        recopy: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let root_guide = fs::read_to_string(repo.join("AGENTS.md")).unwrap();
    assert!(root_guide.contains("Checkout Marker"));

    let expected_local_path = fs::canonicalize(template.path())
        .unwrap()
        .display()
        .to_string();
    let answers = read_answers_toml(&repo.join(".jig.toml")).unwrap();
    assert_eq!(
        answers.get("_src_path").and_then(TomlValue::as_str),
        Some(expected_local_path.as_str())
    );
    assert_eq!(
        answers.get(TEMPLATE_MODE_KEY).and_then(TomlValue::as_str),
        Some(TemplateMode::Committed.as_str())
    );
    assert_eq!(
        answers
            .get(TEMPLATE_LOCAL_PATH_KEY)
            .and_then(TomlValue::as_str),
        Some(expected_local_path.as_str())
    );
}

#[test]
fn update_committed_mode_rewrites_normalized_remote_source_to_local_checkout() {
    let _guard = lock_env();
    let fixture = NormalizedRemoteCommittedFixture::new(false);

    commit_template_root_guide(
        fixture.template.path(),
        "# Local Checkout Marker\n",
        "template update",
    );
    run_update(UpdateOpts {
        path: fixture.repo.clone(),
        template: Some(fixture.template.path().display().to_string()),
        template_mode: Some(TemplateMode::Committed),
        recopy: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let root_guide = fs::read_to_string(fixture.repo.join("AGENTS.md")).unwrap();
    assert!(root_guide.contains("Local Checkout Marker"));

    let answers = read_answers_toml(&fixture.answers_path).unwrap();
    let expected_local_path = fs::canonicalize(fixture.template.path())
        .unwrap()
        .display()
        .to_string();
    assert_eq!(
        answers.get("_src_path").and_then(TomlValue::as_str),
        Some(expected_local_path.as_str())
    );
    assert_eq!(
        answers.get(TEMPLATE_MODE_KEY).and_then(TomlValue::as_str),
        Some(TemplateMode::Committed.as_str())
    );
    assert_eq!(
        answers
            .get(TEMPLATE_LOCAL_PATH_KEY)
            .and_then(TomlValue::as_str),
        Some(expected_local_path.as_str())
    );
}

#[test]
fn update_committed_mode_uses_unpushed_local_checkout_for_normalized_remote_source() {
    let _guard = lock_env();
    let fixture = NormalizedRemoteCommittedFixture::new(false);
    let local_commit = commit_template_root_guide(
        fixture.template.path(),
        "# Unpushed Local Checkout Marker\n",
        "template update",
    );

    run_update(UpdateOpts {
        path: fixture.repo.clone(),
        template: None,
        template_mode: Some(TemplateMode::Committed),
        recopy: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let root_guide = fs::read_to_string(fixture.repo.join("AGENTS.md")).unwrap();
    assert!(root_guide.contains("Unpushed Local Checkout Marker"));

    let answers = read_answers_toml(&fixture.answers_path).unwrap();
    let expected_local_path = fs::canonicalize(fixture.template.path())
        .unwrap()
        .display()
        .to_string();
    assert_eq!(
        answers.get("_src_path").and_then(TomlValue::as_str),
        Some(expected_local_path.as_str())
    );
    assert_eq!(
        answers.get("_commit").and_then(TomlValue::as_str),
        Some(local_commit.as_str())
    );
    assert_eq!(
        answers
            .get(TEMPLATE_LOCAL_PATH_KEY)
            .and_then(TomlValue::as_str),
        Some(expected_local_path.as_str())
    );
}

#[test]
fn update_committed_mode_with_vcs_ref_uses_local_checkout_for_normalized_remote_source() {
    let _guard = lock_env();
    let fixture = NormalizedRemoteCommittedFixture::new(false);
    let old_ref = commit_template_root_guide(
        fixture.template.path(),
        "# Older Marker\n",
        "older template",
    );
    push_template_main(fixture.template.path(), &fixture.remote_url);
    commit_template_root_guide(
        fixture.template.path(),
        "# Newer Marker\n",
        "newer template",
    );
    push_template_main(fixture.template.path(), &fixture.remote_url);

    run_update(UpdateOpts {
        path: fixture.repo.clone(),
        template: None,
        template_mode: Some(TemplateMode::Committed),
        recopy: false,
        force: true,
        vcs_ref: Some(old_ref.clone()),
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let root_guide = fs::read_to_string(fixture.repo.join("AGENTS.md")).unwrap();
    assert!(root_guide.contains("Older Marker"));
    assert!(!root_guide.contains("Newer Marker"));

    assert_eq!(
        read_optional_answer_string(&fixture.answers_path, "_commit")
            .unwrap()
            .as_deref(),
        Some(old_ref.as_str())
    );
}

#[test]
fn update_committed_mode_with_vcs_ref_accepts_legacy_normalized_remote_source() {
    let _guard = lock_env();
    let fixture = NormalizedRemoteCommittedFixture::new(true);
    let old_ref = commit_template_root_guide(
        fixture.template.path(),
        "# Older Marker\n",
        "older template",
    );
    push_template_main(fixture.template.path(), &fixture.remote_url);
    commit_template_root_guide(
        fixture.template.path(),
        "# Newer Marker\n",
        "newer template",
    );
    push_template_main(fixture.template.path(), &fixture.remote_url);

    run_update(UpdateOpts {
        path: fixture.repo.clone(),
        template: None,
        template_mode: Some(TemplateMode::Committed),
        recopy: false,
        force: true,
        vcs_ref: Some(old_ref.clone()),
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let root_guide = fs::read_to_string(fixture.repo.join("AGENTS.md")).unwrap();
    assert!(root_guide.contains("Older Marker"));
    assert!(!root_guide.contains("Newer Marker"));

    let answers = read_answers_toml(&fixture.answers_path).unwrap();
    assert_eq!(
        answers.get("_src_path").and_then(TomlValue::as_str),
        Some(fixture.remote_url.as_str())
    );
    assert_eq!(
        answers
            .get(TEMPLATE_LOCAL_PATH_KEY)
            .and_then(TomlValue::as_str),
        Some("")
    );
    assert_eq!(
        answers.get(TEMPLATE_MODE_KEY).and_then(TomlValue::as_str),
        Some(TemplateMode::Committed.as_str())
    );
    assert_eq!(
        answers.get("_commit").and_then(TomlValue::as_str),
        Some(old_ref.as_str())
    );
}

#[test]
fn update_committed_mode_rejects_explicit_remote_template_with_template_mode() {
    let _guard = lock_env();
    let fixture = NormalizedRemoteCommittedFixture::new(false);

    let error = run_update(UpdateOpts {
        path: fixture.repo,
        template: Some(fixture.remote_url),
        template_mode: Some(TemplateMode::Committed),
        recopy: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("--template-mode only applies to local git template paths."));
    assert!(error.contains("Omit --template-mode for remote templates"));
}

#[test]
fn update_committed_mode_accepts_explicit_normalized_remote_template_source() {
    let _guard = lock_env();
    let fixture = NormalizedRemoteCommittedFixture::new(false);
    let new_commit = commit_template_root_guide(
        fixture.template.path(),
        "# Explicit Remote Marker\n",
        "template update",
    );
    push_template_main(fixture.template.path(), &fixture.remote_url);

    run_update(UpdateOpts {
        path: fixture.repo.clone(),
        template: Some(fixture.remote_url.clone()),
        template_mode: None,
        recopy: false,
        force: true,
        vcs_ref: Some("main".into()),
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let root_guide = fs::read_to_string(fixture.repo.join("AGENTS.md")).unwrap();
    assert!(root_guide.contains("Explicit Remote Marker"));
    let expected_local_path = fs::canonicalize(fixture.template.path())
        .unwrap()
        .display()
        .to_string();
    let answers = read_answers_toml(&fixture.answers_path).unwrap();
    assert_eq!(
        answers.get("_src_path").and_then(TomlValue::as_str),
        Some(fixture.remote_url.as_str())
    );
    assert_eq!(
        answers.get("_commit").and_then(TomlValue::as_str),
        Some(new_commit.as_str())
    );
    assert_eq!(
        answers.get(TEMPLATE_MODE_KEY).and_then(TomlValue::as_str),
        Some(TemplateMode::Committed.as_str())
    );
    assert_eq!(
        answers
            .get(TEMPLATE_LOCAL_PATH_KEY)
            .and_then(TomlValue::as_str),
        Some(expected_local_path.as_str())
    );
}

#[test]
fn final_update_template_state_keeps_prepared_source_for_committed_local_checkout() {
    let stored = StoredTemplateState::test_committed(
        "https://example.com/template.git",
        Some("/tmp/template".into()),
    );
    let prepared = PreparedTemplateSource::test_local(
        "/tmp/template".into(),
        PathBuf::from("/tmp/template"),
        Some("deadbeef".into()),
        PrivateAnswerOverrides::test_committed("/tmp/template"),
    );

    let final_template = test_final_update_template_state(&stored, &prepared);

    assert_eq!(final_template.source(), prepared.source());
    assert_eq!(final_template.vcs_ref(), prepared.vcs_ref());
}

#[test]
fn resolve_update_template_source_prefers_stored_local_checkout_for_explicit_committed_mode() {
    let stored = StoredTemplateState::test_committed(
        "https://example.com/template.git",
        Some("/tmp/template".into()),
    );
    let opts = UpdateOpts {
        path: PathBuf::from("."),
        template: None,
        template_mode: Some(TemplateMode::Committed),
        recopy: false,
        force: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    };

    let (template, template_mode) = test_resolve_update_template_source(&opts, &stored).unwrap();

    assert_eq!(template, "/tmp/template");
    assert_eq!(template_mode, Some(TemplateMode::Committed));
}

#[test]
fn resolve_update_template_source_falls_back_to_remote_for_legacy_committed_repo() {
    let stored = StoredTemplateState::test_committed("https://example.com/template.git", None);
    let opts = UpdateOpts {
        path: PathBuf::from("."),
        template: None,
        template_mode: Some(TemplateMode::Committed),
        recopy: false,
        force: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    };

    let (template, template_mode) = test_resolve_update_template_source(&opts, &stored).unwrap();

    assert_eq!(template, "https://example.com/template.git");
    assert_eq!(template_mode, None);
}
