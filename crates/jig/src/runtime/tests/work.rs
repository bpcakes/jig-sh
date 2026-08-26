use super::*;
use crate::tool_defs::WORKER_RUN_TOOL;
use std::path::Path;

fn write_v6_schema_check_fixture(root: &Path, effects: &[&str]) {
    write_v6_evidence_fixture_repo(root, "");
    fs::create_dir_all(root.join("docs/schema")).unwrap();
    fs::write(root.join("docs/schema/schema.sql"), "schema\n").unwrap();
    let config_path = root.join(".jig.toml");
    let effects_toml = serde_json::to_string(effects).unwrap();
    let action = format!(
        r#"[[repository.actions]]
target = {{ component = "api", action = "schema" }}
intent = "check"
effects = {effects_toml}
runner = {{ kind = "native", operation = "jig.schema_check" }}
inputs = ["api/**"]

[[repository.actions]]
target = {{ component = "api", action = "schema-dump" }}
intent = "generate"
effects = ["worktree", "process"]
runner = {{ kind = "command", command = "api_schema_dump_command" }}
inputs = ["api/**"]
legacy_aliases = ["jig.schema_dump"]

[[repository.profiles]]"#
    );
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace(
            "default_branch = \"main\"",
            "default_branch = \"main\"\nschema_dump_enabled = true",
        )
        .replace(
            "[commands]",
            "[commands]\napi_schema_dump_command = \"printf 'schema\\n' > docs/schema/schema.sql\"",
        )
        .replace("adapters = [\"go\"]", "adapters = [\"go\", \"sqlx\"]")
        .replace("[[repository.profiles]]", &action);
    fs::write(config_path, config).unwrap();
    let manifest_path = root.join(".agent/jig-contract.json");
    let mut manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["required_commands"]
        .as_array_mut()
        .unwrap()
        .push(json!("api_schema_dump_command"));
    manifest["components"][0]["adapters"] = json!(["go", "sqlx"]);
    manifest["tools"].as_array_mut().unwrap().push(json!({
        "name": "jig.schema_dump",
        "kind": "command",
        "description": "Dump the schema.",
        "command": "api_schema_dump_command"
    }));
    manifest["actions"].as_array_mut().unwrap().extend([
        json!({
            "target": {"component": "api", "action": "schema"},
            "intent": "check",
            "effects": effects,
            "runner": {"kind": "native", "operation": "jig.schema_check"},
            "inputs": ["api/**"]
        }),
        json!({
            "target": {"component": "api", "action": "schema-dump"},
            "intent": "generate",
            "effects": ["worktree", "process"],
            "runner": {"kind": "command", "command": "api_schema_dump_command"},
            "inputs": ["api/**"],
            "legacy_aliases": ["jig.schema_dump"]
        }),
    ]);
    fs::write(
        manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

#[test]
fn cli_dispatch_requires_manifest_tool_declaration() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckOpts::with_command(
            crate::cli::CheckCommand::Fmt(crate::cli::CheckTargetOpts {
                tool: crate::cli::ToolOpts {
                    plan_id: None,
                    no_receipt: false,
                },
                selectors: Vec::new(),
            }),
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Tool is not declared in .agent/jig-contract.json"));
}

#[test]
fn unavailable_schema_check_explains_disabled_config() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckOpts::with_command(
            crate::cli::CheckCommand::Schema(crate::cli::CheckTargetOpts {
                tool: crate::cli::ToolOpts {
                    plan_id: None,
                    no_receipt: false,
                },
                selectors: Vec::new(),
            }),
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("jig.schema_check is not available"));
    assert!(error.contains("sqlx_enabled = false"));
    assert!(error.contains("jig update --recopy"));
}

#[test]
fn explicit_schema_check_executes_a_declared_read_only_action_on_contract_six() {
    let temp = tempdir().unwrap();
    write_v6_schema_check_fixture(temp.path(), &["read_only", "process"]);
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckOpts::with_command(
            crate::cli::CheckCommand::Schema(crate::cli::CheckTargetOpts {
                tool: crate::cli::ToolOpts {
                    plan_id: None,
                    no_receipt: false,
                },
                selectors: Vec::new(),
            }),
        )),
    )
    .unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(output["run"]["conclusion"], "success");
}

