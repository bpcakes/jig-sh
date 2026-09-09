use super::*;
use crate::command::{RepositoryRunRequest, RuntimeCommand, ToolRequest};

fn fixture() -> tempfile::TempDir {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    add_v6_generate_action(temp.path());
    let declarations = json!({
        "message": {"type": "string", "required": true, "max_bytes": 64},
        "optional": {"type": "string", "allow_empty": true, "max_bytes": 8}
    });
    update_action(temp.path(), "generate", declarations, 8, false);
    init_git_repo(temp.path());
    temp
}

fn update_action(
    root: &std::path::Path,
    action: &str,
    declarations: Value,
    version: u32,
    alias: bool,
) {
    let config_path = root.join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let actions = config["repository"]["actions"].as_array_mut().unwrap();
    let source = actions
        .iter_mut()
        .find(|a| a["target"]["action"].as_str() == Some(action))
        .unwrap();
    source.as_table_mut().unwrap().insert(
        "arguments".into(),
        toml::Value::try_from(&declarations).unwrap(),
    );
    if alias {
        source.as_table_mut().unwrap().insert(
            "legacy_aliases".into(),
            toml::Value::try_from(vec![tool::MIGRATION_ADD]).unwrap(),
        );
    }
    if version >= 8 {
        for source in actions {
            if source["runner"]["kind"].as_str() == Some("command") {
                source["runner"]["kind"] = toml::Value::String("shell".into());
            }
            if source["target"]["action"].as_str() == Some("generate") {
                source["runner"] = toml::Value::try_from(json!({
                    "kind": "argv", "program": "python3",
                    "args": ["-c", "import json, pathlib, sys; pathlib.Path('generated.txt').write_text('generated'); pathlib.Path('arguments.json').write_text(json.dumps(sys.argv[1:]))",
                        {"argument": "message"}, {"argument": "optional"}]
                })).unwrap();
            }
        }
    }
    fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
    let path = root.join(".agent/jig-contract.json");
    let mut manifest: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    manifest["contract_version"] = json!(version);
    manifest["actions"] = serde_json::to_value(&config["repository"]["actions"]).unwrap();
    if alias {
        manifest["tools"].as_array_mut().unwrap().push(json!({
            "name": tool::MIGRATION_ADD, "kind": "native", "description": "Create migration"
        }));
    }
    fs::write(path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
}

fn request(target: &str, values: Vec<String>) -> RepositoryRunRequest {
    RepositoryRunRequest {
        selectors: vec![target.into()],
        arguments: crate::repository::arguments::parse_cli(values).unwrap(),
        profile: None,
        affected_base: None,
        comparison: None,
        explain: true,
        fail_fast: false,
        approved_effects: vec![],
        tool: ToolRequest::default(),
    }
}

#[test]
fn action_arguments_cli_mcp_canonical_identity_and_literal_execution() {
    let temp = fixture();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let literal = "  $(touch injected); x=y\n";
    let mut args = request(
        "api:generate",
        vec![
            "api:generate:optional=".into(),
            format!("api:generate:message={literal}"),
        ],
    );
    let cli = crate::runtime::dispatch(&ctx, RuntimeCommand::Run(args.clone())).unwrap();
    let mcp = call_tool(
        &ctx,
        tool::PLAN_RUN,
        json!({
            "selectors": ["api:generate"],
            "arguments": {"api:generate": {"message": literal, "optional": ""}}
        }),
    )
    .unwrap();
    assert_eq!(cli["plan"], mcp["plan"]);
    assert_eq!(cli["plan"]["targets"][0]["arguments"]["message"], literal);
    assert_repository_output_schema(&ctx, tool::PLAN_RUN, &mcp);
    let changed = call_tool(
        &ctx,
        tool::PLAN_RUN,
        json!({
            "selectors": ["api:generate"], "arguments": {"api:generate": {"message": "different"}}
        }),
    )
    .unwrap();
    assert_ne!(cli["plan"]["id"], changed["plan"]["id"]);
    let omitted = call_tool(
        &ctx,
        tool::PLAN_RUN,
        json!({
            "selectors": ["api:generate"], "arguments": {"api:generate": {"message": literal}}
        }),
    )
    .unwrap();
    assert_ne!(cli["plan"]["id"], omitted["plan"]["id"]);
    let mut tampered = mcp["plan"].clone();
    tampered["targets"][0]["arguments"]["message"] = json!("tampered");
    assert!(
        call_tool(
            &ctx,
            tool::EXECUTE_RUN,
            json!({"plan": tampered, "approved_effects": ["worktree"]})
        )
        .is_err()
    );
    assert!(!ctx.state_file("runs.jsonl").exists());
    args.explain = false;
    args.approved_effects = vec![jig_contract::ActionEffect::Worktree];
    let output = crate::runtime::dispatch(&ctx, RuntimeCommand::Run(args)).unwrap();
    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(
        fs::read_to_string(temp.path().join("generated.txt")).unwrap(),
        "generated"
    );
    let captured: Value =
        serde_json::from_str(&fs::read_to_string(temp.path().join("arguments.json")).unwrap())
            .unwrap();
    assert_eq!(captured, json!([literal, ""]));
    assert!(!temp.path().join("injected").exists());
}

#[test]
fn action_arguments_reject_invalid_requests_before_execution() {
    let temp = fixture();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    for (values, error) in [
        (vec![], "requires string argument"),
        (vec!["api:generate:message=".into()], "forbids empty"),
        (
            vec![format!("api:generate:message={}", "é".repeat(33))],
            "exceeds 64 bytes",
        ),
        (vec!["api:generate:unknown=value".into()], "does not accept"),
        (vec!["web:test:message=value".into()], "unselected target"),
        (vec!["api:generate:message=a\0b".into()], "contains NUL"),
    ] {
        let mut args = request("api:generate", values);
        let mcp_args = args
            .arguments
            .iter()
            .map(|(target, values)| (target.to_string(), json!(values)))
            .collect::<serde_json::Map<_, _>>();
        args.explain = false;
        args.approved_effects = vec![jig_contract::ActionEffect::Worktree];
        let cli_error = crate::runtime::dispatch(&ctx, RuntimeCommand::Run(args)).unwrap_err();
        let mcp_error = call_tool(
            &ctx,
            tool::PLAN_RUN,
            json!({"selectors": ["api:generate"], "arguments": mcp_args}),
        )
        .unwrap_err();
        assert!(format!("{cli_error:#}").contains(error), "{cli_error:#}");
        assert!(format!("{mcp_error:#}").contains(error), "{mcp_error:#}");
        assert!(!ctx.state_file("runs.jsonl").exists());
        assert!(!ctx.state_file("receipts.jsonl").exists());
        assert!(!temp.path().join("generated.txt").exists());
    }
    for value in [
        json!(null),
        json!(4),
        json!(["value"]),
        json!({"nested": "value"}),
    ] {
        assert!(call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:generate"], "arguments": {"api:generate": {"message": value}}})).is_err());
    }
}

#[test]
fn action_arguments_native_migration_and_compatibility_alias_agree() {
    for version in [6, 7, 8] {
        let temp = tempdir().unwrap();
        write_v6_evidence_fixture_repo(temp.path(), "");
        add_v6_native_migration_action(temp.path());
        let declaration = if version == 8 {
            json!({"name": {"type": "string", "required": true, "max_bytes": 200}})
        } else {
            json!({})
        };
        update_action(temp.path(), "migration-add", declaration, version, true);
        init_git_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let name = if version < 8 {
            "x".repeat(201)
        } else {
            "Create Examples".into()
        };
        let mut args = request(
            "api:migration-add",
            vec![format!("api:migration-add:name={name}")],
        );
        let cli = crate::runtime::dispatch(&ctx, RuntimeCommand::Run(args.clone())).unwrap();
        let mcp = call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:migration-add"], "arguments": {"api:migration-add": {"name": name}}})).unwrap();
        assert_eq!(cli["plan"], mcp["plan"]);
        let mut invalid_names = vec!["".to_owned(), "-unsafe".into()];
        if version == 8 {
            invalid_names.extend(["___".into(), "x".repeat(201)]);
        }
        for name in invalid_names {
            assert!(call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:migration-add"], "arguments": {"api:migration-add": {"name": name}}})).is_err());
            let alias_error = crate::runtime::dispatch(
                &ctx,
                RuntimeCommand::MigrationAdd(crate::command::MigrationAddRequest {
                    name,
                    tool: ToolRequest::default(),
                }),
            )
            .unwrap_err();
            assert!(
                format!("{alias_error:#}").contains("argument 'name'"),
                "{alias_error:#}"
            );
            assert!(!temp.path().join("migrations").exists());
        }
        args.explain = false;
        args.approved_effects = vec![jig_contract::ActionEffect::Worktree];
        let run = crate::runtime::dispatch(&ctx, RuntimeCommand::Run(args)).unwrap();
        assert_eq!(run["ok"], true, "{run:#}");
        let fresh = call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:migration-add"], "arguments": {"api:migration-add": {"name": name}}})).unwrap();
        let accepted = call_tool(
            &ctx,
            tool::EXECUTE_RUN,
            json!({"plan": fresh["plan"], "approved_effects": ["worktree"]}),
        )
        .unwrap();
        let terminal = wait_for_repository_run(&ctx, accepted["run_id"].as_str().unwrap());
        assert_eq!(
            terminal["result"]["run"]["result"]["conclusion"], "success",
            "{terminal:#}"
        );
        let alias = crate::runtime::dispatch(
            &ctx,
            RuntimeCommand::MigrationAdd(crate::command::MigrationAddRequest {
                name: name.clone(),
                tool: ToolRequest::default(),
            }),
        )
        .unwrap();
        assert_eq!(alias["name"], name);
        let files = fs::read_dir(temp.path().join("migrations"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(files.len(), 3);
        let suffix = format!("_{}.sql", name.to_lowercase().replace(' ', "_"));
        assert!(
            files
                .iter()
                .all(|file| file.to_string_lossy().ends_with(&suffix))
        );
    }
}

mod runners;
