use super::*;

#[test]
fn full_update_recovers_missing_and_malformed_contract_manifests() {
    let _guard = lock_env();
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

    let contract_path = repo.join(".agent/jig-contract.json");
    for damaged_contract in [None, Some("{\n")] {
        match damaged_contract {
            Some(contents) => fs::write(&contract_path, contents).unwrap(),
            None => fs::remove_file(&contract_path).unwrap(),
        }

        run_update(UpdateOpts {
            path: repo.clone(),
            template: None,
            template_mode: None,
            recopy: false,
            launcher_only: false,
            force: true,
            vcs_ref: None,
            defaults: true,
            no_input: true,
        })
        .unwrap();

        let contract: serde_json::Value =
            serde_json::from_slice(&fs::read(&contract_path).unwrap()).unwrap();
        assert_eq!(contract["contract_version"], CURRENT_CONTRACT_VERSION);
        RepoContext::load_from_root(repo.clone()).unwrap();
    }
}

#[test]
fn recopy_renders_committed_pre_v4_template_with_legacy_jig_version() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    let answers_template = template.path().join("templates/project/.jig.toml.jinja");
    let answers = fs::read_to_string(&answers_template)
        .unwrap()
        .replace(
            "repo_name =",
            "jig_version = \"<<[ jig_version ]>>\"\nrepo_name =",
        )
        .replace("_jig.contract_version", "3");
    fs::write(&answers_template, answers).unwrap();

    let contract_template = template
        .path()
        .join("templates/project/.agent/jig-contract.json.jinja");
    let contract = fs::read_to_string(&contract_template)
        .unwrap()
        .replace(
            "\"contract_version\": <<[ _jig.contract_version ]>>,",
            "\"contract_version\": 3,\n  \"jig_version\": \"<<[ jig_version ]>>\",",
        )
        .replace("_jig.contract_version", "3");
    fs::write(&contract_template, contract).unwrap();

    let launcher_template = template.path().join("templates/project/scripts/jig.jinja");
    let launcher = fs::read_to_string(&launcher_template).unwrap().replace(
        "CONTRACT_VERSION=\"<<[ _jig.contract_version ]>>\"",
        "JIG_VERSION=\"<<[ jig_version ]>>\"\nCONTRACT_VERSION=\"3\"",
    );
    fs::write(&launcher_template, launcher).unwrap();
    git(template.path(), ["add", "."]).unwrap();
    git(template.path(), ["commit", "-m", "pre-v4 template fixture"]).unwrap();

    run_adopt(AdoptOpts {
        path: repo.clone(),
        template: Some(template.path().display().to_string()),
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
            jig_version: Some("0.2.0-beta.1".into()),
            sqlx_enabled: Some(false),
            ..AnswerOpts::default()
        },
    })
    .unwrap();

    run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: true,
        launcher_only: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    assert!(
        fs::read_to_string(repo.join(".jig.toml"))
            .unwrap()
            .contains("jig_version = \"0.2.0-beta.1\"")
    );
    assert!(
        fs::read_to_string(repo.join("scripts/jig"))
            .unwrap()
            .contains("JIG_VERSION=\"0.2.0-beta.1\"")
    );
    let contract: serde_json::Value =
        serde_json::from_slice(&fs::read(repo.join(".agent/jig-contract.json")).unwrap()).unwrap();
    assert_eq!(contract["contract_version"], 3);
    assert_eq!(contract["jig_version"], "0.2.0-beta.1");
}

