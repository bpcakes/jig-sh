use super::*;
use crate::command::{RepositoryRunRequest, RuntimeCommand, ToolRequest};

fn request(selectors: &[&str]) -> RepositoryRunRequest {
    RepositoryRunRequest {
        arguments: Default::default(),
        selectors: selectors.iter().map(|s| (*s).to_owned()).collect(),
        profile: None,
        affected_base: None,
        comparison: None,
        explain: false,
        fail_fast: false,
        approved_effects: Vec::new(),
        tool: ToolRequest::default(),
    }
}

#[test]
fn foreground_run_explain_matches_mcp_and_creates_no_execution_state() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    add_v6_generate_action(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let before = fs::read_dir(temp.path().join(".agent")).unwrap().count();
    let mcp = call_tool(&ctx, tool::PLAN_RUN, json!({"selectors": ["api:generate"]})).unwrap();
    let mut args = request(&["api:generate"]);
    args.explain = true;
    let output = crate::runtime::dispatch(&ctx, RuntimeCommand::Run(args)).unwrap();
    assert_eq!(output["executed"], false);
    assert_eq!(output["plan"], mcp["plan"]);
    assert!(!ctx.state_file("runs.jsonl").exists());
    assert!(!ctx.state_file("receipts.jsonl").exists());
    assert_eq!(
        fs::read_dir(temp.path().join(".agent")).unwrap().count(),
        before
    );
    assert!(!temp.path().join("generated.txt").exists());
}

#[test]
fn foreground_run_requires_exact_approval_and_executes_non_check_actions() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    add_v6_generate_action(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    for approvals in [
        vec![],
        vec![jig_contract::ActionEffect::External],
        vec![
            jig_contract::ActionEffect::Worktree,
            jig_contract::ActionEffect::External,
        ],
    ] {
        let mut args = request(&["api:generate"]);
        args.approved_effects = approvals;
        assert!(
            crate::runtime::dispatch(&ctx, RuntimeCommand::Run(args))
                .unwrap_err()
                .to_string()
                .contains("approved_effects")
        );
        assert!(!ctx.state_file("runs.jsonl").exists());
        assert!(!temp.path().join("generated.txt").exists());
    }
    let mut args = request(&["api:generate"]);
    args.approved_effects = vec![jig_contract::ActionEffect::Worktree];
    let output = crate::runtime::dispatch(&ctx, RuntimeCommand::Run(args)).unwrap();
    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(output["run"]["conclusion"], "success");
    assert_eq!(
        fs::read_to_string(temp.path().join("generated.txt")).unwrap(),
        "generated"
    );
}

#[test]
fn foreground_run_default_profile_matches_mcp_terminal_outcomes() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let planned = call_tool(&ctx, tool::PLAN_RUN, json!({"profile": "verify"})).unwrap();
    let args = request(&[]);
    let output = crate::runtime::dispatch(&ctx, RuntimeCommand::Run(args)).unwrap();
    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(output["plan"], planned["plan"]);
    let accepted = call_tool(&ctx, tool::EXECUTE_RUN, json!({"plan": planned["plan"]})).unwrap();
    let terminal = wait_for_repository_run(&ctx, accepted["run_id"].as_str().unwrap());
    let outcomes = |value: &Value| {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                (
                    t["target"].clone(),
                    t["conclusion"].clone(),
                    t["exit_code"].clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        outcomes(&output["run"]["targets"]),
        outcomes(&terminal["result"]["run"]["result"]["targets"])
    );
    crate::runtime::mcp_repository::wait_for_live_runs(&ctx);
}

#[test]
fn foreground_run_fail_fast_accounts_for_unstarted_targets() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let path = temp.path().join(".jig.toml");
    fs::write(
        &path,
        fs::read_to_string(&path)
            .unwrap()
            .replace("printf 'api tests passed\\n'", "exit 7"),
    )
    .unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut args = request(&[]);
    args.profile = Some("verify".into());
    args.fail_fast = true;
    let output = crate::runtime::dispatch(&ctx, RuntimeCommand::Run(args)).unwrap();
    assert_eq!(output["ok"], false);
    assert_eq!(output["run"]["targets"][0]["conclusion"], "failure");
    assert_eq!(output["run"]["targets"][0]["exit_code"], 7);
    assert_eq!(output["run"]["targets"][1]["conclusion"], "skipped");
}

