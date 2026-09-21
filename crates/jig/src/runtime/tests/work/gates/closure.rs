use super::*;

#[test]
fn work_finish_holds_checkout_read_lease_through_plan_closure() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_id = open_test_plan(&ctx);
    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            rust_focus: Default::default(),
            projection: crate::surface::ResponseSurface::Standard,
            plan_id: plan_id.clone(),
            gates: Vec::new(),
            tools: Vec::new(),
            phase: None,
            explain: false,
        })),
    )
    .unwrap();

    let (start_writer_tx, start_writer_rx) = mpsc::channel();
    let (attempting_tx, attempting_rx) = mpsc::channel();
    let (observed_open_tx, observed_open_rx) = mpsc::channel();
    let worker_root = temp.path().to_path_buf();
    let worker_plan_id = plan_id.clone();
    let triggered = AtomicBool::new(false);

    std::thread::scope(|scope| {
        scope.spawn(move || {
            start_writer_rx.recv().unwrap();
            let worker_ctx = RepoContext::load_from(&worker_root).unwrap();
            attempting_tx.send(()).unwrap();
            let _writer = crate::state::acquire_repository_execution_lease(
                &worker_ctx,
                &[jig_contract::ActionEffect::Worktree],
            )
            .unwrap();
            observed_open_tx
                .send(crate::state::ensure_plan_is_open(&worker_ctx, &worker_plan_id).is_ok())
                .unwrap();
        });

        let output = crate::runtime::work::finish_with_cancellation(
            &ctx,
            crate::command::WorkFinishRequest {
                plan_id: plan_id.clone(),
                resolution: Some("done".into()),
                outcome: Some("success".into()),
            },
            &|| {
                if !triggered.swap(true, Ordering::AcqRel) {
                    start_writer_tx.send(()).unwrap();
                    attempting_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                }
                false
            },
        )
        .unwrap();

        assert_eq!(output["ok"], true, "{output:#}");
        assert!(
            !observed_open_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            "an effectful writer acquired the checkout before plan closure committed"
        );
    });
}

#[test]
fn work_finish_rejects_gate_authority_that_changed_after_evaluation() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_id = open_test_plan(&ctx);

    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            rust_focus: Default::default(),
            projection: crate::surface::ResponseSurface::Standard,
            plan_id: plan_id.clone(),
            gates: Vec::new(),
            tools: Vec::new(),
            phase: None,
            explain: false,
        })),
    )
    .unwrap();

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            projection: crate::surface::ResponseSurface::Standard,
            freshness_timeout_ms: None,
            plan_id: Some(plan_id.clone()),
        })),
    )
    .unwrap();
    assert_eq!(gates["overall"], "passed", "{gates:#}");

    let config_path = temp.path().join(".jig.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    fs::write(
        config_path,
        config.replace("id = \"custom\"", "id = \"replacement\""),
    )
    .unwrap();

    let error = crate::runtime::work::finish_after_required_gates_passed(
        &ctx,
        crate::command::WorkFinishRequest {
            plan_id: plan_id.clone(),
            resolution: Some("done".into()),
            outcome: Some("success".into()),
        },
        crate::runtime::work::RequiredGateProof {
            worktree_fingerprint: gates["current_worktree_fingerprint"]
                .as_str()
                .map(str::to_owned),
            ..Default::default()
        },
        &|| false,
    )
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("Work gate configuration changed while evaluating required work gates"),
        "{error}"
    );
    crate::state::ensure_plan_is_open(&ctx, &plan_id).unwrap();
}

#[test]
fn work_finish_rejects_source_that_changed_after_gate_evaluation() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_id = open_test_plan(&ctx);

    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            rust_focus: Default::default(),
            projection: crate::surface::ResponseSurface::Standard,
            plan_id: plan_id.clone(),
            gates: Vec::new(),
            tools: Vec::new(),
            phase: None,
            explain: false,
        })),
    )
    .unwrap();
    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            projection: crate::surface::ResponseSurface::Standard,
            freshness_timeout_ms: None,
            plan_id: Some(plan_id.clone()),
        })),
    )
    .unwrap();
    assert_eq!(gates["overall"], "passed", "{gates:#}");
    fs::write(temp.path().join("changed-after-gates.txt"), "changed\n").unwrap();

    let error = crate::runtime::work::finish_after_required_gates_passed(
        &ctx,
        crate::command::WorkFinishRequest {
            plan_id: plan_id.clone(),
            resolution: Some("done".into()),
            outcome: Some("success".into()),
        },
        crate::runtime::work::RequiredGateProof {
            worktree_fingerprint: gates["current_worktree_fingerprint"]
                .as_str()
                .map(str::to_owned),
            ..Default::default()
        },
        &|| false,
    )
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("Worktree changed while evaluating required work gates"),
        "{error}"
    );
    crate::state::ensure_plan_is_open(&ctx, &plan_id).unwrap();
}
