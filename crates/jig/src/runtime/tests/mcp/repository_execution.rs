// agentic-loc-exception: MCP repository execution isolation and cancellation cases share one end-to-end runtime fixture.

use super::*;

#[test]
fn targets_without_a_worktree_effect_cannot_mutate_the_repository() {
    for (action, effect) in [
        ("external-operation", "external"),
        ("process-operation", "process"),
    ] {
        let temp = tempdir().unwrap();
        write_v6_evidence_fixture_repo(temp.path(), "");
        add_v6_mutating_effect_action(temp.path(), action, effect);
        init_git_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
        let plan = crate::repository::plan_action_run(
            &ctx,
            &catalog,
            crate::repository::PlanRunRequest {
                selectors: vec![format!("api:{action}")],
                profile: None,
                affected_base: None,
            },
            Default::default(),
        )
        .unwrap();
        let (run, _lease) =
            crate::runtime::run_execution::start_check_run(&ctx, &catalog, plan, None).unwrap();

        let execution = crate::runtime::run_execution::execute_started_check_run(
            &ctx,
            &catalog,
            run,
            crate::runtime::run_execution::ExecuteCheckRunRequest {
                work_plan_id: None,
                record_receipts: true,
                fail_fast: false,
            },
            &|| Ok(false),
        )
        .unwrap();

        assert_eq!(
            execution.run.result.targets[0].conclusion,
            Some(jig_contract::RunConclusion::Failure),
            "{action}"
        );
        assert!(
            execution.run.result.targets[0]
                .findings
                .iter()
                .any(|finding| finding.source.as_deref() == Some("effect_policy")),
            "{action}"
        );
        assert!(temp.path().join(format!("{action}-mutation.txt")).exists());
    }
}

#[test]
fn mcp_repository_cancel_is_cooperative_idempotent_and_cleans_registry() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    fs::write(
        &config_path,
        config.replace(
            "printf 'api tests passed\\n'",
            "printf 'started\\n'; sleep 30; printf 'done\\n' > api-finished.txt",
        ),
    )
    .unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let planned = call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:test"]})).unwrap();
    let accepted = call_tool(
        &ctx,
        tool::EXECUTE_RUN,
        json!({"plan": planned["plan"].clone()}),
    )
    .unwrap();
    let run_id = accepted["run_id"].as_str().unwrap();

    let cancelled = call_tool(&ctx, tool::CANCEL_RUN, json!({"run_id": run_id})).unwrap();
    assert_repository_output_schema(&ctx, tool::CANCEL_RUN, &cancelled);
    assert_eq!(cancelled["cancellation_requested"], true);
    assert_eq!(cancelled["worker_signalled"], true);

    let terminal = wait_for_repository_run(&ctx, run_id);
    assert_eq!(
        terminal["result"]["run"]["result"]["conclusion"],
        "cancelled"
    );
    let registry_deadline = Instant::now() + Duration::from_secs(1);
    while crate::runtime::mcp_repository::is_live_run_registered(&ctx, run_id) {
        assert!(
            Instant::now() < registry_deadline,
            "completed run remained in the live registry"
        );
        thread::sleep(Duration::from_millis(10));
    }
    thread::sleep(Duration::from_millis(100));
    assert!(!temp.path().join("api-finished.txt").exists());

    let repeated = call_tool(&ctx, tool::CANCEL_RUN, json!({"run_id": run_id})).unwrap();
    assert_eq!(repeated["ok"], true);
    assert_eq!(repeated["worker_signalled"], false);
    assert_eq!(repeated["run"]["status"], "completed");
}

#[test]
fn parallel_read_only_layer_observes_mcp_cancellation() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace("printf 'api tests passed\\n'", "sleep 30")
        .replace("printf 'web tests passed\\n'", "sleep 30");
    fs::write(&config_path, config).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let planned = call_tool(&ctx, tool::PLAN_RUN, json!({})).unwrap();
    assert_eq!(
        planned["plan"]["execution_layers"][0]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let accepted = call_tool(
        &ctx,
        tool::EXECUTE_RUN,
        json!({"plan": planned["plan"].clone()}),
    )
    .unwrap();
    let run_id = accepted["run_id"].as_str().unwrap();

    call_tool(&ctx, tool::CANCEL_RUN, json!({"run_id": run_id})).unwrap();
    let terminal = wait_for_repository_run(&ctx, run_id);

    assert_eq!(
        terminal["result"]["run"]["result"]["conclusion"],
        "cancelled"
    );
    assert!(
        terminal["result"]["run"]["result"]["targets"]
            .as_array()
            .unwrap()
            .iter()
            .all(|target| target["conclusion"] == "cancelled")
    );
}

