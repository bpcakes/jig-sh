use std::collections::BTreeMap;
use std::io::Write;
use std::time::Duration;

use jig_contract::{PlannedTarget, TargetId};

use super::*;
use crate::repository::freshness::{CollectionBudget, CollectionLimits, collect_target_identities};

mod native;
mod recovery;
mod worktree;

fn fixture(root: &Path, dependency: bool, profile: bool) -> RepoContext {
    let selector = if profile {
        "profile = \"verify\""
    } else {
        "target = \"web:test\""
    };
    write_v6_evidence_fixture_repo(
        root,
        &format!("[[work.gates]]\nid = \"verify\"\nkind = \"evidence\"\n{selector}\n"),
    );
    let config_path = root.join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    let manifest_path = root.join(".agent/jig-contract.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["contract_version"] = json!(9);
    for action in config["repository"]["actions"].as_array_mut().unwrap() {
        action.as_table_mut().unwrap().insert(
            "inputs_policy".into(),
            toml::Value::String("exhaustive".into()),
        );
        action["runner"]["kind"] = toml::Value::String("shell".into());
    }
    for action in manifest["actions"].as_array_mut().unwrap() {
        action["inputs_policy"] = json!("exhaustive");
        action["runner"]["kind"] = json!("shell");
    }
    if dependency {
        let target = "api:test".parse::<TargetId>().unwrap();
        manifest["actions"][1]["depends_on"] = json!([target]);
        config["repository"]["actions"][1]
            .as_table_mut()
            .unwrap()
            .insert(
                "depends_on".into(),
                toml::Value::try_from(vec![target]).unwrap(),
            );
    }
    fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
    fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    init_git_repo(root);
    RepoContext::load_from_root(root.to_path_buf()).unwrap()
}

fn original_records(ctx: &RepoContext) -> BTreeMap<TargetId, Value> {
    let catalog = crate::repository::RepositoryCatalog::from_context(ctx).unwrap();
    let invocations = catalog
        .actions()
        .map(|action| {
            let mut planned = PlannedTarget::new(
                action.target.clone(),
                action.intent,
                action.runner.clone(),
                "",
            );
            planned.effects = action.effects.clone();
            planned.inputs = action.inputs.clone();
            planned.depends_on = action.depends_on.clone();
            planned.timeout_seconds = action.timeout_seconds;
            planned.result_parser = action.result_parser;
            planned
        })
        .collect::<Vec<_>>();
    let source = crate::state::current_worktree_fingerprint(ctx)
        .fingerprint
        .unwrap();
    let mut budget = CollectionBudget::new(
        CollectionLimits::with_timeout(Duration::from_secs(2)),
        &|| false,
    );
    let identities = collect_target_identities(ctx, &catalog, &invocations, &source, &mut budget)
        .unwrap()
        .targets;
    let mut records = BTreeMap::new();
    for (index, planned) in invocations.iter().enumerate() {
        let identity = identities[&planned.target].as_ref().unwrap();
        let dependencies = identity.dependencies.iter().map(|dependency| {
            let original: &Value = &records[&dependency.target];
            json!({"target": dependency.target, "receipt_id": original["id"], "run_id": original["run_id"],
                "plan_id": "plan_1", "identity_digest": dependency.identity_digest, "conclusion": "success",
                "effective_valid_until_ms": null, "effective_requires_time_validity": false})
        }).collect::<Vec<_>>();
        records.insert(planned.target.clone(), json!({
            "id": format!("receipt_original_{index}"), "run_id": format!("run_original_{index}"),
            "plan_id": "plan_1", "target": planned.target, "tool_name": "jig.target_run", "args": {},
            "started_at_ms": index * 10 + 10, "ended_at_ms": index * 10 + 15,
            "exit_status": 0, "stdout_preview": "", "stderr_preview": "", "changed_paths": [],
            "diff_stat": {"files": 0, "insertions": 0, "deletions": 0},
            "worktree_fingerprint": source, "config_digest": catalog.config_digest(), "input_digest": "legacy-input",
            "target_freshness": {
                "schema_version": 1, "contract_epoch": 9, "state": "complete", "identity": identity,
                "dependency_execution_proof": dependencies, "effective_valid_until_ms": null,
                "effective_requires_time_validity": false,
                "global_execution_proof": {"state": "unchanged", "before_source_digest": source, "after_source_digest": source},
            },
        }));
    }
    records
}

fn append(ctx: &RepoContext, records: impl IntoIterator<Item = Value>) {
    let path = ctx.root().join(".agent/state/receipts.jsonl");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    for record in records {
        writeln!(&mut file, "{record}").unwrap();
    }
}

#[test]
fn historical_conflicts_do_not_poison_unrelated_gates_but_required_conflicts_block() {
    for dependency in [false, true] {
        let temp = tempdir().unwrap();
        let ctx = fixture(temp.path(), dependency, false);
        let originals = original_records(&ctx);
        let historical = json!({
            "id": "receipt_example_historical", "tool_name": "jig.example", "args": {},
            "started_at_ms": 1, "ended_at_ms": 2, "exit_status": 0,
            "stdout_preview": "Example first preview", "stderr_preview": "", "changed_paths": [],
            "diff_stat": {"files": 0, "insertions": 0, "deletions": 0},
        });
        let mut different = historical.clone();
        different["stdout_preview"] = json!("Example second preview");
        append(&ctx, [historical, different]);
        append(&ctx, originals.values().cloned());
        let usable = work_gates(&ctx);
        assert_eq!(usable["gates"][0]["status"], "passed", "{usable:#}");
        assert_eq!(
            usable["gates"][0]["targets"][0]["receipt_id"],
            originals[&"web:test".parse().unwrap()]["id"]
        );

        let target = if dependency { "api:test" } else { "web:test" };
        let original = originals[&target.parse().unwrap()].clone();
        let mut conflicting = original.clone();
        conflicting["exit_status"] = json!(1);
        // A later repetition of the first envelope cannot repair an ambiguous ID.
        append(&ctx, [conflicting, original]);
        let blocked = work_gates(&ctx);
        assert_eq!(blocked["gates"][0]["status"], "unknown", "{blocked:#}");
        assert!(
            blocked["gates"][0]["freshness_reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reason| reason["code"] == "collection_failed"),
            "{blocked:#}"
        );
    }
}

#[test]
fn epoch_nine_gate_compares_scoped_inputs_and_keeps_only_explicit_required_receipts() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), true, false);
    let originals = original_records(&ctx);
    append(&ctx, originals.values().cloned());
    let first = work_gates(&ctx);
    assert_eq!(first["gates"][0]["status"], "passed", "{first:#}");
    assert_eq!(first["gates"][0]["targets"].as_array().unwrap().len(), 1);
    let mut failed_dependency = originals[&"api:test".parse().unwrap()].clone();
    failed_dependency["id"] = json!("receipt_newer_failed_dependency");
    failed_dependency["ended_at_ms"] = json!(100);
    failed_dependency["exit_status"] = json!(1);
    append(&ctx, [failed_dependency]);
    fs::write(temp.path().join("unrelated.md"), "Example unrelated edit\n").unwrap();
    let unchanged = work_gates(&ctx);
    assert_eq!(unchanged["gates"][0]["status"], "passed", "{unchanged:#}");
    assert_eq!(
        unchanged["gates"][0]["targets"][0]["receipt_id"],
        originals[&"web:test".parse().unwrap()]["id"]
    );
    fs::write(temp.path().join("api/example.go"), "package changed\n").unwrap();
    let changed = work_gates(&ctx);
    assert_eq!(changed["gates"][0]["status"], "stale", "{changed:#}");
    assert!(
        changed["gates"][0]["freshness_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason["code"] == "dependency_changed")
    );
}