#[test]
fn explicit_schema_check_rejects_a_declared_worktree_effect() {
    let temp = tempdir().unwrap();
    write_v6_schema_check_fixture(temp.path(), &["worktree", "process"]);
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckOpts::with_command(
            crate::cli::CheckCommand::Schema(crate::cli::CheckTargetOpts {
                tool: crate::cli::ToolOpts {
                    plan_id: None,
                    no_receipt: false,
                },
                selectors: Vec::new(),
            }),
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("not a read-only check"), "{error}");
}

#[test]
fn effectful_v6_compatibility_alias_refreshes_authority_after_checkout_wait() {
    use std::sync::mpsc;
    use std::time::Duration;

    let temp = tempdir().unwrap();
    write_v6_schema_check_fixture(temp.path(), &["read_only", "process"]);
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let reader = crate::state::acquire_repository_execution_lease(
        &ctx,
        &[jig_contract::ActionEffect::ReadOnly],
    )
    .unwrap();
    let worker_ctx = ctx;
    let (attempting_tx, attempting_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();

    std::thread::scope(|scope| {
        scope.spawn(move || {
            attempting_tx.send(()).unwrap();
            result_tx
                .send(super::super::dispatch(
                    &worker_ctx,
                    crate::command::RuntimeCommand::Sqlx(crate::command::SqlxCommand::SchemaDump(
                        crate::command::ToolRequest::default(),
                    )),
                ))
                .unwrap();
        });

        attempting_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        if let Ok(result) = result_rx.recv_timeout(Duration::from_millis(100)) {
            panic!(
                "effectful compatibility alias completed while a checkout reader was active: {result:#?}"
            );
        }
        let config_path = temp.path().join(".jig.toml");
        let mut config =
            toml::from_str::<toml::Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();
        config["commands"]["api_schema_dump_command"] = toml::Value::String(
            "printf 'refreshed authority\\n'; printf 'schema\\n' > docs/schema/schema.sql".into(),
        );
        fs::write(config_path, toml::to_string_pretty(&config).unwrap()).unwrap();
        let refreshed = RepoContext::load_from(temp.path()).unwrap();
        assert!(
            refreshed
                .command_for_key("api_schema_dump_command")
                .unwrap()
                .contains("refreshed authority")
        );
        drop(reader);
        let output = result_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        assert_eq!(output["ok"], true, "{output:#}");
        assert_eq!(output["result"]["stdout"], "refreshed authority\n");
    });
}

#[test]
fn unavailable_go_checks_explain_backend_and_database_capabilities() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let lint_error = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckOpts::with_command(
            crate::cli::CheckCommand::Lint(crate::cli::CheckTargetOpts {
                tool: crate::cli::ToolOpts {
                    plan_id: None,
                    no_receipt: false,
                },
                selectors: Vec::new(),
            }),
        )),
    )
    .unwrap_err()
    .to_string();
    assert!(lint_error.contains("backend_language is not \"go\""));
    assert!(lint_error.contains("check clippy"));

    let sqlc_error = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckOpts::with_command(
            crate::cli::CheckCommand::Sqlc(crate::cli::CheckTargetOpts {
                tool: crate::cli::ToolOpts {
                    plan_id: None,
                    no_receipt: false,
                },
                selectors: Vec::new(),
            }),
        )),
    )
    .unwrap_err()
    .to_string();
    assert!(sqlc_error.contains("sqlc checks belong to a Go/PostgreSQL backend"));

    let go = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(go.path())
        .contract_version(5)
        .config("backend_language = \"go\"\ngo_database = \"none\"")
        .write();
    let ctx = RepoContext::load_from(go.path()).unwrap();
    let error = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckOpts::with_command(
            crate::cli::CheckCommand::Sqlc(crate::cli::CheckTargetOpts {
                tool: crate::cli::ToolOpts {
                    plan_id: None,
                    no_receipt: false,
                },
                selectors: Vec::new(),
            }),
        )),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("go_database is not \"postgres\""));
}