#[test]
fn active_plan_linked_mcp_run_blocks_plan_close_until_terminal() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    fs::write(
        &config_path,
        config.replace("printf 'api tests passed\\n'", "sleep 30"),
    )
    .unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    crate::state::seed_open_plan_for_test(&ctx, "plan_active", "Active", "Body").unwrap();
    let planned = call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:test"]})).unwrap();
    let accepted = call_tool(
        &ctx,
        tool::EXECUTE_RUN,
        json!({
            "plan": planned["plan"].clone(),
            "work_plan_id": "plan_active"
        }),
    )
    .unwrap();
    let run_id = accepted["run_id"].as_str().unwrap();

    let error = crate::state::plans_close(
        &ctx,
        crate::state::PlanCloseRequest {
            plan_id: "plan_active".into(),
            resolution: Some("too early".into()),
        },
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("active linked repository runs"), "{error}");
    call_tool(&ctx, tool::CANCEL_RUN, json!({"run_id": run_id})).unwrap();
    let terminal = wait_for_repository_run(&ctx, run_id);
    assert_eq!(
        terminal["result"]["run"]["result"]["conclusion"],
        "cancelled"
    );
    // The terminal event is durable before the worker drops its execution
    // guards. Wait for that cleanup boundary so this assertion tests plan
    // lease release rather than racing the worker's final stack unwinding.
    crate::runtime::mcp_repository::wait_for_live_runs(&ctx);
    crate::state::plans_close(
        &ctx,
        crate::state::PlanCloseRequest {
            plan_id: "plan_active".into(),
            resolution: Some("done".into()),
        },
    )
    .unwrap();
}

#[test]
fn incompatible_mcp_run_is_rejected_without_blocking_cancellation() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    add_v6_effectful_evidence_actions(temp.path());
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "api_test_command = \"printf 'api tests passed\\n'\"",
        "api_test_command = \"if [ -f .agent/.cache/generator-active ]; then printf 'overlap\\n' > overlap.txt; fi; touch .agent/.cache/generator-active; sleep 30; printf 'generated\\n' >> api/generated.txt; rm -f .agent/.cache/generator-active\"",
    );
    fs::write(config_path, config).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let planned = call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:generate"]})).unwrap();
    let execute_args = json!({
        "plan": planned["plan"].clone(),
        "approved_effects": ["worktree"]
    });
    let first = call_tool(&ctx, tool::EXECUTE_RUN, execute_args.clone()).unwrap();
    let first_run_id = first["run_id"].as_str().unwrap();
    let marker = temp.path().join(".agent/.cache/generator-active");
    let marker_deadline = Instant::now() + Duration::from_secs(2);
    while !marker.exists() {
        assert!(
            Instant::now() < marker_deadline,
            "first worktree run did not start"
        );
        thread::sleep(Duration::from_millis(10));
    }

    let rejected_at = Instant::now();
    let second_error = call_tool(&ctx, tool::EXECUTE_RUN, execute_args)
        .unwrap_err()
        .to_string();
    assert_eq!(
        second_error,
        "repository execution is busy with an incompatible run; retry after it finishes or cancel that run first"
    );
    assert!(
        rejected_at.elapsed() < Duration::from_secs(2),
        "an incompatible execute request waited on the active run"
    );

    let cancelled = call_tool(&ctx, tool::CANCEL_RUN, json!({"run_id": first_run_id})).unwrap();
    assert_eq!(cancelled["worker_signalled"], true);
    let first_terminal = wait_for_repository_run(&ctx, first_run_id);
    assert_eq!(
        first_terminal["result"]["run"]["result"]["conclusion"],
        "cancelled"
    );
    assert!(!temp.path().join("overlap.txt").exists());
}

#[test]
fn mcp_repository_worker_observes_a_durable_external_cancel_request() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    fs::write(
        &config_path,
        config.replace(
            "printf 'api tests passed\\n'",
            "printf 'started\\n'; sleep 30; printf 'done\\n' > api-finished.txt",
        ),
    )
    .unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let planned = call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:test"]})).unwrap();
    let accepted = call_tool(
        &ctx,
        tool::EXECUTE_RUN,
        json!({"plan": planned["plan"].clone()}),
    )
    .unwrap();
    let run_id = accepted["run_id"].as_str().unwrap();

    crate::state::request_run_cancel(&ctx, run_id).unwrap();
    let terminal = wait_for_repository_run(&ctx, run_id);

    assert_eq!(
        terminal["result"]["run"]["result"]["conclusion"],
        "cancelled"
    );
    thread::sleep(Duration::from_millis(100));
    assert!(!temp.path().join("api-finished.txt").exists());
}

