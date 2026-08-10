use super::*;

// agentic-loc-exception: legacy migration and launcher-only recovery scenarios share one end-to-end repository fixture suite.

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
    let answers = fs::read_to_string(&answers_template).unwrap().replace(
        "repo_name =",
        "jig_version = \"<<[ jig_version ]>>\"\nrepo_name =",
    );
    fs::write(&answers_template, answers).unwrap();

    let contract_template = template
        .path()
        .join("templates/project/.agent/jig-contract.json.jinja");
    let contract = fs::read_to_string(&contract_template).unwrap().replace(
        "\"contract_version\": <<[ _jig.contract_version ]>>,",
        "\"contract_version\": 3,\n  \"jig_version\": \"<<[ jig_version ]>>\",",
    );
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
fn full_update_retires_current_contract_repair_seed() {
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

    let cache = runtime_cache_base(&repo).join(runtime_profile_cache_name(
        CURRENT_CONTRACT_VERSION,
        RuntimeCacheProfile::Default,
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

    assert!(!cache.join(".jig-source-stamp").exists());
    assert!(!cache.join(".jig-source-metadata-stamp").exists());
    assert!(cache.join("bin/jig").exists());
}

#[test]
fn adopt_write_retires_current_contract_repair_seed() {
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

    assert!(!cache.join(".jig-source-stamp").exists());
    assert!(cache.join("bin/jig").exists());
}

#[test]
fn launcher_only_update_repairs_only_owned_runtime_scripts() {
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
    let mut answers = read_answers_toml(&answers_path).unwrap();
    answers.insert(
        "_src_path".into(),
        TomlValue::String("https://example.invalid/custom-jig.git".into()),
    );
    write_answers_toml(&answers_path, &answers).unwrap();
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

    fs::write(
        repo.join("scripts/jig"),
        "#!/bin/sh\nJIG_VERSION=\"0.2.0-beta.1\"\n",
    )
    .unwrap();
    fs::write(
        repo.join("scripts/install-jig.sh"),
        "#!/usr/bin/env bash\nJIG_VERSION=\"0.2.0-beta.1\"\n",
    )
    .unwrap();
    fs::write(repo.join(".mcp.json"), "{\"locally_modified\":true}\n").unwrap();
    fs::write(
        repo.join("AGENTS.md"),
        "project guidance\n<!-- BEGIN JIG MANAGED BLOCK -->\nmalformed\n",
    )
    .unwrap();

    let managed_paths = managed_paths::load_manifest(&repo).unwrap().unwrap();
    let before = managed_paths
        .iter()
        .map(|path| (path.clone(), fs::read(repo.join(path)).unwrap()))
        .collect::<BTreeMap<_, _>>();

    let output = run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: true,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    assert_eq!(output["render_mode"], "launcher-only");
    assert!(
        output["warnings"][0]
            .as_str()
            .is_some_and(|warning| warning.contains("embedded templates")
                && warning.contains("source-specific launcher customizations"))
    );
    for (path, contents) in &before {
        if LAUNCHER_ONLY_MANAGED_PATHS
            .iter()
            .any(|launcher| path == Path::new(launcher))
        {
            assert_ne!(&fs::read(repo.join(path)).unwrap(), contents, "{path:?}");
        } else {
            assert_eq!(&fs::read(repo.join(path)).unwrap(), contents, "{path:?}");
        }
    }
    let launcher = fs::read_to_string(repo.join("scripts/jig")).unwrap();
    assert!(!launcher.contains("JIG_VERSION="));
    assert!(launcher.contains("--__launcher-contract-version"));
    assert!(launcher.contains("CONTRACT_VERSION=\"3\""));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&contract_path).unwrap()).unwrap()["contract_version"],
        3
    );

    let after_first_repair = managed_paths
        .iter()
        .map(|path| (path.clone(), fs::read(repo.join(path)).unwrap()))
        .collect::<BTreeMap<_, _>>();
    run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: true,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();
    for (path, contents) in after_first_repair {
        assert_eq!(fs::read(repo.join(&path)).unwrap(), contents, "{path:?}");
    }
}