#[test]
fn unavailable_typescript_check_explains_missing_contract_tool() {
    let temp = tempdir().unwrap();
    crate::test_env::TestRepoBuilder::new(temp.path())
        .config(
            r#"
[[frontend_apps]]
name = "web"
dir = "apps/web"
coverage_threshold = 80
"#,
        )
        .required_commands(Vec::<String>::new())
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckOpts::with_command(
            crate::cli::CheckCommand::TypeScriptLint(crate::cli::CheckTargetOpts {
                tool: crate::cli::ToolOpts {
                    plan_id: None,
                    no_receipt: false,
                },
                selectors: Vec::new(),
            }),
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("jig.typescript_lint is not declared"),
        "{error}"
    );
    assert!(error.contains("jig update --recopy"), "{error}");
    assert!(error.contains("project-owned [commands]"), "{error}");
}

#[test]
fn work_goal_opens_durable_plan_and_prompt() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: "Reduce API handler duplication".into(),
            success: "duplication is reduced and the configured gate passes".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: vec!["Do not change public routes".into()],
            checkpoints: vec!["Capture baseline gate status".into()],
            title: Some("API goal".into()),
            notes: Some("Prefer small commits.".into()),
        })),
    )
    .unwrap();

    let plan_id = output["plan"]["plan_id"].as_str().unwrap();
    let body_path = output["plan"]["body_path"].as_str().unwrap();
    let body = fs::read_to_string(temp.path().join(body_path)).unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert!(
        output["goal_prompt"]
            .as_str()
            .unwrap()
            .starts_with("/goal ")
    );
    assert!(output["goal_prompt"].as_str().unwrap().contains(plan_id));
    assert!(body.contains("# Goal Harness"));
    assert!(body.contains("Reduce API handler duplication"));
    assert!(body.contains("- scripts/jig work check"));
    assert!(body.contains("- [ ] Capture baseline gate status"));
    assert!(body.contains("custom: check (jig.custom_check)"));
    assert_eq!(
        output["commands"]["gates"],
        format!("scripts/jig work gates --plan-id {plan_id}")
    );
}

#[test]
fn work_start_validates_plan_body_before_starting_session() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let sessions_path = ctx.state_file("sessions.jsonl");
    let receipts_path = ctx.state_file("receipts.jsonl");
    let sessions_before = fs::read_to_string(&sessions_path).unwrap_or_default();
    let receipts_before = fs::read_to_string(&receipts_path).unwrap_or_default();

    for opts in [
        crate::cli::WorkStartOpts {
            title: "Conflicting body".into(),
            body: Some("inline".into()),
            body_file: Some(temp.path().join("plan.md")),
            print_plan_id: false,
        },
        crate::cli::WorkStartOpts {
            title: "Missing body file".into(),
            body: None,
            body_file: Some(temp.path().join("missing-plan.md")),
            print_plan_id: false,
        },
    ] {
        dispatch(
            &ctx,
            CommandKind::Work(crate::cli::WorkCommand::Start(opts)),
        )
        .unwrap_err();
    }

    assert_eq!(
        fs::read_to_string(&sessions_path).unwrap_or_default(),
        sessions_before
    );
    assert_eq!(
        fs::read_to_string(&receipts_path).unwrap_or_default(),
        receipts_before
    );
    assert_eq!(crate::state::current_session(&ctx).unwrap(), None);
}