#[test]
fn mcp_native_schema_action_honors_timeout_and_cleans_its_process_tree() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    add_v6_native_schema_action(
        temp.path(),
        "sleep 30; printf finished > schema-finished.txt",
        1,
    );
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let planned = call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:schema"]})).unwrap();
    let accepted = call_tool(
        &ctx,
        tool::EXECUTE_RUN,
        json!({
            "plan": planned["plan"].clone(),
            "approved_effects": ["worktree"]
        }),
    )
    .unwrap();

    let terminal = wait_for_repository_run(&ctx, accepted["run_id"].as_str().unwrap());

    assert_eq!(
        terminal["result"]["run"]["result"]["conclusion"], "timed_out",
        "{terminal:#}"
    );
    thread::sleep(Duration::from_millis(100));
    assert!(!temp.path().join("schema-finished.txt").exists());
}

#[test]
fn mcp_native_schema_action_honors_cancellation() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    add_v6_native_schema_action(
        temp.path(),
        "sleep 30; printf finished > schema-finished.txt",
        30,
    );
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let planned = call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:schema"]})).unwrap();
    let accepted = call_tool(
        &ctx,
        tool::EXECUTE_RUN,
        json!({
            "plan": planned["plan"].clone(),
            "approved_effects": ["worktree"]
        }),
    )
    .unwrap();
    let run_id = accepted["run_id"].as_str().unwrap();

    let cancelled = call_tool(&ctx, tool::CANCEL_RUN, json!({"run_id": run_id})).unwrap();
    assert_eq!(cancelled["cancellation_requested"], true);
    let terminal = wait_for_repository_run(&ctx, run_id);

    assert_eq!(
        terminal["result"]["run"]["result"]["conclusion"], "cancelled",
        "{terminal:#}"
    );
    thread::sleep(Duration::from_millis(100));
    assert!(!temp.path().join("schema-finished.txt").exists());
}

#[test]
fn mcp_repository_rejects_unknown_arguments_stale_plans_and_v6_legacy_calls() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = call_tool(
        &ctx,
        tool::INSPECT,
        json!({"kind": "workspace", "unexpected": true}),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("Invalid arguments for jig.inspect"));

    let mut planned = call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:test"]})).unwrap();
    planned["plan"]["config_digest"] = json!("sha256:tampered");
    let error = call_tool(
        &ctx,
        tool::EXECUTE_RUN,
        json!({"plan": planned["plan"].clone()}),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("repository configuration changed"));
    assert!(!temp.path().join(".agent/state/runs.jsonl").exists());

    let error = call_tool(&ctx, tool::TEST, json!({}))
        .unwrap_err()
        .to_string();
    assert!(error.contains("Unsupported tool"));
}

#[test]
fn mcp_call_dispatches_command_tool_declared_only_in_manifest() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = call_tool(&ctx, "jig.custom_check", json!({})).unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["command_key"], "custom_check_command");
    assert_eq!(output["result"]["stdout"], "manifest target ran\n");
}

#[test]
fn mcp_legacy_execution_refreshes_command_authority_after_server_start() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap().replace(
        "custom_check_command = \"printf 'manifest target ran\\n'\"",
        "custom_check_command = \"printf 'refreshed legacy authority\\n'\"",
    );
    fs::write(config_path, config).unwrap();

    let output = call_tool(&ctx, "jig.custom_check", json!({})).unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(output["result"]["stdout"], "refreshed legacy authority\n");
}

#[test]
fn mcp_call_dispatches_command_tool_without_makefile() {
    let temp = tempdir().unwrap();
    write_command_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = call_tool(&ctx, "jig.custom_check", json!({})).unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["command_key"], "rust_test_command");
    assert_eq!(output["result"]["stdout"], "command tool ran\n");
    assert!(!temp.path().join("Makefile").exists());

    let receipts = fs::read_to_string(temp.path().join(".agent/state/receipts.jsonl")).unwrap();
    let receipt = receipts.lines().last().unwrap();
    assert!(receipt.contains(r#""invoked_command_key":"rust_test_command""#));
}

#[test]
fn mcp_native_migration_add_creates_files() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
sqlx_enabled = true
rust_migration_dir = "migrations"
"#,
        )
        .contract_version(2)
        .required_commands(["rust_test_command"])
        .tool(json!({
            "name": "jig.migration_add",
            "kind": "native",
            "description": "Add migration."
        }))
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = call_tool(&ctx, "jig.migration_add", json!({ "name": "create_users" })).unwrap();

    assert_eq!(output["ok"], true);
    assert!(
        output["result"]["stdout"]
            .as_str()
            .unwrap()
            .contains("create_users")
    );
    let entries = fs::read_dir(temp.path().join("migrations"))
        .unwrap()
        .count();
    assert_eq!(entries, 2);
}