struct CancelOnOutput {
    cancelled: bool,
    durable_ready: Option<std::sync::mpsc::Sender<()>>,
    output: Vec<u8>,
}
impl crate::execution::ExecutionObserver for CancelOnOutput {
    fn event(&mut self, event: crate::execution::ExecutionEvent<'_>) {
        if self.cancelled {
            return;
        }
        let crate::execution::ExecutionEvent::Output { bytes, .. } = event else {
            return;
        };
        self.output.extend_from_slice(bytes);
        if !self.output.windows(7).any(|part| part == b"started") {
            return;
        }
        if let Some(ready) = &self.durable_ready {
            ready.send(()).unwrap();
        }
        self.cancelled = true;
    }
}
impl crate::execution::ExecutionCancellation for CancelOnOutput {
    fn cancelled(&self) -> bool {
        self.cancelled && self.durable_ready.is_none()
    }
}

#[test]
fn foreground_run_cancellation_stops_running_and_unstarted_targets() {
    for durable in [false, true] {
        let temp = tempdir().unwrap();
        write_v6_evidence_fixture_repo(temp.path(), "");
        let path = temp.path().join(".jig.toml");
        fs::write(
            &path,
            fs::read_to_string(&path).unwrap().replace(
                "printf 'api tests passed\\n'",
                "printf 'started\\n'; sleep 30; printf finished",
            ),
        )
        .unwrap();
        init_git_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let mut args = request(&[]);
        args.profile = Some("verify".into());
        args.fail_fast = true;
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let cancellation = durable.then(|| {
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                ready_rx
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .unwrap();
                let journal = fs::read_to_string(ctx.state_file("runs.jsonl")).unwrap();
                let started: Value = journal
                    .lines()
                    .rev()
                    .map(|line| serde_json::from_str::<Value>(line).unwrap())
                    .find(|event| event["event"] == "target_started")
                    .unwrap();
                let run_id = started["run_id"].as_str().unwrap().to_owned();
                let response =
                    call_tool(&ctx, tool::CANCEL_RUN, json!({"run_id": run_id})).unwrap();
                assert_eq!(response["cancellation_requested"], true);
                assert_eq!(response["worker_signalled"], false);
                run_id
            })
        });
        let mut observer = CancelOnOutput {
            cancelled: false,
            durable_ready: durable.then_some(ready_tx),
            output: Vec::new(),
        };
        let output =
            dispatch_with_observer(&ctx, RuntimeCommand::Run(args), &mut observer).unwrap();
        if let Some(cancellation) = cancellation {
            assert_eq!(output["run"]["run_id"], cancellation.join().unwrap());
        }
        assert!(
            observer.cancelled,
            "the child must start before cancellation"
        );
        assert_eq!(output["ok"], false, "{output:#}");
        assert_eq!(output["run"]["conclusion"], "cancelled");
        assert_eq!(output["run"]["targets"][0]["conclusion"], "cancelled");
        assert_eq!(output["run"]["targets"][1]["conclusion"], "cancelled");
        assert_eq!(output["run"]["targets"][1]["started_at_ms"], Value::Null);
        // Reacquisition proves foreground execution released its ownership after cleanup.
        let lease = crate::state::acquire_repository_execution_lease_without_wait(
            &ctx,
            &[jig_contract::ActionEffect::Worktree],
        )
        .unwrap();
        drop(lease);
    }
}

