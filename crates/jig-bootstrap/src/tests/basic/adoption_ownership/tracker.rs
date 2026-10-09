use super::*;

#[test]
fn full_readoption_from_contract_eight_moves_tracker_ownership_and_reports_dropped_work() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    configure_frontend_fixture(&repo);

    let mut initial = footprint_adopt_opts(&repo, template.path(), false, false);
    initial.answers.sqlx_enabled = Some(true);
    initial.answers.schema_dump_enabled = Some(true);
    initial.answers.rust_migration_dir = Some("migrations".into());
    initial.answers.web_package_manager = Some("npm".into());
    initial.answers.frontend_apps = vec![frontend_app()];
    run_adopt(initial).unwrap();
    downgrade_to_contract_eight(&repo);

    let config_path = repo.join(".jig.toml");
    let mut config =
        toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let work = config["work"].as_table_mut().unwrap();
    work.insert(
        "receipt_metadata".into(),
        toml::Value::Array(vec![toml::Value::from("beads")]),
    );
    work.insert(
        "checks".into(),
        toml::Value::Array(
            [
                "jig.sqlx_check",
                "jig.schema_check",
                "jig.typescript_lint",
                "jig.fmt_check",
            ]
            .into_iter()
            .map(|tool| toml::Value::String(tool.into()))
            .collect(),
        ),
    );
    let gates = work["gates"].as_array_mut().unwrap();
    for gate in gates.iter_mut() {
        let gate = gate.as_table_mut().unwrap();
        if gate["id"].as_str().unwrap() == "verify" {
            gate.remove("profile");
            gate.insert("target".into(), toml::Value::String("api:test".into()));
            gate.insert("required".into(), toml::Value::Boolean(false));
        }
    }
    gates.push(toml::Value::Table(toml::Table::from_iter([
        ("id".into(), toml::Value::String("project-fmt".into())),
        ("kind".into(), toml::Value::String("check".into())),
        ("tool".into(), toml::Value::String("jig.fmt_check".into())),
        ("required".into(), toml::Value::Boolean(false)),
    ])));
    gates.push(toml::Value::Table(toml::Table::from_iter([
        ("id".into(), toml::Value::String("project-review".into())),
        ("kind".into(), toml::Value::String("codex_review".into())),
        ("skill".into(), toml::Value::String("cc:review".into())),
        ("fail_on".into(), toml::Value::String("warning".into())),
        ("scope".into(), toml::Value::String("uncommitted".into())),
        ("model".into(), toml::Value::String("gpt-5".into())),
    ])));
    gates.push(toml::Value::Table(toml::Table::from_iter([
        ("id".into(), toml::Value::String("project-evidence".into())),
        ("kind".into(), toml::Value::String("evidence".into())),
        ("profile".into(), toml::Value::String("verify".into())),
        ("required".into(), toml::Value::Boolean(false)),
    ])));
    work.insert(
        "refinements".into(),
        toml::Value::Array(vec![toml::Value::Table(toml::Table::from_iter([
            (
                "id".into(),
                toml::Value::String("project-refinement".into()),
            ),
            (
                "skill".into(),
                toml::Value::String("jig-rust:rust-simplify".into()),
            ),
            ("mode".into(), toml::Value::String("write".into())),
            ("model".into(), toml::Value::String("gpt-5".into())),
        ]))]),
    );
    // Explicitly remove frontend ownership from the authored model. Deleting
    // manifests alone must no longer cause readoption to discard components.
    config["frontend_apps"] = toml::Value::Array(Vec::new());
    let components = config["repository"]["components"].as_array_mut().unwrap();
    let frontend_ids = components
        .iter()
        .filter(|component| {
            component["adapters"]
                .as_array()
                .unwrap()
                .iter()
                .any(|adapter| adapter.as_str() == Some("typescript"))
        })
        .map(|component| component["id"].as_str().unwrap().to_owned())
        .collect::<BTreeSet<_>>();
    components.retain(|component| !frontend_ids.contains(component["id"].as_str().unwrap()));
    config["repository"]["actions"]
        .as_array_mut()
        .unwrap()
        .retain(|action| {
            !frontend_ids.contains(action["target"]["component"].as_str().unwrap())
                && !action["target"]["action"]
                    .as_str()
                    .unwrap()
                    .starts_with("typescript-")
        });
    for profile in config["repository"]["profiles"].as_array_mut().unwrap() {
        profile["targets"].as_array_mut().unwrap().retain(|target| {
            !frontend_ids.contains(target["component"].as_str().unwrap())
                && !target["action"]
                    .as_str()
                    .unwrap()
                    .starts_with("typescript-")
        });
    }
    fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
    jig_context::RepoContext::load_from(&repo).unwrap();
    fs::remove_file(repo.join("apps/web/package.json")).unwrap();
    fs::remove_file(repo.join("package.json")).unwrap();
    fs::remove_file(repo.join("package-lock.json")).unwrap();

    let output = run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap();

    let config =
        toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
            .unwrap();
    assert_tracker_ownership(&config);
    assert_contains_note(
        &output["notes"],
        &["`[work] receipt_metadata = [\"beads\"]` moves to `[repository] tracker = \"beads\"`"],
    );
    assert_contains_note(
        &output["notes"],
        &[
            "Retired [work] settings are dropped from .jig.toml: ",
            "`checks`",
            "gates `verify`, `project-fmt`, `project-review`, `project-evidence`",
            "`refinements`",
        ],
    );
    let ctx = jig_context::RepoContext::load_from_root(repo).unwrap();
    assert_eq!(
        ctx.contract_version(),
        jig_context::CURRENT_CONTRACT_VERSION
    );
    assert_eq!(jig_policy::contract_check(&ctx).exit_status, 0);
}