#[test]
fn mcp_rejects_advertised_migration_add_for_versioned_artifacts_without_mutation() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
sqlx_enabled = true
rust_migration_dir = "schema"
rust_migration_layout = "versioned_artifacts"
"#,
        )
        .contract_version(2)
        .tool(json!({
            "name": "jig.migration_add",
            "kind": "native",
            "description": "Add migration."
        }))
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error =
        call_tool(&ctx, "jig.migration_add", json!({ "name": "create_users" })).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("configured Rust migration layout does not permit flat migration stubs")
    );
    assert!(!temp.path().join("schema").exists());
}

#[test]
fn mcp_rejects_command_backed_migration_add_for_versioned_artifacts_without_mutation() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
sqlx_enabled = true
rust_migration_dir = "schema"
rust_migration_layout = "versioned_artifacts"
migration_add_command = "mkdir -p schema && touch schema/should-not-exist.sql"
"#,
        )
        .contract_version(2)
        .required_commands(["migration_add_command"])
        .tool(json!({
            "name": "jig.migration_add",
            "kind": "command",
            "description": "Add migration.",
            "command": "migration_add_command"
        }))
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error =
        call_tool(&ctx, "jig.migration_add", json!({ "name": "create_users" })).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("configured Rust migration layout does not permit flat migration stubs")
    );
    assert!(!temp.path().join("schema").exists());
}

#[test]
fn mcp_native_contract_check_validates_manifest() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("scripts")).unwrap();
    fs::write(temp.path().join(".mcp.json"), "{}").unwrap();
    fs::write(temp.path().join("scripts/jig"), "#!/bin/sh\n").unwrap();
    fs::write(temp.path().join("scripts/install-jig.sh"), "#!/bin/sh\n").unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
bootstrap_command = "cargo fetch"
rust_fmt_check_command = "cargo fmt --check"
rust_clippy_command = "cargo clippy"
rust_test_command = "cargo test"
"#,
        )
        .contract_version(2)
        .required_commands([
            "bootstrap_command",
            "rust_fmt_check_command",
            "rust_clippy_command",
            "rust_test_command",
        ])
        .tool(json!({ "name": "jig.bootstrap", "kind": "command", "description": "Bootstrap.", "command": "bootstrap_command" }))
        .tool(json!({ "name": "jig.fmt_check", "kind": "command", "description": "Format.", "command": "rust_fmt_check_command" }))
        .tool(json!({ "name": "jig.clippy", "kind": "command", "description": "Clippy.", "command": "rust_clippy_command" }))
        .tool(json!({ "name": "jig.test", "kind": "command", "description": "Test.", "command": "rust_test_command" }))
        .tool(json!({ "name": "jig.contract_check", "kind": "native", "description": "Contract check." }))
        .write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = call_tool(&ctx, "jig.contract_check", json!({})).unwrap();

    assert_eq!(output["ok"], true);
    assert!(
        output["result"]["stdout"]
            .as_str()
            .unwrap()
            .contains("jig contract check passed")
    );
}

#[test]
fn mcp_native_schema_check_detects_clean_schema_dump() {
    let temp = tempdir().unwrap();
    fs::create_dir_all(temp.path().join("docs/schema")).unwrap();
    fs::write(temp.path().join("docs/schema/tables.sql"), "stable\n").unwrap();
    TestRepoBuilder::new(temp.path())
        .config(
            r#"
sqlx_enabled = true
schema_dump_enabled = true
rust_migration_dir = "migrations"
schema_dump_command = "mkdir -p docs/schema && printf 'stable\n' > docs/schema/tables.sql"
rust_test_command = "cargo test"
"#,
        )
        .contract_version(2)
        .required_commands(["rust_test_command", "schema_dump_command"])
        .tool(json!({
            "name": "jig.schema_check",
            "kind": "native",
            "description": "Schema check."
        }))
        .write();
    for args in [
        ["init", "-q"].as_slice(),
        ["config", "user.name", "Fixture"].as_slice(),
        ["config", "user.email", "fixture@example.com"].as_slice(),
        ["add", "."].as_slice(),
        ["commit", "-m", "fixture", "-q"].as_slice(),
    ] {
        let status = Command::new("git")
            .current_dir(temp.path())
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    }
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = call_tool(&ctx, "jig.schema_check", json!({})).unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["result"]["stdout"], "Schema dump is up to date.\n");
}