#[test]
fn epoch_nine_failed_outcome_and_unsupported_freshness_have_independent_precedence() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), false, true);
    let mut originals = original_records(&ctx);
    originals.get_mut(&"api:test".parse().unwrap()).unwrap()["exit_status"] = json!(1);
    originals.get_mut(&"web:test".parse().unwrap()).unwrap()["target_freshness"] =
        json!({"schema_version": 2});
    append(&ctx, originals.into_values());
    let report = work_gates(&ctx);
    let gate = &report["gates"][0];
    assert_eq!(gate["status"], "failed", "{report:#}");
    assert_eq!(gate["freshness"], "unsupported");
    assert_eq!(gate["run_id"], Value::Null);
    let typed: jig_ui::dashboard::StatusEvidenceGate =
        serde_json::from_value(gate.clone()).unwrap();
    let round_trip = serde_json::to_value(typed).unwrap();
    assert_eq!(round_trip["freshness_reasons"], gate["freshness_reasons"]);
    assert_eq!(
        round_trip["targets"][0]["recorded_identity"],
        gate["targets"][0]["recorded_identity"]
    );
    assert_eq!(round_trip["freshness_collection"]["timeout_ms"], 2_000);
}

#[test]
fn epoch_nine_legacy_and_newer_missing_run_receipts_block_instead_of_resurrecting_a_pass() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), false, false);
    let originals = original_records(&ctx);
    let mut original = originals[&"web:test".parse().unwrap()].clone();
    append(&ctx, [original.clone()]);
    assert_eq!(work_gates(&ctx)["gates"][0]["status"], "passed");
    original["id"] = json!("receipt_newer_legacy");
    original["ended_at_ms"] = json!(100);
    original.as_object_mut().unwrap().remove("target_freshness");
    append(&ctx, [original.clone()]);
    let legacy = work_gates(&ctx);
    assert_eq!(legacy["gates"][0]["status"], "unknown", "{legacy:#}");
    assert!(
        legacy["gates"][0]["freshness_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason["code"] == "legacy_metadata")
    );
    original["id"] = json!("receipt_newer_missing_run");
    original["ended_at_ms"] = json!(101);
    original.as_object_mut().unwrap().remove("run_id");
    append(&ctx, [original]);
    let missing_run = work_gates(&ctx);
    assert_eq!(
        missing_run["gates"][0]["status"], "unknown",
        "{missing_run:#}"
    );
    assert_eq!(
        missing_run["gates"][0]["targets"][0]["receipt_id"],
        "receipt_newer_missing_run"
    );
}