#[test]
fn launcher_only_update_preserves_every_file_when_force_is_required() {
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
    fs::write(repo.join("scripts/jig"), "# locally modified launcher\n").unwrap();
    let before = fs::read(repo.join("scripts/jig")).unwrap();

    let error = run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: true,
        force: false,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("--launcher-only requires --force"),
        "{error}"
    );
    assert_eq!(fs::read(repo.join("scripts/jig")).unwrap(), before);
}

#[test]
fn launcher_only_update_explains_minimal_footprint_mismatch() {
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
        "harness_footprint = \"full\"",
        "harness_footprint = \"minimal\"",
    );
    fs::write(&answers_path, answers).unwrap();
    fs::write(
        repo.join(managed_paths::MANIFEST_PATH),
        format!(
            "{{\n  \"version\": 1,\n  \"paths\": [\n    {:?}\n  ]\n}}\n",
            managed_paths::MANIFEST_PATH
        ),
    )
    .unwrap();

    let error = run_update(UpdateOpts {
        path: repo,
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: true,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("harness_footprint = \"minimal\""), "{error}");
    assert!(error.contains("do not manage scripts/jig"), "{error}");
    assert!(
        !error.contains("does not own these required managed paths"),
        "{error}"
    );
    assert!(!error.contains("template is missing"), "{error}");
}

#[test]
fn launcher_only_update_rejects_missing_source_before_mutating_scripts() {
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
    let mut answers = read_answers_toml(&answers_path).unwrap();
    answers.insert("_src_path".into(), TomlValue::String(String::new()));
    write_answers_toml(&answers_path, &answers).unwrap();
    let launcher_before = fs::read(repo.join("scripts/jig")).unwrap();
    let installer_before = fs::read(repo.join("scripts/install-jig.sh")).unwrap();

    let error = run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: true,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("non-empty _src_path"), "{error}");
    assert!(!error.contains("Failed to seed"), "{error}");
    assert_eq!(fs::read(repo.join("scripts/jig")).unwrap(), launcher_before);
    assert_eq!(
        fs::read(repo.join("scripts/install-jig.sh")).unwrap(),
        installer_before
    );
}

#[test]
fn launcher_only_update_rolls_back_scripts_when_runtime_seeding_fails() {
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
    fs::write(
        repo.join("scripts/jig"),
        "#!/bin/sh\nJIG_VERSION=\"0.2.0-beta.1\"\n",
    )
    .unwrap();
    fs::write(
        repo.join("scripts/install-jig.sh"),
        "#!/usr/bin/env bash\nJIG_VERSION=\"0.2.0-beta.1\"\n",
    )
    .unwrap();
    let launcher_before = fs::read(repo.join("scripts/jig")).unwrap();
    let installer_before = fs::read(repo.join("scripts/install-jig.sh")).unwrap();
    let _seed_failure = EnvVarGuard::set(TEST_FAIL_LAUNCHER_REPAIR_SEED_ENV, "1");

    let error = run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: true,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("injected launcher repair seed failure"),
        "{error}"
    );
    assert_eq!(fs::read(repo.join("scripts/jig")).unwrap(), launcher_before);
    assert_eq!(
        fs::read(repo.join("scripts/install-jig.sh")).unwrap(),
        installer_before
    );
}