#[test]
fn work_goal_rejects_blank_required_fields() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let blank_validation = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: "Reduce API handler duplication".into(),
            success: "duplication is reduced".into(),
            validations: vec!["   ".into()],
            constraints: Vec::new(),
            checkpoints: Vec::new(),
            title: None,
            notes: None,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(blank_validation.contains("--validation values cannot be empty"));

    let blank_objective = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: " \n\t ".into(),
            success: "duplication is reduced".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: Vec::new(),
            checkpoints: Vec::new(),
            title: None,
            notes: None,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(blank_objective.contains("--objective cannot be empty"));

    let blank_success = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: "Reduce API handler duplication".into(),
            success: " \n\t ".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: Vec::new(),
            checkpoints: Vec::new(),
            title: None,
            notes: None,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(blank_success.contains("--success cannot be empty"));
}

#[test]
fn work_goal_normalizes_prompt_and_defaults_missing_checkpoints() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: "Reduce API handler duplication".into(),
            success: "duplication is reduced\nand the configured gate passes".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: Vec::new(),
            checkpoints: Vec::new(),
            title: None,
            notes: None,
        })),
    )
    .unwrap();

    let body_path = output["plan"]["body_path"].as_str().unwrap();
    let body = fs::read_to_string(temp.path().join(body_path)).unwrap();
    let prompt = output["goal_prompt"].as_str().unwrap();

    assert!(prompt.contains("duplication is reduced and the configured gate passes"));
    assert!(!prompt.contains("reduced\nand"));
    assert!(body.contains("duplication is reduced\nand the configured gate passes"));
    assert!(body.contains("- [ ] Read the relevant AGENTS.md files and repo guidance."));
}

#[test]
fn work_goal_rejects_blank_checkpoints_when_provided() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: "Reduce API handler duplication".into(),
            success: "duplication is reduced".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: Vec::new(),
            checkpoints: vec!["   ".into()],
            title: None,
            notes: None,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("--checkpoint values cannot be empty"));
}

#[test]
fn work_goal_rejects_blank_constraints_when_provided() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: "Reduce API handler duplication".into(),
            success: "duplication is reduced".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: vec!["   ".into()],
            checkpoints: Vec::new(),
            title: None,
            notes: None,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("--constraint values cannot be empty"));
}

#[test]
fn work_goal_truncates_generated_title_to_eighty_chars() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let objective =
        "Reduce API handler duplication while preserving every public route and fixture behavior";

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: objective.into(),
            success: "duplication is reduced".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: Vec::new(),
            checkpoints: Vec::new(),
            title: None,
            notes: None,
        })),
    )
    .unwrap();

    let plan_id = output["plan"]["plan_id"].as_str().unwrap();
    let plans = fs::read_to_string(temp.path().join(".agent/state/plans.jsonl")).unwrap();
    let plan_line = plans
        .lines()
        .find(|line| line.contains(plan_id))
        .expect("goal plan event should be recorded");
    let title = serde_json::from_str::<Value>(plan_line).unwrap()["title"]
        .as_str()
        .unwrap()
        .to_string();

    assert_eq!(title.chars().count(), 80);
    assert!(title.ends_with("..."));
}

#[test]
fn work_goal_defaults_blank_title_to_generated_title() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: "Reduce API handler duplication".into(),
            success: "duplication is reduced".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: Vec::new(),
            checkpoints: Vec::new(),
            title: Some("   ".into()),
            notes: None,
        })),
    )
    .unwrap();

    let plan_id = output["plan"]["plan_id"].as_str().unwrap();
    let plans = fs::read_to_string(temp.path().join(".agent/state/plans.jsonl")).unwrap();
    let plan_line = plans
        .lines()
        .find(|line| line.contains(plan_id))
        .expect("goal plan event should be recorded");
    let title = serde_json::from_str::<Value>(plan_line).unwrap()["title"]
        .as_str()
        .unwrap()
        .to_string();

    assert_eq!(title, "Reduce API handler duplication");
}

fn read_receipts(root: &Path) -> Vec<Value> {
    fs::read_to_string(root.join(".agent/state/receipts.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect()
}

mod checks;
mod evidence;
mod gate_receipt_ordering;
mod gates;
mod review;