#[test]
fn live_worker_records_complete_original_dependency_proof_and_scoped_reuse() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), true, false);
    let result = run_repository_target(&ctx, "web:test");
    assert_eq!(result["ok"], true, "{result:#}");
    let receipts = fs::read_to_string(ctx.root().join(".agent/state/receipts.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|receipt| receipt["target"].is_object())
        .collect::<Vec<_>>();
    assert_eq!(receipts.len(), 2);
    assert_eq!(
        receipts[0]["target_freshness"]["state"], "complete",
        "{receipts:#?}"
    );
    assert_eq!(
        receipts[1]["target_freshness"]["state"], "complete",
        "{receipts:#?}"
    );
    assert_eq!(
        receipts[1]["target_freshness"]["dependency_execution_proof"][0]["receipt_id"],
        receipts[0]["id"]
    );
    assert_eq!(
        receipts[1]["target_freshness"]["dependency_execution_proof"][0]["run_id"],
        receipts[0]["run_id"]
    );
    assert_eq!(work_gates(&ctx)["overall"], "passed");
    fs::write(
        ctx.root().join("unrelated.md"),
        "Example unrelated change\n",
    )
    .unwrap();
    let retained = work_gates(&ctx);
    assert_eq!(retained["overall"], "passed", "{retained:#}");
    assert_eq!(
        retained["gates"][0]["targets"][0]["receipt_id"],
        receipts[1]["id"]
    );
}

#[test]
fn parallel_live_receipts_and_ignored_output_preserve_complete_authority() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), false, true);
    let config_path = ctx.root().join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["commands"]["api_test_command"] =
        toml::Value::String("mkdir -p target; printf 'example output' > target/example".into());
    fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
    fs::write(ctx.root().join(".gitignore"), "target/\n").unwrap();
    let ctx = RepoContext::load_from_root(ctx.root().to_path_buf()).unwrap();
    let result = run_repository_target(&ctx, "test");
    assert_eq!(result["ok"], true, "{result:#}");
    assert!(ctx.root().join("target/example").is_file());
    let report = work_gates(&ctx);
    assert_eq!(report["overall"], "passed", "{report:#}");
    assert_eq!(report["gates"][0]["targets"].as_array().unwrap().len(), 2);
}

