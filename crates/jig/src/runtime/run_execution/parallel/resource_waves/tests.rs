use super::*;
use crate::test_env::TestRepoBuilder;

fn planned() -> PlannedTarget {
    PlannedTarget::new(
        "repo:test".parse().unwrap(),
        jig_contract::ActionIntent::Check,
        ActionRunner::command("test_command"),
        "sha256:input",
    )
}

fn pending(planned: &PlannedTarget) -> Pending<'_> {
    Pending {
        planned,
        position: PhasePosition::new(1, 1).unwrap(),
        budget: None,
        resolved: None,
        waited: true,
        force_execution: false,
        done: false,
    }
}

fn proof() -> TargetRunResult {
    let mut result = TargetRunResult::queued("repo:test".parse().unwrap(), "config", "input");
    result.status = RunStatus::Completed;
    result.conclusion = Some(RunConclusion::Success);
    result.valid_until_ms = Some(200);
    result.receipt_id = Some("receipt-original".into());
    result.reused_from = Some(jig_contract::ReusedTargetEvidenceV1 {
        receipt_id: "receipt-original".into(),
        run_id: "run-original".into(),
        plan_id: "plan-original".into(),
    });
    result
}

#[test]
fn shared_wave_postcondition_rejects_successful_capture_and_reuse_after_source_change() {
    let planned = planned();
    let mut pending = pending(&planned);
    let mut epoch = ExecutionSourceEpoch::from_plan("sha256:stable".into());
    epoch
        .prepare_read_only_layer_with(2, || Ok("sha256:stable".into()))
        .unwrap();
    let capture = CompletedTargetCapture::now(
        Some(100),
        TargetCapture::from_process(0, String::new(), String::new(), ResultParser::ExitCode),
    );
    let fingerprint =
        epoch.observe_read_only_layer_postcondition_with(|| Ok("sha256:changed".into()));
    let (finished, _) =
        epoch.finish_started_read_only_layer_target(&planned, &fingerprint, capture);
    assert_eq!(finished.capture.conclusion, RunConclusion::Failure);
    assert_eq!(
        finished.capture.findings[0].source.as_deref(),
        Some("effect_policy")
    );
    let reused = finalize_wave_reuse(&mut pending, &epoch, &fingerprint, proof(), 199);
    assert!(matches!(reused, Err(TargetStop::Blocked(_))));
    assert!(!pending.done);
    assert!(
        !pending.force_execution,
        "a source mismatch is not permission to rerun"
    );
    assert_eq!(epoch.metrics().count, 2);
}

#[test]
fn rejected_reuse_publishes_the_wave_source_observation() {
    let temp = tempfile::tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let catalog = RepositoryCatalog::from_context(&ctx).unwrap();
    let planned = planned();
    let plan = RunPlan::new(
        "example-plan",
        "sha256:config",
        jig_contract::SourceIdentity::new(None, "sha256:stable"),
        vec![planned.clone()],
        vec![vec![planned.target.clone()]],
    );
    let run = crate::state::DurableRun {
        result: jig_contract::RunResult::queued(
            "example-run",
            plan.id.clone(),
            0,
            vec![TargetRunResult::queued(
                planned.target.clone(),
                plan.config_digest.clone(),
                planned.input_digest.clone(),
            )],
        ),
        plan,
        work_plan_id: None,
        cancel_requested: false,
    };
    let finisher = TargetFinisher {
        alias_override: None,
        ctx: &ctx,
        catalog: &catalog,
        run: &run,
        work_plan_id: None,
        record_receipts: false,
        freshness: None,
    };
    let mut pending = pending(&planned);
    let slots = ExecutionSlots::new();
    let member = Member {
        index: 0,
        lease: None,
        _slot: slots.try_acquire().unwrap(),
    };
    let mut control = CancellationOnlyRunControl {
        cancelled: &|| Ok(false),
    };
    let mut epoch = ExecutionSourceEpoch::from_plan("sha256:stable".into());
    epoch
        .prepare_read_only_layer_with(1, || Ok("sha256:stable".into()))
        .unwrap();
    let fingerprint =
        epoch.observe_read_only_layer_postcondition_with(|| Ok("sha256:changed".into()));
    let mut published = None;

    publish_outcome(
        &finisher,
        &mut control,
        &epoch,
        &mut pending,
        &member,
        WaveOutcome::Reused(proof()),
        &fingerprint,
        7,
        &mut |target, result, _, source, wave_number| {
            published = Some((target.clone(), result, source.cloned(), wave_number));
            Ok(())
        },
    )
    .unwrap();

    let (target, result, source, wave_number) = published.unwrap();
    assert_eq!(target, planned.target);
    assert_eq!(result.conclusion, Some(RunConclusion::Blocked));
    assert_eq!(source, Some(Ok("sha256:changed".into())));
    assert_eq!(wave_number, Some(7));
    assert!(pending.done);
}

#[test]
fn expired_wave_reuse_requires_execution_without_restarting_admission_budget() {
    let temp = tempfile::tempdir().unwrap();
    TestRepoBuilder::new(temp.path()).write();
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let planned = planned();
    let mut pending = pending(&planned);
    let budget = TargetBudget::new(&ctx, &planned);
    pending.budget = Some(budget);
    let epoch = ExecutionSourceEpoch::from_plan("sha256:stable".into());
    let fingerprint = Ok("sha256:stable".into());
    let valid = finalize_wave_reuse(&mut pending, &epoch, &fingerprint, proof(), 199)
        .unwrap()
        .unwrap();
    assert_eq!(valid.receipt_id.as_deref(), Some("receipt-original"));
    assert!(valid.reused_from.is_some());
    assert!(!pending.force_execution);
    let expired = finalize_wave_reuse(&mut pending, &epoch, &fingerprint, proof(), 200).unwrap();
    assert!(expired.is_none());
    assert!(pending.force_execution);
    assert!(!pending.done);
    assert_eq!(pending.budget, Some(budget));
    assert!(
        pending.resolved.is_none(),
        "the retry must re-resolve resource authority"
    );
}
