use super::*;

pub(super) fn collect_sessions(
    context: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<StreamSection<SessionFacts>, SourceError> {
    let path = context.state_file("sessions.jsonl");
    let mut canonical = HashMap::<String, (DashboardSessionEvent, String, u64)>::new();
    let mut facts = SessionFacts::default();
    let mut timeline = NewestRows::new(LimitId::Timeline.ceiling());
    let result = scan_dashboard_jsonl_raw(&path, cancelled, |raw| {
        let envelope =
            serde_json::from_slice::<DashboardSessionEvent>(raw.bytes).with_context(|| {
                format!(
                    "Failed to decode session record at byte {}",
                    raw.start_offset
                )
            })?;
        match canonical.get(&envelope.id) {
            Some((existing, _, _)) if existing == &envelope => return Ok(()),
            Some((_, _, first_line)) => anyhow::bail!(
                "Conflicting session event envelope for ID `{}` at JSONL records {} and {} in {}",
                envelope.id,
                first_line,
                raw.line_number,
                path.display(),
            ),
            None => {
                let identity = stable_identity("session", raw);
                if canonical.len() < MAX_AGGREGATION_KEYS {
                    canonical.insert(envelope.id.clone(), (envelope, identity, raw.line_number));
                } else {
                    observe_session(&mut facts, &mut timeline, envelope, identity);
                }
            }
        }
        Ok(())
    });
    let error = stream_error(CollectionDomain::Sessions, result, cancelled)?;
    let mut events = canonical.into_values().collect::<Vec<_>>();
    events.sort_by(|(left, _, _), (right, _, _)| {
        left.timestamp_ms
            .cmp(&right.timestamp_ms)
            .then_with(|| left.id.cmp(&right.id))
    });
    for (event, stable_identity, _) in events {
        observe_session(&mut facts, &mut timeline, event, stable_identity);
    }
    facts.timeline = timeline.into_rows();
    Ok(StreamSection { data: facts, error })
}

fn observe_session(
    facts: &mut SessionFacts,
    timeline: &mut NewestRows<TimelineRow>,
    event: DashboardSessionEvent,
    stable_identity: String,
) {
    facts.events = facts.events.saturating_add(1);
    facts.starts = facts
        .starts
        .saturating_add(u64::from(event.event == "start"));
    timeline.push(TimelineRow::Session(SessionTimelineRow {
        stable_identity,
        timestamp_ms: Some(event.timestamp_ms),
        id: event.id,
        event: event.event,
        session_id: event.session_id,
        outcome: event.outcome,
    }));
}

pub(super) fn collect_plans(
    context: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<StreamSection<PlanFacts>, SourceError> {
    let path = context.state_file("plans.jsonl");
    let mut facts = PlanFacts::default();
    let mut timeline = NewestRows::new(LimitId::Timeline.ceiling());
    let result = scan_dashboard_jsonl_raw(&path, cancelled, |raw| {
        let event = serde_json::from_slice::<DashboardPlanEvent>(raw.bytes).with_context(|| {
            format!("Failed to decode plan record at byte {}", raw.start_offset)
        })?;
        facts.events = facts.events.saturating_add(1);
        let stable_identity = stable_identity("plan", raw);
        let (id, event_name, plan_id, timestamp_ms, title, resolution) = match event {
            DashboardPlanEvent::Open {
                id,
                plan_id,
                timestamp_ms,
                title,
                body_path,
                baseline,
            } => {
                if !facts.distinct.contains_key(&plan_id)
                    && facts.distinct.len() == MAX_AGGREGATION_KEYS
                {
                    anyhow::bail!(
                        "dashboard plan aggregation exceeds the {MAX_AGGREGATION_KEYS}-key working-set limit"
                    );
                }
                facts.open_events = facts.open_events.saturating_add(1);
                if facts.distinct.get(&plan_id).is_some_and(|info| info.opened) {
                    facts.gate_errors.entry(plan_id.clone()).or_insert_with(|| {
                        format!(
                            "Plan {plan_id} has multiple Open records; repair the append-only plan stream before collecting gate evidence"
                        )
                    });
                }
                let info = facts.distinct.entry(plan_id.clone()).or_default();
                info.title.clone_from(&title);
                info.body_path = body_path;
                info.opened_at_ms = Some(timestamp_ms);
                info.baseline = baseline;
                info.opened = true;
                (
                    id,
                    "open".to_string(),
                    plan_id,
                    timestamp_ms,
                    Some(title),
                    None,
                )
            }
            DashboardPlanEvent::Append {
                id,
                plan_id,
                timestamp_ms,
                body_path: _,
            } => (id, "append".to_string(), plan_id, timestamp_ms, None, None),
            DashboardPlanEvent::Close {
                id,
                plan_id,
                timestamp_ms,
                resolution,
            } => {
                if !facts.distinct.contains_key(&plan_id)
                    && facts.distinct.len() == MAX_AGGREGATION_KEYS
                {
                    anyhow::bail!(
                        "dashboard plan aggregation exceeds the {MAX_AGGREGATION_KEYS}-key working-set limit"
                    );
                }
                let info = facts.distinct.entry(plan_id.clone()).or_default();
                info.closed = true;
                info.closed_at_ms = Some(timestamp_ms);
                info.resolution.clone_from(&resolution);
                (
                    id,
                    "close".to_string(),
                    plan_id,
                    timestamp_ms,
                    None,
                    resolution,
                )
            }
            DashboardPlanEvent::Unknown {
                id,
                plan_id,
                event,
                timestamp_ms,
            } => (id, event, plan_id, timestamp_ms, None, None),
        };
        timeline.push(TimelineRow::Plan(PlanTimelineRow {
            stable_identity,
            timestamp_ms: Some(timestamp_ms),
            id,
            event: event_name,
            plan_id,
            title,
            resolution,
        }));
        Ok(())
    });
    facts.timeline = timeline.into_rows();
    finish_stream(CollectionDomain::Plans, facts, result, cancelled)
}

pub(super) fn collect_decisions(
    context: &RepoContext,
    cancelled: &dyn Fn() -> bool,
) -> Result<StreamSection<DecisionFacts>, SourceError> {
    let path = context.state_file("decisions.jsonl");
    let mut facts = DecisionFacts::default();
    let mut timeline = NewestRows::new(LimitId::Timeline.ceiling());
    let result = scan_dashboard_jsonl_raw(&path, cancelled, |raw| {
        let decision =
            serde_json::from_slice::<DashboardDecisionRecord>(raw.bytes).with_context(|| {
                format!(
                    "Failed to decode decision record at byte {}",
                    raw.start_offset
                )
            })?;
        facts.count = facts.count.saturating_add(1);
        push_recent_file_order(
            &mut facts.recent,
            StatusDecisionSummary {
                id: decision.id.clone(),
                session_id: decision.session_id.clone(),
                plan_id: decision.plan_id.clone(),
                title: decision.title.clone(),
                selected_option: decision.selected_option.clone(),
                timestamp_ms: decision.timestamp_ms,
            },
            STATUS_RECENT_ROWS,
        );
        timeline.push(TimelineRow::Decision(DecisionTimelineRow {
            stable_identity: stable_identity("decision", raw),
            timestamp_ms: Some(decision.timestamp_ms),
            id: decision.id,
            plan_id: decision.plan_id,
            title: decision.title,
            selected_option: decision.selected_option,
            rationale: bounded_text(&decision.rationale, LimitId::TimelineDecisionRationaleChars)?,
        }));
        Ok(())
    });
    facts.timeline = timeline.into_rows();
    let mut section = finish_stream(CollectionDomain::Decisions, facts, result, cancelled)?;
    section.data.recent.reverse();
    Ok(section)
}

pub(super) fn collect_receipts(
    context: &RepoContext,
    mut gate_indexes: crate::state::WorkGateReceiptIndexes,
    cancelled: &dyn Fn() -> bool,
) -> Result<
    (
        StreamSection<ReceiptFacts>,
        crate::state::WorkGateReceiptIndexes,
    ),
    SourceError,
> {
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
        gate_indexes.observe(&receipt);
        facts.count = facts.count.saturating_add(1);
        facts.failed = facts
            .failed
            .saturating_add(u64::from(receipt.exit_status != 0));
        let diff_summary = Some(receipt_diff_summary(&receipt));
        push_recent_file_order(
            &mut facts.recent,
            StatusReceiptSummary {
                id: receipt.id.clone(),
                session_id: receipt.session_id.clone(),
                tool_name: receipt.tool_name.clone(),
                invoked_command_key: receipt.invoked_command_key.clone(),
                plan_id: receipt.plan_id.clone(),
                exit_status: i64::from(receipt.exit_status),
                started_at_ms: Some(receipt.started_at_ms),
                ended_at_ms: Some(receipt.ended_at_ms),
                diff_summary: diff_summary.clone(),
            },
            STATUS_RECENT_ROWS,
        );
        if receipt.exit_status != 0 {
            failures.push(Failure {
                id: receipt.id.clone(),
                tool_name: receipt.tool_name.clone(),
                plan_id: receipt.plan_id.clone(),
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
            plan_id: receipt.plan_id,
            session_id: receipt.session_id,
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
    facts.recent.reverse();
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
    Ok((
        StreamSection {
            data: ReceiptFacts {
                count: facts.count,
                failed: facts.failed,
                recent: facts.recent,
                failures: facts.failures,
                tool_stats,
                tool_count,
                timeline: facts.timeline,
            },
            error,
        },
        gate_indexes,
    ))
}

pub(super) fn collect_gates(
    context: &RepoContext,
    baselines: &BTreeMap<String, Option<crate::state::PlanBaseline>>,
    indexes: BTreeMap<String, crate::state::WorkGateReceiptIndex>,
    plan_state: &'static str,
    cancelled: &dyn Fn() -> bool,
) -> Result<BTreeMap<String, GateFacts>, SourceError> {
    if baselines.is_empty() {
        return Ok(BTreeMap::new());
    }
    let reports = match crate::runtime::dashboard_open_plan_reports_with_cancellation(
        context, baselines, indexes, plan_state, cancelled,
    ) {
        Ok(reports) => reports,
        Err(error)
            if crate::cancellation::is_status_collection_cancellation(&error) || cancelled() =>
        {
            return Err(SourceError::Cancelled);
        }
        Err(error) => {
            let message = format!("{error:#}");
            return Ok(baselines
                .keys()
                .map(|plan_id| {
                    (
                        plan_id.clone(),
                        GateFacts {
                            error: Some(message.clone()),
                            ..GateFacts::default()
                        },
                    )
                })
                .collect());
        }
    };
    Ok(baselines
        .keys()
        .map(|plan_id| {
            let facts = reports.get(plan_id).map_or_else(
                || GateFacts {
                    error: Some(format!(
                        "Batched gate evaluation did not return requested plan '{plan_id}'"
                    )),
                    ..GateFacts::default()
                },
                |report| {
                    let status = report.status_view();
                    match report.recorder_view() {
                        Ok(recorder) => GateFacts {
                            status: Some(status),
                            recorder: Some(recorder),
                            error: None,
                        },
                        Err(error) => GateFacts {
                            status: Some(status),
                            error: Some(error.to_string()),
                            recorder: None,
                        },
                    }
                },
            );
            (plan_id.clone(), facts)
        })
        .collect())
}