#[test]
fn mutation_outside_exhaustive_inputs_records_incomplete_global_proof() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), false, false);
    let config_path = ctx.root().join(".jig.toml");
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["commands"]["web_test_command"] =
        toml::Value::String("printf 'mutated' > unrelated.md".into());
    fs::write(config_path, toml::to_string(&config).unwrap()).unwrap();
    let ctx = RepoContext::load_from_root(ctx.root().to_path_buf()).unwrap();
    let result = run_repository_target(&ctx, "web:test");
    assert_eq!(result["ok"], false, "{result:#}");
    let receipts = fs::read_to_string(ctx.root().join(".agent/state/receipts.jsonl")).unwrap();
    let receipt = receipts
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|receipt| receipt["target"]["component"] == "web")
        .unwrap();
    assert_eq!(receipt["target_freshness"]["state"], "incomplete");
    assert_eq!(
        receipt["target_freshness"]["global_execution_proof"]["state"],
        "mutated"
    );
    assert!(
        receipt["target_freshness"]["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason["code"] == "execution_mutated")
    );
    assert!(receipt["target_freshness"].get("identity").is_none());
}

#[test]
fn inspection_timeout_is_request_scoped_and_check_uses_recording_budget() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), true, false);
    append(&ctx, original_records(&ctx).into_values());
    for tool in [
        crate::tool_defs::tool::WORK_GATES,
        crate::tool_defs::tool::WORK_EVIDENCE,
    ] {
        let response = crate::runtime::call_tool(
            &ctx,
            tool,
            json!({"plan_id": "plan_1", "freshness_timeout_ms": 1}),
        )
        .unwrap();
        assert_eq!(response["gates"][0]["freshness"], "unknown", "{response:#}");
        assert_eq!(
            response["recovery"]["inspection"], "deadline_exhausted",
            "{response:#}"
        );
        assert_eq!(response["recovery"]["preview_available"], false);
        assert_eq!(response["recovery"]["next_step"]["read_only"], true);
        let operation = if tool == crate::tool_defs::tool::WORK_GATES {
            "gates"
        } else {
            "evidence"
        };
        assert_eq!(
            response["recovery"]["next_step"]["argv"],
            json!([
                "scripts/jig",
                "work",
                operation,
                "--plan-id",
                "plan_1",
                "--freshness-timeout-ms",
                "30000"
            ])
        );
        assert_eq!(
            response["gates"][0]["freshness_collection"]["timeout_ms"],
            1
        );
        assert!(
            response["gates"][0]["freshness_reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reason| reason["code"] == "collection_limit")
        );
        let response = crate::runtime::call_tool(
            &ctx,
            tool,
            json!({"plan_id": "plan_1", "freshness_timeout_ms": 30_000}),
        )
        .unwrap();
        assert_eq!(response["overall"], "passed", "{response:#}");
        assert_eq!(
            response["gates"][0]["freshness_collection"]["timeout_ms"],
            30_000
        );
        for invalid in [
            json!(0),
            json!(30_001),
            json!(-1),
            json!(1.5),
            json!("2000"),
            Value::Null,
        ] {
            assert!(
                crate::runtime::call_tool(
                    &ctx,
                    tool,
                    json!({"plan_id": "plan_1", "freshness_timeout_ms": invalid})
                )
                .is_err()
            );
        }
    }
    let status =
        crate::status::snapshot_with_freshness_timeout(&ctx, &|| false, Some(30_000)).unwrap();
    assert_eq!(
        status["work"]["gates"][0]["snapshot"]["gates"][0]["freshness_collection"]["timeout_ms"],
        30_000,
        "{status:#}"
    );
    let checked = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            gates: Vec::new(),
            tools: Vec::new(),
        })),
    )
    .unwrap();
    assert_eq!(checked["ok"], true, "{checked:#}");
    assert_eq!(
        checked["freshness_collection"]["timeout_ms"], 30_000,
        "{checked:#}"
    );
}