#[test]
fn adopt_rejects_current_contract_template_with_legacy_launcher_protocol() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    let template = materialize_template_git_worktree();
    write_test_crate_guide(&repo);

    let launcher_template = template.path().join("templates/project/scripts/jig.jinja");
    let launcher = fs::read_to_string(&launcher_template)
        .unwrap()
        .replace(
            "# jig-generated-runtime-launcher:v1",
            "# legacy-runtime-launcher",
        )
        .replace(
            "# jig-runtime-repository-scope:v1",
            "# legacy-runtime-scope",
        );
    fs::write(&launcher_template, launcher).unwrap();
    git(template.path(), ["add", "."]).unwrap();
    git(
        template.path(),
        ["commit", "-m", "legacy launcher protocol fixture"],
    )
    .unwrap();

    let error = run_adopt(AdoptOpts {
        path: repo,
        template: Some(template.path().display().to_string()),
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
    .unwrap_err()
    .to_string();

    assert!(error.contains("launcher"), "{error}");
    assert!(
        error.contains("repository-scoped runtime protocol"),
        "{error}"
    );
}

#[test]
fn full_update_upgrades_legacy_contract_and_launcher_together() {
    let _guard = lock_env();
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

    let answers_path = repo.join(".jig.toml");
    let answers = fs::read_to_string(&answers_path).unwrap().replace(
        "template_source_url =",
        "jig_version = \"0.2.0-beta.1\"\ntemplate_source_url =",
    );
    fs::write(&answers_path, answers).unwrap();
    let contract_path = repo.join(".agent/jig-contract.json");
    let mut contract: serde_json::Value =
        serde_json::from_slice(&fs::read(&contract_path).unwrap()).unwrap();
    contract["contract_version"] = json!(3);
    contract["jig_version"] = json!("0.2.0-beta.1");
    fs::write(
        &contract_path,
        format!("{}\n", serde_json::to_string_pretty(&contract).unwrap()),
    )
    .unwrap();

    run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let contract: serde_json::Value =
        serde_json::from_slice(&fs::read(&contract_path).unwrap()).unwrap();
    assert_eq!(contract["contract_version"], CURRENT_CONTRACT_VERSION);
    assert!(contract.get("jig_version").is_none());
    let launcher = fs::read_to_string(repo.join("scripts/jig")).unwrap();
    assert!(launcher.contains(&format!("CONTRACT_VERSION=\"{CURRENT_CONTRACT_VERSION}\"")));
    assert!(
        !fs::read_to_string(answers_path)
            .unwrap()
            .contains("jig_version")
    );
    RepoContext::load_from_root(repo).unwrap();
}

#[test]
fn embedded_full_update_replaces_current_contract_repair_seed() {
    let _guard = lock_env();
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

    let profile = if cfg!(feature = "dev-proxy") {
        RuntimeCacheProfile::Default
    } else {
        RuntimeCacheProfile::Runtime
    };
    let cache = runtime_cache_base(&repo).join(runtime_profile_cache_name(
        CURRENT_CONTRACT_VERSION,
        profile,
    ));
    fs::create_dir_all(cache.join("bin")).unwrap();
    fs::write(cache.join("bin/jig"), "repair runtime").unwrap();
    fs::write(
        cache.join(".jig-source-stamp"),
        format!("{LAUNCHER_REPAIR_SEED_STAMP_HEADER}\nsource:fixture\n"),
    )
    .unwrap();
    fs::write(cache.join(".jig-source-metadata-stamp"), "metadata\n").unwrap();

    run_update(UpdateOpts {
        path: repo,
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: false,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    assert_eq!(
        fs::read_to_string(cache.join(".jig-source-stamp")).unwrap(),
        "jig-embedded-runtime-v1\nsource:fixture\n"
    );
    assert!(!cache.join(".jig-source-metadata-stamp").exists());
    assert!(cache.join("bin/jig").exists());
}

#[test]
fn embedded_adopt_replaces_current_contract_repair_seed() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo = temp.path().join("repo");
    write_test_crate_guide(&repo);
    let cache = runtime_cache_base(&repo).join(runtime_profile_cache_name(
        CURRENT_CONTRACT_VERSION,
        RuntimeCacheProfile::Runtime,
    ));
    fs::create_dir_all(cache.join("bin")).unwrap();
    fs::write(cache.join("bin/jig"), "repair runtime").unwrap();
    fs::write(
        cache.join(".jig-source-stamp"),
        format!("{LAUNCHER_REPAIR_SEED_STAMP_HEADER}\nsource:fixture\n"),
    )
    .unwrap();

    with_test_build_template_pin_policy(BuildTemplatePinPolicy::Unreleased, || {
        run_adopt(AdoptOpts {
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
        .unwrap()
    });

    assert_eq!(
        fs::read_to_string(cache.join(".jig-source-stamp")).unwrap(),
        "jig-embedded-runtime-v1\nsource:fixture\n"
    );
    assert!(cache.join("bin/jig").exists());
}

mod launcher_only;
