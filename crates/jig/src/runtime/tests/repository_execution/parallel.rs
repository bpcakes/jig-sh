use super::*;

#[test]
fn independent_read_only_layer_targets_execute_concurrently() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace(
            "api_test_command = \"printf 'api tests passed\\n'\"",
            "api_test_command = \"touch .agent/.cache/api-started; for attempt in $(seq 1 200); do [ -f .agent/.cache/web-started ] && { touch .agent/.cache/api-finished; exit 0; }; sleep 0.01; done; exit 9\"",
        )
        .replace(
            "web_test_command = \"printf 'web tests passed\\n'\"",
            "web_test_command = \"touch .agent/.cache/web-started; for attempt in $(seq 1 200); do [ -f .agent/.cache/api-finished ] && { sleep 1; exit 0; }; sleep 0.01; done; exit 9\"",
        );
    fs::write(config_path, config).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = jig_repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = jig_repository::plan_run(&ctx, &catalog, jig_repository::PlanRunRequest::default())
        .unwrap();
    assert_eq!(plan.execution_layers.len(), 1);
    assert_eq!(plan.execution_layers[0].len(), 2);
    let mut observer = PhaseRecordingObserver::default();

    let execution = crate::runtime::run_execution::execute_freshly_planned_check_run(
        &ctx,
        &catalog,
        plan,
        crate::runtime::run_execution::ExecuteCheckRunRequest {
            alias_override: None,
            fail_fast: false,
        },
        &mut observer,
    )
    .unwrap();

    assert_eq!(
        execution.run.result.conclusion,
        Some(jig_contract::RunConclusion::Success)
    );
    assert_eq!(
        execution
            .run
            .result
            .targets
            .iter()
            .map(|target| target.target.to_string())
            .collect::<Vec<_>>(),
        ["api:test", "web:test"]
    );
    assert_eq!(observer.started.len(), 2);
    assert_eq!(observer.finished.len(), 2);
    let api = &execution.run.result.targets[0];
    let web = &execution.run.result.targets[1];
    assert!(
        api.ended_at_ms.unwrap().saturating_add(500) <= web.ended_at_ms.unwrap(),
        "each parallel target must retain its own completion time: api={api:?}, web={web:?}"
    );
}

#[test]
fn wide_parallel_layer_keeps_the_bounded_worker_pool_busy() {
    let temp = tempdir().unwrap();
    let mut commands = vec!["sleep 0.05".to_owned(); 9];
    commands[0] = "for attempt in $(seq 1 300); do [ -f .agent/.cache/ninth-started ] && exit 0; sleep 0.01; done; exit 9".into();
    commands[8] = "touch .agent/.cache/ninth-started".into();
    write_wide_v6_evidence_fixture_repo(temp.path(), &commands);
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = crate::runtime::dispatch(
        &ctx,
        RuntimeCommand::Check(crate::command::CheckCommand::Repository(
            crate::command::RepositoryCheckRequest {
                selectors: Vec::new(),
                profile: None,
                affected_base: None,
                comparison: None,
                explain: false,
                fail_fast: false,
            },
        )),
    )
    .unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(
        output["source_observations"]["count"], 3,
        "the queued target requires a fresh source precondition"
    );
}

#[test]
fn queued_parallel_target_revalidates_source_before_starting() {
    let temp = tempdir().unwrap();
    let mut commands = (0..9)
        .map(|_| {
            "for attempt in $(seq 1 200); do [ -f .agent/.cache/source-mutated ] && { sleep 0.5; exit 0; }; sleep 0.01; done; exit 9"
                .to_owned()
        })
        .collect::<Vec<_>>();
    commands[0] =
        "printf 'mutated\n' >> example0/example.txt; touch .agent/.cache/source-mutated".into();
    commands[8] = "touch .agent/.cache/queued-target-ran".into();
    write_wide_v6_evidence_fixture_repo(temp.path(), &commands);
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = jig_repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = jig_repository::plan_run(&ctx, &catalog, jig_repository::PlanRunRequest::default())
        .unwrap();
    let mut observer = PhaseRecordingObserver::default();

    let execution = crate::runtime::run_execution::execute_freshly_planned_check_run(
        &ctx,
        &catalog,
        plan,
        crate::runtime::run_execution::ExecuteCheckRunRequest {
            alias_override: None,
            fail_fast: false,
        },
        &mut observer,
    )
    .unwrap();

    let queued = &execution.run.result.targets[8];
    assert_eq!(queued.started_at_ms, None, "{queued:?}");
    assert!(
        queued.findings.iter().any(|finding| finding
            .message
            .contains("worktree changed after plan validation")),
        "queued work must preserve its failed source precondition: {queued:?}"
    );
    assert!(
        !temp.path().join(".agent/.cache/queued-target-ran").exists(),
        "a target claimed after stable source drift must remain unstarted"
    );
}