#[test]
fn archive_retains_original_dependency_receipts_across_runs_and_newer_blockers() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), true, false);
    let originals = original_records(&ctx);
    append(&ctx, originals.values().cloned());
    let mut unrelated = originals[&"api:test".parse().unwrap()].clone();
    unrelated["id"] = json!("receipt_other_plan");
    unrelated["plan_id"] = json!("plan_other");
    append(&ctx, [unrelated]);
    let before = work_gates(&ctx);
    assert_eq!(before["overall"], "passed", "{before:#}");
    let archived = crate::state::receipts_archive(
        &ctx,
        crate::state::StateArchiveRequest {
            before: "1000".into(),
            dry_run: false,
        },
    )
    .unwrap();
    assert_eq!(archived["receipts_archived"], 1, "{archived:#}");
    assert_eq!(archived["protected_receipts_retained"], 2, "{archived:#}");
    assert_eq!(work_gates(&ctx)["overall"], "passed");
    let mut blocker = originals[&"web:test".parse().unwrap()].clone();
    blocker["id"] = json!("receipt_newer_blocker");
    blocker["ended_at_ms"] = json!(100);
    blocker["exit_status"] = json!(1);
    blocker.as_object_mut().unwrap().remove("target_freshness");
    append(&ctx, [blocker]);
    crate::state::receipts_archive(
        &ctx,
        crate::state::StateArchiveRequest {
            before: "1000".into(),
            dry_run: false,
        },
    )
    .unwrap();
    let after = work_gates(&ctx);
    assert_eq!(after["gates"][0]["status"], "failed", "{after:#}");
    assert_eq!(
        after["gates"][0]["targets"][0]["receipt_id"],
        "receipt_newer_blocker"
    );
}

#[test]
fn archive_refuses_unresolvable_or_future_protected_dependency_metadata_without_rewrite() {
    for future in [false, true] {
        let temp = tempdir().unwrap();
        let ctx = fixture(temp.path(), true, false);
        let mut originals = original_records(&ctx);
        if future {
            originals.get_mut(&"api:test".parse().unwrap()).unwrap()["target_freshness"] =
                json!({"schema_version": 99, "future_proof": []});
        } else {
            originals.remove(&"api:test".parse().unwrap());
        }
        append(&ctx, originals.into_values());
        let path = ctx.state_file("receipts.jsonl");
        let before = fs::read(&path).unwrap();
        let error = crate::state::receipts_archive(
            &ctx,
            crate::state::StateArchiveRequest {
                before: "1000".into(),
                dry_run: false,
            },
        )
        .unwrap_err();
        assert!(
            error.to_string().contains(if future {
                "unsupported freshness schema"
            } else {
                "original dependency receipt is missing"
            }),
            "{error:#}"
        );
        assert_eq!(fs::read(path).unwrap(), before);
    }
}