#[test]
fn launcher_only_update_without_manifest_accepts_only_recognizable_legacy_scripts() {
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
    let manifest_path = repo.join(managed_paths::MANIFEST_PATH);
    fs::remove_file(&manifest_path).unwrap();

    fs::write(repo.join("scripts/jig"), "#!/bin/sh\n# project-owned\n").unwrap();
    fs::write(
        repo.join("scripts/install-jig.sh"),
        "#!/usr/bin/env bash\n# project-owned\n",
    )
    .unwrap();
    let error = run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: true,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("not a recognizable generated Jig launcher/installer pair"));
    assert_eq!(
        fs::read_to_string(repo.join("scripts/jig")).unwrap(),
        "#!/bin/sh\n# project-owned\n"
    );

    fs::write(
        repo.join("scripts/jig"),
        "#!/bin/sh\nINSTALLER=\"$ROOT_DIR/scripts/install-jig.sh\"\nJIG_VERSION=\"0.2.0-beta.1\"\n",
    )
    .unwrap();
    fs::write(
        repo.join("scripts/install-jig.sh"),
        "#!/usr/bin/env bash\nANSWERS_FILE=\"$ROOT_DIR/.jig.toml\"\nassert_exact_version() { :; }\n",
    )
    .unwrap();
    let error = run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: true,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("not a recognizable generated Jig launcher/installer pair"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let external_installer = temp.path().join("external-install-jig.sh");
        fs::write(
            &external_installer,
            "#!/usr/bin/env bash\nROOT_DIR=/tmp/repo\nANSWERS_FILE=\"$ROOT_DIR/.jig.toml\"\nassert_exact_version() { :; }\n",
        )
        .unwrap();
        fs::remove_file(repo.join("scripts/install-jig.sh")).unwrap();
        symlink(&external_installer, repo.join("scripts/install-jig.sh")).unwrap();

        let error = run_update(UpdateOpts {
            path: repo.clone(),
            template: None,
            template_mode: None,
            recopy: false,
            launcher_only: true,
            force: true,
            vcs_ref: None,
            defaults: true,
            no_input: true,
        })
        .unwrap_err()
        .to_string();
        assert!(error.contains("expected a regular generated file"));
        assert!(repo.join("scripts/install-jig.sh").is_symlink());
        assert!(external_installer.exists());
        fs::remove_file(repo.join("scripts/install-jig.sh")).unwrap();
    }

    fs::write(
        repo.join("scripts/jig"),
        r#"#!/bin/sh
set -eu
SCRIPT_DIR="$(dirname "$0")"
ROOT_DIR="$(CDPATH= cd "$SCRIPT_DIR/.." && pwd -P)"
INSTALLER="$ROOT_DIR/scripts/install-jig.sh"
JIG_VERSION="0.2.0-beta.1"
jig_help_requested_before_separator() { :; }
jig_subcommand() { :; }
binary_version() { :; }
use_matching_binary() { :; }
resolve_cached_binary() { :; }
resolve_mcp_binary() { :; }
actual_version="$(binary_version "$bin_path" || true)"
exec "$bin_path" "$@"
"#,
    )
    .unwrap();
    fs::write(
        repo.join("scripts/install-jig.sh"),
        r#"#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
ANSWERS_FILE="$ROOT_DIR/.jig.toml"
JIG_VERSION="0.2.0-beta.1"
read_field() { :; }
assert_exact_version() { :; }
acquire_install_lock() { :; }
install_from_local_source() { :; }
install_from_git_source() { :; }
printf '%s\n' "$BIN_PATH"
"#,
    )
    .unwrap();

    let output = run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: true,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    assert_eq!(output["render_mode"], "launcher-only");
    assert!(!manifest_path.exists());
    assert!(output["next_steps"][0].as_str().is_some_and(
        |step| step.contains("jig adopt") && step.contains(managed_paths::MANIFEST_PATH)
    ));
    let launcher = fs::read_to_string(repo.join("scripts/jig")).unwrap();
    assert!(launcher.contains("--__launcher-contract-version"));
    assert!(!launcher.contains("JIG_VERSION="));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&contract_path).unwrap()).unwrap()["contract_version"],
        3
    );

    run_update(UpdateOpts {
        path: repo.clone(),
        template: None,
        template_mode: None,
        recopy: false,
        launcher_only: true,
        force: true,
        vcs_ref: None,
        defaults: true,
        no_input: true,
    })
    .unwrap();

    let error = run_update(UpdateOpts {
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
    .unwrap_err()
    .to_string();
    assert!(error.contains(managed_paths::MANIFEST_PATH), "{error}");

    with_test_build_template_pin_policy(BuildTemplatePinPolicy::Unreleased, || {
        run_adopt(AdoptOpts {
            path: repo.clone(),
            template: None,
            template_mode: None,
            vcs_ref: None,
            force: true,
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

    assert!(manifest_path.exists());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&contract_path).unwrap()).unwrap()["contract_version"],
        4
    );
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
}