#[test]
fn invalid_runtime_config_is_repaired_without_dropping_tracker_ownership() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    for update in [false, true] {
        let repo = temp.path().join(if update { "update" } else { "readopt" });
        fs::create_dir_all(&repo).unwrap();
        run_adopt(footprint_adopt_opts(&repo, template.path(), false, false)).unwrap();
        add_project_runtime_tables(&repo);

        let config_path = repo.join(".jig.toml");
        let mut config =
            toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
        config
            .as_table_mut()
            .unwrap()
            .insert("commands".into(), toml::Value::String("invalid".into()));
        fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
        assert!(jig_context::RepoContext::validate_config_file(&repo).is_err());

        if update {
            run_update(update_opts(&repo, template.path(), false)).unwrap();
        } else {
            run_adopt(footprint_adopt_opts(&repo, template.path(), false, true)).unwrap();
        }

        let repaired =
            toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
                .unwrap();
        assert!(repaired["commands"].as_table().is_some());
        assert!(repaired["commands"]["api_test_command"].as_str().is_some());
        assert_tracker_ownership(&repaired);
        jig_context::RepoContext::load_from(&repo).unwrap();
    }
}

#[test]
fn update_does_not_enable_tracker_ownership_when_it_was_never_declared() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    for minimal in [true, false] {
        let repo = temp.path().join(if minimal { "minimal" } else { "full" });
        fs::create_dir_all(&repo).unwrap();
        run_adopt(footprint_adopt_opts(&repo, template.path(), minimal, false)).unwrap();

        run_update(update_opts(&repo, template.path(), false)).unwrap();

        let config =
            toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
                .unwrap();
        assert!(config.get("work").is_none());
        assert!(config["repository"].get("tracker").is_none());
        jig_context::RepoContext::load_from(&repo).unwrap();
    }
}

#[test]
fn update_and_readoption_refuse_to_delete_invalid_tracker_ownership() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    for refresh in ["update", "readopt"] {
        for (name, minimal, contract_eight, table, field, value) in [
            (
                "full-tracker",
                false,
                false,
                "repository",
                "tracker",
                toml::Value::from("linear"),
            ),
            (
                "minimal-receipt-metadata",
                true,
                true,
                "work",
                "receipt_metadata",
                toml::Value::Integer(7),
            ),
        ] {
            let repo = temp.path().join(format!("{refresh}-{name}"));
            fs::create_dir_all(&repo).unwrap();
            run_adopt(footprint_adopt_opts(&repo, template.path(), minimal, false)).unwrap();
            if contract_eight {
                downgrade_to_contract_eight(&repo);
            }
            let config_path = repo.join(".jig.toml");
            let mut config =
                toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
            config[table]
                .as_table_mut()
                .unwrap()
                .insert(field.into(), value);
            let authored = toml::to_string_pretty(&config).unwrap();
            fs::write(&config_path, &authored).unwrap();

            let error = if refresh == "update" {
                run_update(update_opts(&repo, template.path(), false)).unwrap_err()
            } else {
                run_adopt(footprint_adopt_opts(&repo, template.path(), minimal, true)).unwrap_err()
            };

            assert!(
                error
                    .to_string()
                    .contains(&format!("existing [{table}].{field} is invalid")),
                "{error:#}"
            );
            assert_eq!(fs::read_to_string(config_path).unwrap(), authored);
        }
    }
}

#[test]
fn update_from_contract_eight_moves_tracker_ownership_and_drops_work() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();

    for minimal in [true, false] {
        let repo = temp.path().join(if minimal { "minimal" } else { "full" });
        fs::create_dir_all(&repo).unwrap();
        run_adopt(footprint_adopt_opts(&repo, template.path(), minimal, false)).unwrap();
        downgrade_to_contract_eight(&repo);
        add_contract_eight_work_authority(&repo);

        // Moving to a new contract rewrites the manifest, which needs --force.
        let output = run_update(update_opts(&repo, template.path(), true)).unwrap();

        let config =
            toml::from_str::<toml::Value>(&fs::read_to_string(repo.join(".jig.toml")).unwrap())
                .unwrap();
        assert_tracker_ownership(&config);
        assert_contains_note(
            &output["warnings"],
            &[
                "`[work] receipt_metadata = [\"beads\"]` moves to `[repository] tracker = \"beads\"`",
            ],
        );
        assert_contains_note(
            &output["warnings"],
            &["Retired [work] settings are dropped from .jig.toml: `checks`; `tracker`."],
        );
        let ctx = jig_context::RepoContext::load_from_root(repo.clone()).unwrap();
        assert_eq!(
            ctx.contract_version(),
            jig_context::CURRENT_CONTRACT_VERSION
        );
    }
}