#[test]
fn mcp_exposes_read_only_agent_doctor_tool() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let codex_home = temp.path().join("codex-home");
    fs::create_dir_all(&codex_home).unwrap();
    fs::write(codex_home.join("config.toml"), "").unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        "#!/bin/sh\nif [ \"$1 $2 $3 $4\" = \"plugin marketplace add --help\" ]; then exit 0; fi\nexit 2\n",
    );

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let _codex_home = EnvVarGuard::set("CODEX_HOME", &codex_home);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = call_tool(&ctx, tool::AGENT_DOCTOR, json!({})).unwrap();

    assert_eq!(output["command"], "agent doctor");
    assert_eq!(output["codex"]["available"], true);
}

#[test]
fn mcp_does_not_expose_dev_or_proxy_commands() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    for name in [
        "jig.dev",
        "jig.proxy",
        "jig.proxy_start",
        "jig.proxy_cert_trust",
    ] {
        let error = call_tool(&ctx, name, json!({})).unwrap_err().to_string();
        assert!(error.contains("Unsupported tool"));
    }
}

#[test]
fn mcp_work_tools_deserialize_typed_arguments() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = call_tool(
        &ctx,
        tool::WORK_START,
        json!({
            "title": "Typed MCP request",
            "body": "Use serde for tool arguments"
        }),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert!(output["plan"]["plan_id"].as_str().is_some());
}

#[test]
fn mcp_work_append_rejects_blank_progress_without_mutating_plan() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_path = ctx.plan_body_path("plan_1");
    let body_before = fs::read_to_string(&plan_path).unwrap();
    let state_before = crate::state::state_summary(&ctx).unwrap();

    let error = call_tool(
        &ctx,
        tool::WORK_APPEND,
        json!({
            "plan_id": "plan_1",
            "body": " \n\t "
        }),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Progress text must not be empty"));
    assert_eq!(fs::read_to_string(plan_path).unwrap(), body_before);
    assert_eq!(crate::state::state_summary(&ctx).unwrap(), state_before);
}

#[test]
fn mcp_work_tools_tolerate_null_optional_defaults() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let check = call_tool(
        &ctx,
        tool::WORK_CHECK,
        json!({
            "plan_id": "plan_1",
            "tools": null
        }),
    )
    .unwrap();
    let receipts = call_tool(
        &ctx,
        tool::WORK_RECEIPTS,
        json!({
            "failed_only": null,
            "limit": null
        }),
    )
    .unwrap();
    let evidence = call_tool(
        &ctx,
        tool::WORK_EVIDENCE,
        json!({
            "plan_id": null
        }),
    )
    .unwrap();

    assert_eq!(check["ok"], true);
    assert_eq!(receipts["ok"], true);
    assert_eq!(evidence["command"], "work evidence");
    assert!(!receipts["receipts"].as_array().unwrap().is_empty());
}

#[test]
fn mcp_work_check_rejects_unknown_plan_before_running_tools() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = call_tool(
        &ctx,
        tool::WORK_CHECK,
        json!({
            "plan_id": "plan_missing",
            "tools": null
        }),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Plan not found: plan_missing"));
    let receipts_path = temp.path().join(".agent/state/receipts.jsonl");
    let receipts = fs::read_to_string(receipts_path).unwrap_or_default();
    assert!(!receipts.contains("jig.custom_check"));
}

#[test]
fn mcp_work_tools_reject_invalid_typed_arguments() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = call_tool(&ctx, tool::WORK_START, json!({ "body": "missing title" })).unwrap_err();
    let error = format!("{error:#}");

    assert!(error.contains("Invalid work tool arguments"));
    assert!(error.contains("missing field `title`"));
}

#[test]
fn mcp_inspect_reconciles_a_run_whose_worker_lease_disappeared() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let planned = call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:test"]})).unwrap();
    let plan: jig_contract::RunPlan = serde_json::from_value(planned["plan"].clone()).unwrap();
    let (abandoned, abandoned_lease) = crate::state::start_run(&ctx, plan, None).unwrap();
    drop(abandoned_lease);

    let inspected = call_tool(
        &ctx,
        tool::INSPECT,
        json!({"kind": "run", "run_id": abandoned.result.run_id}),
    )
    .unwrap();

    assert_eq!(inspected["result"]["run"]["result"]["status"], "completed");
    assert_eq!(
        inspected["result"]["run"]["result"]["conclusion"],
        "blocked"
    );
    assert!(
        inspected["result"]["run"]["result"]["targets"][0]["findings"][0]["message"]
            .as_str()
            .unwrap()
            .contains("worker lease is no longer held")
    );
}