#[test]
fn foreground_run_affected_explain_and_execution_match_mcp() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    init_git_repo(temp.path());
    fs::write(temp.path().join("api/example.go"), "package changed\n").unwrap();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mcp = call_tool(
        &ctx,
        tool::PLAN_RUN,
        json!({"profile": "verify", "affected_base": "HEAD"}),
    )
    .unwrap();
    let mut args = request(&[]);
    args.profile = Some("verify".into());
    args.affected_base = Some("HEAD".into());
    args.explain = true;
    let output = crate::runtime::dispatch(&ctx, RuntimeCommand::Run(args.clone())).unwrap();
    assert_eq!(output["plan"], mcp["plan"]);
    assert_eq!(output["plan"]["targets"].as_array().unwrap().len(), 1);
    assert_eq!(output["plan"]["targets"][0]["target"]["component"], "api");
    args.explain = false;
    let executed = crate::runtime::dispatch(&ctx, RuntimeCommand::Run(args)).unwrap();
    assert_eq!(executed["ok"], true, "{executed:#}");
    assert_eq!(executed["plan"], output["plan"]);
    assert_eq!(executed["run"]["targets"].as_array().unwrap().len(), 1);
    assert_eq!(executed["run"]["targets"][0]["target"]["component"], "api");
}

#[test]
fn foreground_run_and_check_preserve_work_plan_and_receipt_identity() {
    for (native, check) in [(false, false), (true, false), (false, true), (true, true)] {
        let temp = tempdir().unwrap();
        write_non_rust_file_budget_fixture_repo(temp.path());
        if native {
            let config_path = temp.path().join(".jig.toml");
            let config = fs::read_to_string(&config_path).unwrap().replace(
                "runner = { kind = \"command\", command = \"web_file_loc_command\" }",
                "runner = { kind = \"native\", operation = \"jig.file_budget\" }",
            );
            fs::write(
                config_path,
                config.replace("inputs = [\"web/**\"]", "inputs = [\"**\"]"),
            )
            .unwrap();
            let manifest_path = temp.path().join(".agent/jig-contract.json");
            let mut manifest: Value =
                serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
            manifest["contract_version"] = json!(7);
            manifest["actions"][0]["inputs"] = json!(["**"]);
            manifest["actions"][0]["runner"] =
                json!({"kind": "native", "operation": "jig.file_budget"});
            fs::write(
                manifest_path,
                serde_json::to_string_pretty(&manifest).unwrap(),
            )
            .unwrap();
            fs::create_dir_all(temp.path().join(".jig")).unwrap();
            fs::write(
                temp.path().join(".jig/file-budget.toml"),
                "version=1\n[[rules]]\nid=\"web\"\ninclude=[\"web/**\"]\nmax_lines=100\n",
            )
            .unwrap();
        }
        init_git_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let started = crate::runtime::dispatch(
            &ctx,
            RuntimeCommand::Work(crate::command::WorkCommand::Start(
                crate::command::WorkStartRequest {
                    title: "Native run fixture".into(),
                    body: Some("Validate native work identity".into()),
                    body_file: None,
                    base: None,
                },
            )),
        )
        .unwrap();
        let id = started["plan"]["plan_id"].as_str().unwrap().to_owned();
        let mut args = request(&["web:file-loc"]);
        if native {
            args.comparison = Some(jig_contract::ComparisonRequestV1::StrictInventory {
                reason: jig_contract::StrictInventoryReasonV1::ExplicitCheck,
            });
        }
        args.tool = ToolRequest::new(Some(id.clone()), true);
        let command = if check {
            RuntimeCommand::Check(crate::command::CheckCommand::Repository(
                crate::command::RepositoryCheckRequest {
                    selectors: args.selectors,
                    profile: args.profile,
                    affected_base: args.affected_base,
                    comparison: args.comparison,
                    explain: args.explain,
                    fail_fast: args.fail_fast,
                    tool: args.tool,
                },
            ))
        } else {
            RuntimeCommand::Run(args)
        };
        let output = crate::runtime::dispatch(&ctx, command).unwrap();
        assert_eq!(output["ok"], true, "{output:#}");
        if native {
            assert_eq!(
                output["plan"]["targets"][0]["prepared_native_input"]["work_plan_id"],
                id
            );
        }
        let durable =
            crate::state::run_by_id(&ctx, output["run"]["run_id"].as_str().unwrap()).unwrap();
        assert_eq!(durable.work_plan_id.as_deref(), Some(id.as_str()));
        let receipt = output["run"]["targets"][0]["receipt_id"].as_str().unwrap();
        let receipts = fs::read_to_string(ctx.state_file("receipts.jsonl")).unwrap();
        let receipt: Value = receipts
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .find(|r| r["id"] == receipt)
            .unwrap();
        assert_eq!(receipt["plan_id"], id);
    }
}