#[test]
fn inherited_expiry_reaches_target_gate_status_latest_and_work_check_summaries() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), true, false);
    let mut originals = original_records(&ctx);
    let child = originals.get_mut(&"api:test".parse().unwrap()).unwrap();
    child["valid_until_ms"] = json!(u64::MAX);
    child["target_freshness"]["effective_valid_until_ms"] = json!(u64::MAX);
    child["target_freshness"]["effective_requires_time_validity"] = json!(true);
    let parent = originals.get_mut(&"web:test".parse().unwrap()).unwrap();
    parent["target_freshness"]["effective_valid_until_ms"] = json!(u64::MAX);
    parent["target_freshness"]["effective_requires_time_validity"] = json!(true);
    parent["target_freshness"]["dependency_execution_proof"][0]["effective_valid_until_ms"] =
        json!(u64::MAX);
    parent["target_freshness"]["dependency_execution_proof"][0]["effective_requires_time_validity"] =
        json!(true);
    append(&ctx, originals.into_values());
    let gates = work_gates(&ctx);
    let target = &gates["gates"][0]["targets"][0];
    assert_eq!(target["status"], "passed", "{gates:#}");
    assert!(target["valid_until_ms"].is_null());
    assert_eq!(target["effective_valid_until_ms"], u64::MAX);
    assert_eq!(target["effective_requires_time_validity"], true);
    let evidence = crate::runtime::call_tool(
        &ctx,
        crate::tool_defs::tool::WORK_EVIDENCE,
        json!({"plan_id": "plan_1"}),
    )
    .unwrap();
    assert_eq!(
        evidence["latest_passing_gates"][0]["effective_valid_until_ms"],
        u64::MAX,
        "{evidence:#}"
    );
    assert!(evidence["latest_passing_gates"][0]["valid_until_ms"].is_null());
    let status = crate::status::snapshot_with_freshness_timeout(&ctx, &|| false, None).unwrap();
    assert_eq!(
        status["work"]["gates"][0]["snapshot"]["gates"][0]["effective_valid_until_ms"],
        u64::MAX
    );
    let checked = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            gates: Vec::new(),
            tools: Vec::new(),
        })),
    )
    .unwrap();
    assert_eq!(checked["ok"], true, "{checked:#}");
    assert!(
        checked["run"].is_null(),
        "current originals should be reused"
    );
    assert_eq!(checked["effective_valid_until_ms"], u64::MAX);
    assert_eq!(checked["effective_requires_time_validity"], true);
    let journal = fs::read_to_string(ctx.state_file("receipts.jsonl")).unwrap();
    let batch: Value = serde_json::from_str(journal.lines().last().unwrap()).unwrap();
    assert!(batch["valid_until_ms"].is_null());
    assert_eq!(batch["evidence"]["effective_valid_until_ms"], u64::MAX);
}

#[test]
fn finish_rechecks_expiry_after_the_final_authority_check() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), true, false);
    let observed = crate::state::now_ms();
    let proof = crate::runtime::work::RequiredGateProof {
        worktree_fingerprint: crate::state::current_worktree_fingerprint(&ctx).fingerprint,
        valid_until_ms: Some(observed),
        requires_time_validity: true,
    };
    let error = crate::runtime::work::finish_after_required_gates_passed(
        &ctx,
        crate::command::WorkFinishRequest {
            plan_id: "plan_1".into(),
            resolution: Some("completed".into()),
            outcome: None,
        },
        proof,
        &|| false,
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("expired before the plan could close"),
        "{error:#}"
    );
    crate::state::ensure_plan_is_open(&ctx, "plan_1").unwrap();
}

#[test]
fn profile_summary_keeps_a_missing_required_boundary_beside_finite_validity() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), false, true);
    let mut originals = original_records(&ctx);
    let missing = originals.get_mut(&"api:test".parse().unwrap()).unwrap();
    missing["evidence"] = json!({"requires_time_validity": true});
    missing["target_freshness"]["effective_requires_time_validity"] = json!(true);
    let finite = originals.get_mut(&"web:test".parse().unwrap()).unwrap();
    finite["valid_until_ms"] = json!(u64::MAX);
    finite["target_freshness"]["effective_valid_until_ms"] = json!(u64::MAX);
    finite["target_freshness"]["effective_requires_time_validity"] = json!(true);
    append(&ctx, originals.into_values());
    let gates = work_gates(&ctx);
    let gate = &gates["gates"][0];
    assert_eq!(gate["status"], "unknown", "{gates:#}");
    assert!(gate["effective_valid_until_ms"].is_null(), "{gates:#}");
    assert_eq!(gate["effective_requires_time_validity"], true, "{gates:#}");
    let status = crate::status::snapshot_with_freshness_timeout(&ctx, &|| false, None).unwrap();
    let gate = &status["work"]["gates"][0]["snapshot"]["gates"][0];
    assert!(gate["effective_valid_until_ms"].is_null(), "{status:#}");
    assert_eq!(gate["effective_requires_time_validity"], true, "{status:#}");
}

