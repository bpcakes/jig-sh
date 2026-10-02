use super::*;

#[test]
fn recopy_preserves_legacy_check_policy_and_tracker_in_pinned_templates() {
    let _guard = lock_env();
    for (contract_version, use_check_gate) in [(5, false), (5, true), (8, false)] {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("ExampleProject");
        let template = materialize_template_git_worktree();
        write_test_crate_guide(&repo);
        for relative in [
            "templates/project/.jig.toml.jinja",
            "templates/project/.agent/jig-contract.json.jinja",
            "templates/project/scripts/jig.jinja",
        ] {
            let path = template.path().join(relative);
            let contents = fs::read_to_string(&path)
                .unwrap()
                .replace("_jig.contract_version", &contract_version.to_string())
                .replace("id = \"contract\"", "id = \"jig-contract\"");
            fs::write(path, contents).unwrap();
        }
        git(template.path(), ["add", "."]).unwrap();
        git(
            template.path(),
            ["commit", "-m", "pinned legacy template fixture"],
        )
        .unwrap();

        run_adopt(AdoptOpts {
            components: Default::default(),
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
                repo_name: Some("ExampleProject".into()),
                backend_language: Some(crate::backend::BackendLanguage::Rust),
                sqlx_enabled: Some(false),
                ..AnswerOpts::default()
            },
        })
        .unwrap();
        let config_path = repo.join(".jig.toml");
        let mut config =
            toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
        let work = config
            .as_table_mut()
            .unwrap()
            .entry("work")
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()
            .unwrap();
        work.insert(
            "receipt_metadata".into(),
            toml::Value::Array(vec![toml::Value::from("beads")]),
        );
        if contract_version == 5 {
            let gates = if use_check_gate {
                let mut gates = vec![toml::Value::Table(toml::Table::from_iter([
                    ("id".into(), toml::Value::from("project-locked")),
                    ("kind".into(), toml::Value::from("check")),
                    ("tool".into(), toml::Value::from("jig.test_locked")),
                ]))];
                // Exact generated policy must win over the later legacy alias.
                for (id, required) in [("jig-contract", false), ("contract", true)] {
                    gates.push(toml::Value::Table(toml::Table::from_iter([
                        ("id".into(), toml::Value::from(id)),
                        ("kind".into(), toml::Value::from("check")),
                        ("tool".into(), toml::Value::from("jig.contract_check")),
                        ("required".into(), toml::Value::from(required)),
                    ])));
                }
                gates
            } else {
                work.insert(
                    "checks".into(),
                    toml::Value::Array(vec![toml::Value::from("jig.test_locked")]),
                );
                Vec::new()
            };
            work.insert("gates".into(), toml::Value::Array(gates));
        }
        fs::write(&config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
        let initial = RepoContext::load_from(&repo).unwrap();
        assert_eq!(initial.contract_version(), contract_version);
        if contract_version == 5 {
            assert!(
                initial
                    .work_check_tools()
                    .iter()
                    .any(|tool| tool == "jig.test_locked")
            );
        }

        let output = run_update(UpdateOpts {
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

        let config =
            toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(
            config["work"]["receipt_metadata"][0].as_str(),
            Some("beads")
        );
        assert!(
            config
                .get("repository")
                .is_none_or(|repository| repository.get("tracker").is_none())
        );
        if use_check_gate {
            let gates = config["work"]["gates"].as_array().unwrap();
            let contract = gates
                .iter()
                .find(|gate| gate["id"].as_str() == Some("jig-contract"))
                .unwrap();
            assert_eq!(contract["required"].as_bool(), Some(false));
            assert!(
                gates
                    .iter()
                    .all(|gate| gate["id"].as_str() != Some("contract"))
            );
        }
        let refreshed = RepoContext::load_from(&repo).unwrap();
        assert_eq!(refreshed.contract_version(), contract_version);
        if contract_version == 5 {
            let catalog = crate::repository::RepositoryCatalog::from_context(&refreshed).unwrap();
            let targets = &catalog
                .profile(catalog.default_check_profile().unwrap())
                .unwrap()
                .targets;
            assert_eq!(
                targets.iter().map(ToString::to_string).collect::<Vec<_>>(),
                ["repo:contract", "repo:test", "repo:test-locked"]
            );
        }
        assert_eq!(crate::policy::contract_check(&refreshed).exit_status, 0);
        assert!(output["warnings"].as_array().unwrap().is_empty());
    }
}