#[test]
fn parallel_read_only_layer_fails_closed_and_reports_failure_on_a_source_mutation() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace(
            "api_test_command = \"printf 'api tests passed\\n'\"",
            "api_test_command = \"touch .agent/.cache/api-started; for attempt in $(seq 1 200); do [ -f .agent/.cache/web-started ] && { printf 'mutated\\n' >> api/example.go; exit 0; }; sleep 0.01; done; exit 9\"",
        )
        .replace(
            "web_test_command = \"printf 'web tests passed\\n'\"",
            "web_test_command = \"touch .agent/.cache/web-started; for attempt in $(seq 1 200); do [ -f .agent/.cache/api-started ] && { sleep 0.1; exit 0; }; sleep 0.01; done; exit 9\"",
        );
    fs::write(config_path, config).unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = jig_repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = jig_repository::plan_run(&ctx, &catalog, jig_repository::PlanRunRequest::default())
        .unwrap();
    let mut observer = PhaseRecordingObserver::default();

    let execution = crate::runtime::run_execution::execute_freshly_planned_check_run(
        &ctx,
        &catalog,
        plan,
        crate::runtime::run_execution::ExecuteCheckRunRequest {
            alias_override: None,
            fail_fast: false,
        },
        &mut observer,
    )
    .unwrap();

    assert_eq!(
        execution.run.result.conclusion,
        Some(jig_contract::RunConclusion::Failure)
    );
    assert_eq!(
        serde_json::to_value(execution.source_observations).unwrap()["count"],
        2
    );
    assert!(
        execution
            .run
            .result
            .targets
            .iter()
            .all(|target| target.conclusion == Some(jig_contract::RunConclusion::Failure))
    );
    assert!(
        observer.finished.iter().all(|(_, success)| !success),
        "phase completion must reflect the postcondition-adjusted target result: {:?}",
        observer.finished
    );
    for target in &execution.run.result.targets {
        let effect_policy = target
            .findings
            .iter()
            .find(|finding| finding.source.as_deref() == Some("effect_policy"))
            .expect("each started parallel target must record the shared layer violation");
        assert!(
            effect_policy.message.contains("parallel read-only layer"),
            "shared observations must describe the layer rather than blame one target: {effect_policy:?}"
        );
        assert!(
            !effect_policy.message.contains("while target"),
            "shared observations cannot identify which concurrent target changed the source: {effect_policy:?}"
        );
    }
}

#[test]
fn cancelled_parallel_target_keeps_not_started_evidence_after_a_sibling_mutation() {
    let temp = tempdir().unwrap();
    let mut commands = vec!["sleep 2".to_owned(); 9];
    commands[0] =
        "printf 'mutated\\n' >> example0/example.txt; touch .agent/.cache/cancel; sleep 2".into();
    commands[8] = "exit 9".into();
    write_wide_v6_evidence_fixture_repo(temp.path(), &commands);
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = jig_repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = jig_repository::plan_run(&ctx, &catalog, jig_repository::PlanRunRequest::default())
        .unwrap();
    let ninth_planned_digest = plan.targets[8].input_digest.clone();
    let mut observer = MarkerCancellationObserver {
        marker: temp.path().join(".agent/.cache/cancel"),
    };

    let execution = crate::runtime::run_execution::execute_freshly_planned_check_run(
        &ctx,
        &catalog,
        plan,
        crate::runtime::run_execution::ExecuteCheckRunRequest {
            alias_override: None,
            fail_fast: false,
        },
        &mut observer,
    )
    .unwrap();

    let ninth = &execution.run.result.targets[8];
    assert_eq!(ninth.started_at_ms, None, "{ninth:?}");
    assert_eq!(ninth.input_digest, ninth_planned_digest);
    assert!(
        ninth
            .findings
            .iter()
            .all(|finding| finding.source.as_deref() != Some("effect_policy")),
        "a target that never started must not be blamed for a sibling mutation: {ninth:?}"
    );
}