#[test]
fn profile_unverified_original_keeps_missing_time_beside_finite_validity() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), false, true);
    let mut originals = original_records(&ctx);
    let missing = originals.get_mut(&"api:test".parse().unwrap()).unwrap();
    missing["evidence"] = json!({"requires_time_validity": true});
    missing["target_freshness"]["effective_requires_time_validity"] = json!(true);
    missing["target_freshness"]["dependency_execution_proof"] = json!([{
        "target": "shared:verify".parse::<TargetId>().unwrap(),
        "receipt_id": "receipt_absent", "run_id": "run_absent", "plan_id": "plan_1",
        "identity_digest": "absent", "conclusion": "success",
        "effective_requires_time_validity": false
    }]);
    let finite = originals.get_mut(&"web:test".parse().unwrap()).unwrap();
    finite["valid_until_ms"] = json!(u64::MAX);
    finite["target_freshness"]["effective_valid_until_ms"] = json!(u64::MAX);
    finite["target_freshness"]["effective_requires_time_validity"] = json!(true);
    append(&ctx, originals.into_values());
    let gates = work_gates(&ctx);
    assert!(
        gates.to_string().contains("dependency_proof_missing"),
        "{gates:#}"
    );
    let gate = &gates["gates"][0];
    assert_eq!(gate["status"], "unknown", "{gates:#}");
    assert!(gate["effective_valid_until_ms"].is_null(), "{gates:#}");
    assert_eq!(gate["effective_requires_time_validity"], true, "{gates:#}");
    let status = crate::status::snapshot_with_freshness_timeout(&ctx, &|| false, None).unwrap();
    let gate = &status["work"]["gates"][0]["snapshot"]["gates"][0];
    assert!(gate["effective_valid_until_ms"].is_null(), "{status:#}");
    assert_eq!(gate["effective_requires_time_validity"], true, "{status:#}");
}

#[test]
fn archive_can_shrink_a_journal_beyond_the_freshness_entry_ceiling() {
    let temp = tempdir().unwrap();
    let ctx = fixture(temp.path(), true, false);
    let originals = original_records(&ctx);
    append(&ctx, originals.values().cloned());
    let path = ctx.state_file("receipts.jsonl");
    let old = json!({"id": "receipt_archivable", "plan_id": "plan_other", "tool_name": "example",
        "args": {}, "started_at_ms": 1, "ended_at_ms": 2, "exit_status": 0,
        "stdout_preview": "", "stderr_preview": "", "changed_paths": [],
        "diff_stat": {"files": 0, "insertions": 0, "deletions": 0}});
    {
        let mut file =
            std::io::BufWriter::new(fs::OpenOptions::new().append(true).open(&path).unwrap());
        let line = format!("{old}\n");
        for _ in 0..250_001 {
            file.write_all(line.as_bytes()).unwrap();
        }
        file.flush().unwrap();
    }
    let archived = crate::state::receipts_archive(
        &ctx,
        crate::state::StateArchiveRequest {
            before: "1000".into(),
            dry_run: false,
        },
    )
    .unwrap();
    assert_eq!(archived["receipts_archived"], 250_001, "{archived:#}");
    assert_eq!(archived["protected_receipts_retained"], 2, "{archived:#}");
    assert_eq!(fs::read_to_string(path).unwrap().lines().count(), 2);
    assert_eq!(work_gates(&ctx)["overall"], "passed");
}
