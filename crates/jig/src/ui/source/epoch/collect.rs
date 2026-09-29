use super::*;

/// Folds finished target results from run history into the dashboard's
/// timeline, failure list and per-target statistics in one bounded pass.
pub(super) fn collect_runs(
    context: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<StreamSection<RunFacts>, SourceError> {
    let path = context.state_file("runs.jsonl");
    let mut facts = MutableRunFacts::default();
    let mut failures = NewestRows::new(LimitId::Failures.ceiling());
    let mut timeline = NewestRows::new(LimitId::Timeline.ceiling());
    let result = scan_dashboard_jsonl_raw(&path, cancelled, |raw| {
        let RunHistoryEvent::TargetCompleted(event) = run_history_event(raw.bytes)
            .with_context(|| format!("Failed to decode run record at byte {}", raw.start_offset))?
        else {
            return Ok(());
        };
        let failed = event.failed();
        let CompletedTargetEvent { run_id, result } = *event;
        let target = result.target.to_string();
        let conclusion = result.conclusion.map(snake_case);
        let duration_ms = result
            .started_at_ms
            .zip(result.ended_at_ms)
            .map(|(started, ended)| ended.saturating_sub(started));
        let output_tail = (result.conclusion != Some(RunConclusion::Success))
            .then(|| output_tail(result.output_tail.as_ref()))
            .transpose()?;
        facts.count = facts.count.saturating_add(1);
        facts.failed = facts.failed.saturating_add(u64::from(failed));
        if failed {
            failures.push(Failure {
                run_id: run_id.clone(),
                target: target.clone(),
                conclusion: conclusion.clone().unwrap_or_default(),
                exit_code: result.exit_code.map(i64::from),
                ended_at_ms: result.ended_at_ms,
                output_tail: output_tail
                    .clone()
                    .map_or_else(|| bounded_tail("", LimitId::FailureOutputChars), Ok)?,
            });
        }
        if !facts.targets.contains_key(&target) && facts.targets.len() == MAX_AGGREGATION_KEYS {
            anyhow::bail!(
                "dashboard target aggregation exceeds the {MAX_AGGREGATION_KEYS}-key working-set limit"
            );
        }
        let ended_at_ms = result.ended_at_ms.unwrap_or_default();
        let stat = facts
            .targets
            .entry(target.clone())
            .or_insert(MutableTargetStat {
                runs: 0,
                failures: 0,
                total_duration_ms: 0,
                last_conclusion: conclusion.clone(),
                last_ended_at_ms: ended_at_ms,
            });
        stat.runs = stat.runs.saturating_add(1);
        stat.failures = stat.failures.saturating_add(u64::from(failed));
        stat.total_duration_ms = stat
            .total_duration_ms
            .saturating_add(duration_ms.unwrap_or_default());
        if ended_at_ms >= stat.last_ended_at_ms {
            stat.last_ended_at_ms = ended_at_ms;
            stat.last_conclusion.clone_from(&conclusion);
        }
        timeline.push(TimelineRow {
            stable_identity: stable_identity("target_result", raw),
            timestamp_ms: result.ended_at_ms.or(result.started_at_ms),
            run_id,
            target,
            status: snake_case(result.status),
            conclusion,
            exit_code: result.exit_code.map(i64::from),
            started_at_ms: result.started_at_ms,
            ended_at_ms: result.ended_at_ms,
            duration_ms,
            finding_count: result.finding_count,
            output_tail,
        });
        Ok(())
    });
    let error = stream_error(CollectionDomain::Runs, result, cancelled)?;
    facts.failures = failures.into_rows();
    facts.timeline = timeline.into_rows();
    facts.failures.sort_by(|left, right| {
        right
            .ended_at_ms
            .cmp(&left.ended_at_ms)
            .then_with(|| left.run_id.cmp(&right.run_id))
            .then_with(|| left.target.cmp(&right.target))
    });
    let target_count = facts.targets.len();
    let mut target_stats = facts
        .targets
        .into_iter()
        .map(|(target, stat)| TargetStat {
            target,
            runs: stat.runs,
            failures: stat.failures,
            last_conclusion: stat.last_conclusion,
            last_ended_at_ms: stat.last_ended_at_ms,
            avg_duration_ms: stat.total_duration_ms / stat.runs.max(1),
        })
        .collect::<Vec<_>>();
    target_stats.sort_by(|left, right| {
        right
            .last_ended_at_ms
            .cmp(&left.last_ended_at_ms)
            .then_with(|| left.target.cmp(&right.target))
    });
    target_stats.truncate(LimitId::TargetStats.ceiling());
    Ok(StreamSection {
        data: RunFacts {
            count: facts.count,
            failed: facts.failed,
            failures: facts.failures,
            target_stats,
            target_count,
            timeline: facts.timeline,
        },
        error,
    })
}

/// The stderr tail when a target wrote one, otherwise its stdout tail.
fn output_tail(tail: Option<&TargetOutputTailV1>) -> Result<BoundedText, SourceError> {
    let text = tail.map_or("", |tail| {
        if tail.stderr.is_empty() {
            &tail.stdout
        } else {
            &tail.stderr
        }
    });
    bounded_tail(text, LimitId::FailureOutputChars)
}

fn snake_case(value: impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}
