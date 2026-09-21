use super::*;

pub(super) fn check_prepared_with_failure_mode(
    ctx: &RepoContext,
    plan_id: &str,
    batch: PreparedCheckBatch,
    started: u64,
    failure_mode: FailureMode,
    execution: WorkCheckExecution,
    observer: &mut dyn ExecutionControl,
) -> Result<Value> {
    let outcome = execute_check_batch(ctx, plan_id, &batch, failure_mode, execution, observer)?;
    let receipt_result =
        record_check_batch_receipt(ctx, plan_id, started, &batch, &outcome, observer);

    if (failure_mode.aborts() || observer.cancelled())
        && let Some(failure) = outcome.failure.as_ref()
    {
        return match receipt_result {
            Ok(_) => Err(anyhow!("{:#}", failure.error)),
            Err(receipt_error) => {
                bail!(
                    "{:#}\nwork check batch receipt recording also failed:\n{receipt_error:#}",
                    failure.error
                )
            }
        };
    }
    let receipt_id = receipt_result?;
    let failure_message = outcome
        .failure
        .as_ref()
        .map(|failure| format!("{:#}", failure.error));
    let mut value = json!({
        "ok": outcome.failure.is_none(),
        "plan_id": plan_id,
        "checks": outcome.results,
        "change_evidence": batch.changes.to_value(),
        "gate_evidence": outcome.gate_evidence,
        "error": failure_message,
        "receipt_id": receipt_id,
    });
    if let Some(validity) = batch_effective_time(ctx, &outcome) {
        value["effective_valid_until_ms"] = json!(validity.effective_valid_until_ms);
        value["effective_requires_time_validity"] =
            json!(validity.effective_requires_time_validity);
    }
    Ok(value)
}
