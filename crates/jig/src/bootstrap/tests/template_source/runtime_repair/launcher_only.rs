use super::*;

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