#[test]
fn parallel_target_that_fails_authority_before_start_keeps_specific_evidence() {
    let temp = tempdir().unwrap();
    let mut commands = (0..8)
        .map(|index| {
            format!(
                "touch .agent/.cache/parallel-started-{index}; for attempt in $(seq 1 200); do grep -q 'invalid contract' .agent/jig-contract.json && exit 0; sleep 0.01; done; exit 9"
            )
        })
        .chain(std::iter::once("exit 0".to_owned()))
        .collect::<Vec<_>>();
    commands[0] = "touch .agent/.cache/parallel-started-0; for attempt in $(seq 1 200); do [ \"$(find .agent/.cache -name 'parallel-started-*' | wc -l)\" -eq 8 ] && { printf 'invalid contract\n' > .agent/jig-contract.json; exit 0; }; sleep 0.01; done; exit 9".into();
    write_wide_v6_evidence_fixture_repo(temp.path(), &commands);
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = jig_repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = jig_repository::plan_run(&ctx, &catalog, jig_repository::PlanRunRequest::default())
        .unwrap();
    let mut observer = PhaseRecordingObserver::default();

    let execution = crate::runtime::run_execution::execute_freshly_planned_check_run(
        &ctx,
        &catalog,
        plan,
        crate::runtime::run_execution::ExecuteCheckRunRequest {
            alias_override: None,
            fail_fast: false,
        },
        &mut observer,
    )
    .unwrap();

    let ninth = &execution.run.result.targets[8];
    assert_eq!(ninth.started_at_ms, None, "{ninth:?}");
    assert_eq!(
        ninth.conclusion,
        Some(jig_contract::RunConclusion::Blocked),
        "{ninth:?}"
    );
    assert!(
        ninth
            .output_tail
            .as_ref()
            .unwrap()
            .stderr
            .contains("authority could not be verified"),
        "a pre-start authority failure must remain specific in durable evidence: {ninth:?}"
    );
}

#[test]
fn parallel_layer_uses_the_baseline_adopted_by_a_mutating_predecessor() {
    let temp = tempdir().unwrap();
    write_v6_evidence_fixture_repo(temp.path(), "");
    add_v6_effectful_evidence_actions(temp.path());
    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path)
        .unwrap()
        .replace(
            "[commands]",
            "[commands]\napi_generate_command = \"printf 'generated\\n' > api/generated.go\"",
        )
        .replace(
            "target = { component = \"api\", action = \"generate\" }\nintent = \"generate\"\neffects = [\"worktree\", \"process\"]\nrunner = { kind = \"command\", command = \"api_test_command\" }",
            "target = { component = \"api\", action = \"generate\" }\nintent = \"generate\"\neffects = [\"worktree\", \"process\"]\nrunner = { kind = \"command\", command = \"api_generate_command\" }",
        )
        .replace(
            "[[repository.profiles]]",
            r#"[[repository.actions]]
target = { component = "web", action = "verify-generated" }
intent = "check"
effects = ["read_only", "process"]
runner = { kind = "command", command = "web_test_command" }
inputs = ["web/**"]
depends_on = [{ component = "api", action = "generate" }]

[[repository.profiles]]"#,
        );
    fs::write(&config_path, config).unwrap();
    let manifest_path = temp.path().join(".agent/jig-contract.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["required_commands"]
        .as_array_mut()
        .unwrap()
        .push(json!("api_generate_command"));
    let actions = manifest["actions"].as_array_mut().unwrap();
    actions
        .iter_mut()
        .find(|action| action["target"]["action"] == "generate")
        .unwrap()["runner"]["command"] = json!("api_generate_command");
    actions.push(json!({
        "target": {"component": "web", "action": "verify-generated"},
        "intent": "check",
        "effects": ["read_only", "process"],
        "runner": {"kind": "command", "command": "web_test_command"},
        "inputs": ["web/**"],
        "depends_on": [{"component": "api", "action": "generate"}]
    }));
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = jig_repository::RepositoryCatalog::from_context(&ctx).unwrap();
    let plan = jig_repository::plan_action_run(
        &ctx,
        &catalog,
        jig_repository::PlanRunRequest {
            selectors: vec!["api:verify-generated".into(), "web:verify-generated".into()],
            profile: None,
            affected_base: None,
            comparison: None,
        },
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        plan.execution_layers
            .iter()
            .map(Vec::len)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    let mut observer = PhaseRecordingObserver::default();

    let execution = crate::runtime::run_execution::execute_freshly_planned_check_run(
        &ctx,
        &catalog,
        plan,
        crate::runtime::run_execution::ExecuteCheckRunRequest {
            alias_override: None,
            fail_fast: false,
        },
        &mut observer,
    )
    .unwrap();

    assert_eq!(
        execution.run.result.conclusion,
        Some(jig_contract::RunConclusion::Success),
        "{:?}",
        execution.run.result.targets
    );
    assert_eq!(
        serde_json::to_value(execution.source_observations).unwrap()["count"],
        4
    );
    assert!(temp.path().join("api/generated.go").exists());
}
