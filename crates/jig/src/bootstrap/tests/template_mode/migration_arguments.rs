use super::*;
use crate::command::{MigrationAddRequest, RuntimeCommand, ToolRequest};
use crate::context::{CURRENT_CONTRACT_VERSION, RepoContext};

#[test]
fn action_arguments_update_preserves_command_migration_alias() {
    assert_command_migration_alias_survives_update(false);
}

#[test]
fn action_arguments_recopy_preserves_command_migration_alias() {
    assert_command_migration_alias_survives_update(true);
}

fn assert_command_migration_alias_survives_update(recopy: bool) {
    let _guard = lock_env();
    let template = materialize_template_git_worktree();
    for version in [6, 7] {
        let temp = tempdir().unwrap();
        let repo = temp.path().join("ExampleProject");
        write_test_crate_guide(&repo);
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
                backend_language: Some(BackendLanguage::Rust),
                sqlx_enabled: Some(false),
                ..AnswerOpts::default()
            },
        })
        .unwrap();

        let command = "printf '%s' \"$NAME\" > migration-name.txt";
        let action = serde_json::json!({
            "target": {"component": "repo", "action": "migration-add"},
            "intent": "generate", "effects": ["worktree", "process"],
            "runner": {"kind": "command", "command": "example_migration_command"},
            "legacy_aliases": ["jig.migration_add"]
        });
        let source_path = repo.join(".jig.toml");
        let mut source = read_answers_toml(&source_path).unwrap();
        source["commands"].as_table_mut().unwrap().insert(
            "example_migration_command".into(),
            TomlValue::String(command.into()),
        );
        source["repository"]["actions"]
            .as_array_mut()
            .unwrap()
            .push(TomlValue::try_from(&action).unwrap());
        write_answers_toml(&source_path, &source).unwrap();
        let manifest_path = repo.join(".agent/jig-contract.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest["contract_version"] = serde_json::json!(version);
        // This fixture deliberately recreates a pre-v8 source. Fresh rendering
        // now uses explicit shell runners and freshness policy fields, which
        // those old epochs reject.
        for action in source["repository"]["actions"].as_array_mut().unwrap() {
            action.as_table_mut().unwrap().remove("inputs_policy");
            action.as_table_mut().unwrap().remove("source_state");
            if let Some(provenance) = action.get_mut("provenance") {
                provenance.as_table_mut().unwrap().remove("inputs_policy");
                provenance.as_table_mut().unwrap().remove("source_state");
            }
            if action["runner"]["kind"].as_str() == Some("shell") {
                action["runner"]["kind"] = TomlValue::String("command".into());
            }
        }
        for action in manifest["actions"].as_array_mut().unwrap() {
            action.as_object_mut().unwrap().remove("inputs_policy");
            action.as_object_mut().unwrap().remove("source_state");
            if let Some(provenance) = action.get_mut("provenance") {
                provenance.as_object_mut().unwrap().remove("inputs_policy");
                provenance.as_object_mut().unwrap().remove("source_state");
            }
            if action["runner"]["kind"] == "shell" {
                action["runner"]["kind"] = serde_json::json!("command");
            }
        }
        write_answers_toml(&source_path, &source).unwrap();
        manifest["actions"]
            .as_array_mut()
            .unwrap()
            .push(action.clone());
        manifest["required_commands"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!("example_migration_command"));
        manifest["tools"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "name": "jig.migration_add", "kind": "command",
                "command": "example_migration_command", "description": "Create example migration"
            }));
        // Contract v6 predates the generated native file-budget action.
        if version == 6 {
            remove_file_budget(&mut source, &mut manifest);
            write_answers_toml(&source_path, &source).unwrap();
        }
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();

        // A long shell-looking value proves both legacy length compatibility and
        // byte-exact environment delivery before and after the real transaction.
        let name = format!("Create Examples $(touch injected) {}", "x".repeat(201));
        assert_migration_alias(&repo, version, &name);
        run_update(UpdateOpts {
            path: repo.clone(),
            template: None,
            template_mode: None,
            recopy,
            launcher_only: false,
            force: true,
            vcs_ref: None,
            defaults: true,
            no_input: true,
        })
        .unwrap();
        assert_migration_alias(&repo, CURRENT_CONTRACT_VERSION, &name);
        let mut expected_runner = action["runner"].clone();
        expected_runner["kind"] = serde_json::json!("shell");

        let updated = RepoContext::load_from(&repo).unwrap();
        let catalog = crate::repository::RepositoryCatalog::from_context(&updated).unwrap();
        let preserved = catalog.action_for_alias("jig.migration_add").unwrap();
        assert_eq!(
            serde_json::to_value(preserved).unwrap()["runner"],
            expected_runner
        );
        assert!(preserved.arguments.is_empty());
        let source = read_answers_toml(&source_path).unwrap();
        let authored = source["repository"]["actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["target"]["action"].as_str() == Some("migration-add"))
            .unwrap();
        assert_eq!(
            serde_json::to_value(&authored["runner"]).unwrap(),
            expected_runner
        );
        assert_eq!(
            source["commands"]["example_migration_command"].as_str(),
            Some(command)
        );
    }
}

fn assert_migration_alias(repo: &Path, version: u32, name: &str) {
    let ctx = RepoContext::load_from(repo).unwrap();
    assert_eq!(ctx.contract_version(), version);
    crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::MigrationAdd(MigrationAddRequest {
            name: name.into(),
            tool: ToolRequest::default(),
        }),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(repo.join("migration-name.txt")).unwrap(),
        name
    );
    assert!(!repo.join("injected").exists());
    fs::remove_file(repo.join("migration-name.txt")).unwrap();
}

fn remove_file_budget(
    source: &mut toml::map::Map<String, TomlValue>,
    manifest: &mut serde_json::Value,
) {
    source["repository"]["actions"]
        .as_array_mut()
        .unwrap()
        .retain(|a| a["target"]["action"].as_str() != Some("file-budget"));
    manifest["actions"]
        .as_array_mut()
        .unwrap()
        .retain(|a| a["target"]["action"] != "file-budget");
    for profile in source["repository"]["profiles"].as_array_mut().unwrap() {
        profile["targets"]
            .as_array_mut()
            .unwrap()
            .retain(|t| t["action"].as_str() != Some("file-budget"));
    }
    for profile in manifest["profiles"].as_array_mut().unwrap() {
        profile["targets"]
            .as_array_mut()
            .unwrap()
            .retain(|t| t["action"] != "file-budget");
    }
    manifest["tools"]
        .as_array_mut()
        .unwrap()
        .retain(|t| t["name"] != "jig.file_budget");
}
