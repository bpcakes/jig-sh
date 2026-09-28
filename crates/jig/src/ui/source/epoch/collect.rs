use super::*;

pub(super) fn collect_receipts(
    context: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<StreamSection<ReceiptFacts>, SourceError> {
    let path = context.state_file("receipts.jsonl");
    let mut facts = MutableReceiptFacts::default();
    let mut failures = NewestRows::new(LimitId::Failures.ceiling());
    let mut timeline = NewestRows::new(LimitId::Timeline.ceiling());
    let result = scan_dashboard_jsonl_raw(&path, cancelled, |raw| {
        let receipt =
            serde_json::from_slice::<DashboardReceiptRecord>(raw.bytes).with_context(|| {
                format!(
                    "Failed to decode receipt record at byte {}",
                    raw.start_offset
                )
            })?;
        facts.count = facts.count.saturating_add(1);
        facts.failed = facts
            .failed
            .saturating_add(u64::from(receipt.exit_status != 0));
        let diff_summary = Some(receipt_diff_summary(&receipt));
        if receipt.exit_status != 0 {
            failures.push(Failure {
                id: receipt.id.clone(),
                tool_name: receipt.tool_name.clone(),
                ended_at_ms: Some(receipt.ended_at_ms),
                exit_status: i64::from(receipt.exit_status),
                stderr_preview: bounded_text(&receipt.stderr_preview, LimitId::FailureStderrChars)?,
            });
        }
        if receipt.invoked_command_key.is_some() {
            let duration = receipt.ended_at_ms.saturating_sub(receipt.started_at_ms);
            if !facts.tools.contains_key(&receipt.tool_name)
                && facts.tools.len() == MAX_AGGREGATION_KEYS
            {
                anyhow::bail!(
                    "dashboard receipt tool aggregation exceeds the {MAX_AGGREGATION_KEYS}-key working-set limit"
                );
            }
            let tool = facts
                .tools
                .entry(receipt.tool_name.clone())
                .or_insert(MutableToolStat {
                    runs: 0,
                    failures: 0,
                    total_duration_ms: 0,
                    last_exit_status: i64::from(receipt.exit_status),
                    last_ended_at_ms: receipt.ended_at_ms,
                });
            tool.runs = tool.runs.saturating_add(1);
            tool.failures = tool
                .failures
                .saturating_add(u64::from(receipt.exit_status != 0));
            tool.total_duration_ms = tool.total_duration_ms.saturating_add(duration);
            if receipt.ended_at_ms >= tool.last_ended_at_ms {
                tool.last_ended_at_ms = receipt.ended_at_ms;
                tool.last_exit_status = i64::from(receipt.exit_status);
            }
        }
        timeline.push(TimelineRow::Receipt(ReceiptTimelineRow {
            stable_identity: stable_identity("receipt", raw),
            timestamp_ms: Some(receipt.ended_at_ms),
            id: receipt.id,
            tool_name: receipt.tool_name,
            invoked_command_key: receipt.invoked_command_key,
            exit_status: i64::from(receipt.exit_status),
            started_at_ms: Some(receipt.started_at_ms),
            ended_at_ms: Some(receipt.ended_at_ms),
            duration_ms: Some(receipt.ended_at_ms.saturating_sub(receipt.started_at_ms)),
            diff_summary,
            changed_path_count: Some(
                u64::try_from(
                    receipt
                        .changed_path_count
                        .unwrap_or(receipt.changed_paths.len()),
                )
                .unwrap_or(u64::MAX),
            ),
            stderr_preview: (receipt.exit_status != 0)
                .then(|| bounded_text(&receipt.stderr_preview, LimitId::FailureStderrChars))
                .transpose()?,
        }));
        Ok(())
    });
    let error = stream_error(CollectionDomain::Receipts, result, cancelled)?;
    facts.failures = failures.into_rows();
    facts.timeline = timeline.into_rows();
    facts.failures.sort_by(|left, right| {
        right
            .ended_at_ms
            .cmp(&left.ended_at_ms)
            .then_with(|| left.id.cmp(&right.id))
    });
    let tool_count = facts.tools.len();
    let mut tool_stats = facts
        .tools
        .into_iter()
        .map(|(tool, stat)| ToolStat {
            tool,
            runs: stat.runs,
            failures: stat.failures,
            last_exit_status: stat.last_exit_status,
            last_ended_at_ms: stat.last_ended_at_ms,
            avg_duration_ms: stat.total_duration_ms / stat.runs.max(1),
        })
        .collect::<Vec<_>>();
    tool_stats.sort_by(|left, right| {
        right
            .last_ended_at_ms
            .cmp(&left.last_ended_at_ms)
            .then_with(|| left.tool.cmp(&right.tool))
    });
    tool_stats.truncate(LimitId::ToolStats.ceiling());
    Ok(StreamSection {
        data: ReceiptFacts {
            count: facts.count,
            failed: facts.failed,
            failures: facts.failures,
            tool_stats,
            tool_count,
            timeline: facts.timeline,
        },
        error,
    })
}