#[test]
fn foreground_run_rejects_legacy_contract_with_migration_guidance() {
    let temp = tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    assert!(ctx.contract_version() < 6);
    let error = crate::runtime::dispatch(&ctx, RuntimeCommand::Run(request(&["test"])))
        .unwrap_err()
        .to_string();
    assert!(error.contains("contract version 6"), "{error}");
    assert!(error.contains("jig update"), "{error}");
    assert!(!ctx.state_file("runs.jsonl").exists());
}

#[test]
fn foreground_run_rejects_comparison_authority_on_v6_before_state_changes() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    for explain in [false, true] {
        let mut args = request(&["api:test"]);
        args.explain = explain;
        args.comparison = Some(jig_contract::ComparisonRequestV1::StrictInventory {
            reason: jig_contract::StrictInventoryReasonV1::ExplicitCheck,
        });
        let error = crate::runtime::dispatch(&ctx, RuntimeCommand::Run(args))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("comparison authority requires repository contract version 7"),
            "{error}"
        );
        assert!(!ctx.state_file("runs.jsonl").exists());
        assert!(!ctx.state_file("receipts.jsonl").exists());
    }
}

struct AlreadyCancelled;
impl crate::execution::ExecutionObserver for AlreadyCancelled {}
impl crate::execution::ExecutionCancellation for AlreadyCancelled {
    fn cancelled(&self) -> bool {
        true
    }
}

#[test]
fn foreground_prestart_cancellation_keeps_existing_check_run_evidence() {
    for run in [false, true] {
        let temp = tempdir().unwrap();
        write_v6_evidence_fixture_repo(temp.path(), "");
        init_git_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        let catalog = crate::repository::RepositoryCatalog::from_context(&ctx).unwrap();
        let plan = crate::repository::plan_action_run(
            &ctx,
            &catalog,
            crate::repository::PlanRunRequest {
                selectors: vec!["api:test".into()],
                ..Default::default()
            },
            Default::default(),
        )
        .unwrap();
        let execute = if run {
            crate::runtime::run_execution::execute_foreground_action_run
        } else {
            crate::runtime::run_execution::execute_freshly_planned_check_run
        };
        let result = execute(
            &ctx,
            &catalog,
            plan,
            crate::runtime::run_execution::ExecuteCheckRunRequest {
                work_plan_id: None,
                record_receipts: true,
                fail_fast: false,
            },
            &mut AlreadyCancelled,
        );
        if run {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("cancelled before the run started")
            );
            assert!(!ctx.state_file("runs.jsonl").exists());
        } else {
            let output = serde_json::to_value(result.unwrap()).unwrap();
            assert_eq!(output["run"]["result"]["conclusion"], "cancelled");
            assert_eq!(
                output["run"]["result"]["targets"][0]["conclusion"],
                "cancelled"
            );
            assert!(ctx.state_file("runs.jsonl").exists());
        }
    }
}

#[test]
fn foreground_explain_defers_work_plan_openness_to_execution() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let mut args = request(&["api:test"]);
    args.tool = ToolRequest::new(Some("plan_missing".into()), true);
    args.explain = true;
    let explained = crate::runtime::dispatch(&ctx, RuntimeCommand::Run(args.clone())).unwrap();
    assert_eq!(explained["command"], "run plan");
    assert_eq!(explained["executed"], false);
    args.explain = false;
    let error = crate::runtime::dispatch(&ctx, RuntimeCommand::Run(args)).unwrap_err();
    assert!(error.to_string().contains("plan_missing"), "{error:#}");
    assert!(!ctx.state_file("runs.jsonl").exists());
}
