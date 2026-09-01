use std::fmt::Write as _;

use super::{
    escape, format_duration, format_ms, page_shell, plan_link, render_gates_table,
    status_badge_class,
};
use crate::{DashboardSnapshot, ScheduledOccurrenceView, TimelineItem};

pub(crate) fn render_dashboard(snapshot: &DashboardSnapshot, namespace: &str) -> String {
    let mut body = String::with_capacity(16 * 1024);
    let _ = writeln!(
        body,
        "<header><div><h1>{} <span class=\"muted\">flight recorder</span></h1><div class=\"muted\">branch {} · jig {} · contract v{} · generated {}</div></div>",
        escape(&snapshot.repo.name),
        escape(&snapshot.repo.default_branch),
        escape(snapshot.harness.display_runtime_version()),
        snapshot.harness.contract_version,
        format_ms(Some(snapshot.generated_at_ms))
    );
    for (label, count) in [
        ("sessions", snapshot.counts.sessions),
        ("open plans", snapshot.counts.open_plans),
        ("decisions", snapshot.counts.decisions),
    ] {
        let _ = writeln!(body, "<div class=\"stat\"><b>{count}</b> {label}</div>");
    }
    body.push_str("</header>\n<h2>Open plans</h2>\n");
    if snapshot.open_plans.is_empty() {
        body.push_str("<div class=\"muted\">No open plans. Start one with <span class=\"mono\">scripts/jig work start --title …</span></div>\n");
    }
    for plan in &snapshot.open_plans {
        let overall = plan
            .gates
            .as_ref()
            .map_or("unknown", |g| g.overall.as_str());
        let _ = writeln!(
            body,
            "<div class=\"card\"><div><b>{}</b> <span class=\"badge {}\">{}</span><div class=\"muted\">{} · opened {}</div></div>",
            escape(&plan.title),
            if overall == "passed" { "ok" } else { "warn" },
            escape(overall),
            plan_link(namespace, Some(&plan.plan_id)),
            format_ms(plan.opened_at_ms)
        );
        if let Some(e) = &plan.gates_error {
            let _ = writeln!(
                body,
                "<div class=\"hint\">Gate status unavailable: {}</div>",
                escape(e)
            );
        } else if let Some(g) = &plan.gates {
            render_gates_table(&mut body, g);
        }
        body.push_str("</div>\n");
    }
    if !snapshot.failures.is_empty() {
        body.push_str("<h2>Recent failures</h2><table><tr><th>when</th><th>tool</th><th>exit</th><th>plan</th><th>stderr</th></tr>\n");
        for f in &snapshot.failures {
            let detail = if f.stderr_preview.is_empty() {
                "<span class=\"muted\">—</span>".into()
            } else {
                format!(
                    "<details><summary>stderr</summary><pre class=\"err\">{}</pre></details>",
                    escape(&f.stderr_preview)
                )
            };
            let _ = writeln!(
                body,
                "<tr><td class=\"mono muted\">{}</td><td class=\"mono\">{}</td><td><span class=\"badge fail\">exit {}</span></td><td>{}</td><td>{}</td></tr>",
                format_ms(f.ended_at_ms),
                escape(&f.tool_name),
                f.exit_status,
                plan_link(namespace, f.plan_id.as_deref()),
                detail
            );
        }
        body.push_str("</table>\n");
    }
    if !snapshot.history.is_empty() {
        body.push_str("<h2>Recently finished work</h2><table><tr><th>closed</th><th>plan</th><th>took</th><th>resolution</th></tr>\n");
        for p in &snapshot.history {
            let _ = writeln!(
                body,
                "<tr><td class=\"mono muted\">{}</td><td><b>{}</b><br>{}</td><td class=\"muted\">{}</td><td class=\"muted\">{}</td></tr>",
                format_ms(p.closed_at_ms),
                escape(&p.title),
                plan_link(namespace, Some(&p.plan_id)),
                format_duration(p.duration_ms),
                escape(p.resolution.as_deref().unwrap_or(""))
            );
        }
        body.push_str("</table>\n");
    }
    if !snapshot.tool_stats.is_empty() {
        body.push_str("<h2>Check health</h2><table><tr><th>tool</th><th>last run</th><th>last status</th><th>runs</th><th>failures</th><th>avg time</th></tr>\n");
        for s in &snapshot.tool_stats {
            let (badge, label) = if s.last_exit_status == 0 {
                ("ok", "pass".into())
            } else {
                ("fail", format!("exit {}", s.last_exit_status))
            };
            let _ = writeln!(
                body,
                "<tr><td class=\"mono\">{}</td><td class=\"mono muted\">{}</td><td><span class=\"badge {}\">{}</span></td><td>{}</td><td>{}</td><td class=\"muted\">{}</td></tr>",
                escape(&s.tool),
                format_ms(Some(s.last_ended_at_ms)),
                badge,
                label,
                s.runs,
                s.failures,
                format_duration(Some(s.avg_duration_ms))
            );
        }
        body.push_str("</table>\n");
    }
    body.push_str("<h2>Loops</h2>\n");
    if let Some(e) = &snapshot.loops_error {
        let _ = writeln!(
            body,
            "<div class=\"hint\">Loop status unavailable: {}</div>",
            escape(e)
        );
    } else if let Some(loops) = &snapshot.loops {
        if loops.workflows.is_empty() {
            body.push_str("<div class=\"muted\">No loop workflows configured.</div>\n");
        } else {
            body.push_str("<table><tr><th>workflow</th><th>kind</th><th>enabled</th><th>schedule</th><th>last run</th><th>next run</th></tr>");
            for w in &loops.workflows {
                let schedule = w.schedule.as_ref().map_or_else(
                    || "manual".into(),
                    |schedule| format!("{} ({})", schedule.cron, schedule.timezone),
                );
                let last_run = w.schedule_state.as_ref().map_or_else(
                    || "—".into(),
                    |state| {
                        state.last_status.as_ref().map_or_else(
                            || "never".into(),
                            |status| {
                                format!("{} ({})", status, format_ms(state.last_scheduled_at_ms))
                            },
                        )
                    },
                );
                let next_run = w
                    .schedule_state
                    .as_ref()
                    .map_or_else(|| "—".into(), |state| format_ms(Some(state.next_at_ms)));
                let _ = writeln!(
                    body,
                    "<tr><td class=\"mono\">{}</td><td class=\"muted\">{}</td><td><span class=\"badge {}\">{}</span></td><td class=\"mono muted\">{}</td><td>{}</td><td>{}</td></tr>",
                    escape(&w.id),
                    escape(&w.kind),
                    if w.enabled { "ok" } else { "idle" },
                    if w.enabled { "enabled" } else { "disabled" },
                    escape(&schedule),
                    escape(&last_run),
                    escape(&next_run),
                );
            }
            body.push_str("</table>\n");
        }
        for lease in &loops.leases {
            let _ = writeln!(
                body,
                "<div class=\"muted\">lease held: <span class=\"mono\">{}</span> until {}</div>",
                escape(&lease.key),
                format_ms(lease.expires_at_ms)
            );
        }
        if !loops.scheduled_occurrences.is_empty() {
            body.push_str("<h3>Loop runs</h3><table><tr><th>workflow</th><th>when</th><th>status</th><th>worker receipt</th><th>retained worktree</th></tr>");
            for occurrence in loops.scheduled_occurrences.iter().rev() {
                let _ = writeln!(
                    body,
                    "<tr><td class=\"mono\">{}</td><td>{}</td><td><span class=\"badge {}\">{}</span></td><td class=\"mono muted\">{}</td><td class=\"mono muted\">{}</td></tr>",
                    escape(&occurrence.workflow_id),
                    occurrence_time(occurrence),
                    status_badge_class(&occurrence.status),
                    escape(&occurrence.status),
                    escape(occurrence.worker_receipt_id.as_deref().unwrap_or("—")),
                    escape(occurrence.worktree.as_deref().unwrap_or("—")),
                );
            }
            body.push_str("</table>\n");
        }
        for a in &loops.needs_attention.exhausted_attempts {
            let _ = writeln!(
                body,
                "<div class=\"hint\">needs attention: <span class=\"mono\">{} / {}</span> exhausted its attempt budget.</div>",
                escape(&a.workflow_id),
                escape(&a.item_key)
            );
        }
        for occurrence in &loops.needs_attention.scheduled_occurrences {
            let detail = occurrence.error.as_deref().unwrap_or("claim expired");
            let _ = writeln!(
                body,
                "<div class=\"hint\">scheduled run needs attention: <span class=\"mono\">{}</span> — {}</div>",
                escape(&occurrence.occurrence_id),
                escape(detail),
            );
        }
    }
    body.push_str("<h2>Timeline</h2><div>\n");
    for show in [
        "all",
        "receipts",
        "failures",
        "plans",
        "sessions",
        "decisions",
    ] {
        let class = if show == snapshot.timeline_show {
            "chip active"
        } else {
            "chip"
        };
        let _ = writeln!(
            body,
            "<a class=\"{class}\" href=\"{namespace}?show={show}\">{show}</a>"
        );
    }
    body.push_str("</div>\n");
    if snapshot.timeline.is_empty() {
        body.push_str("<div class=\"muted\">No recorded activity for this filter.</div>\n");
    } else {
        body.push_str("<table><tr><th>when</th><th>kind</th><th>what</th><th>plan</th></tr>\n");
        for item in &snapshot.timeline {
            render_timeline(&mut body, item, namespace);
        }
        body.push_str("</table>\n");
    }
    page_shell(&snapshot.repo.name, &body)
}

fn occurrence_time(occurrence: &ScheduledOccurrenceView) -> String {
    if occurrence.scheduled_at_ms != 0 {
        return format_ms(Some(occurrence.scheduled_at_ms));
    }
    if occurrence.started_at_ms == 0 {
        return "Manual".into();
    }
    format!("Manual ({})", format_ms(Some(occurrence.started_at_ms)))
}

fn render_timeline(out: &mut String, item: &TimelineItem, namespace: &str) {
    let (kind, what) = match item {
        TimelineItem::Receipt(v) => {
            let label = if v.exit_status == 0 {
                "pass".into()
            } else {
                format!("exit {}", v.exit_status)
            };
            (
                "receipt",
                format!(
                    "<span class=\"mono\">{}</span> <span class=\"badge {}\">{}</span> <span class=\"muted\">{}</span>",
                    escape(&v.tool_name),
                    if v.exit_status == 0 { "ok" } else { "fail" },
                    label,
                    format_duration(v.duration_ms)
                ),
            )
        }
        TimelineItem::Plan(v) => (
            "plan",
            format!(
                "plan {} <b>{}</b>",
                escape(&v.event),
                escape(v.title.as_deref().or(v.resolution.as_deref()).unwrap_or(""))
            ),
        ),
        TimelineItem::Session(v) => (
            "session",
            format!(
                "session {} <span class=\"mono muted\">{}</span> {}",
                escape(&v.event),
                escape(&v.session_id),
                escape(v.outcome.as_deref().unwrap_or(""))
            ),
        ),
        TimelineItem::Decision(v) => (
            "decision",
            format!(
                "decision <b>{}</b> → {} <span class=\"muted\">{}</span>",
                escape(&v.title),
                escape(&v.selected_option),
                escape(&v.rationale)
            ),
        ),
    };
    let _ = writeln!(
        out,
        "<tr><td class=\"mono muted\">{}</td><td class=\"muted\">{}</td><td>{}</td><td>{}</td></tr>",
        format_ms(item.timestamp_ms()),
        kind,
        what,
        plan_link(namespace, item.plan_id())
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_occurrence_uses_its_start_time_instead_of_the_epoch_sentinel() {
        let occurrence = serde_json::from_value::<ScheduledOccurrenceView>(serde_json::json!({
            "occurrence_id": "example@manual",
            "workflow_id": "example",
            "scheduled_at_ms": 0,
            "started_at_ms": 1_788_134_400_000_u64,
            "status": "succeeded",
            "finished_at_ms": 1_788_134_401_000_u64,
            "worker_receipt_id": null,
            "worktree": null,
            "error": null
        }))
        .unwrap();

        let rendered = occurrence_time(&occurrence);

        assert!(rendered.starts_with("Manual ("), "{rendered}");
        assert!(!rendered.contains("1970"), "{rendered}");
    }

    #[test]
    fn legacy_manual_occurrence_without_a_start_time_still_avoids_the_epoch() {
        let occurrence = serde_json::from_value::<ScheduledOccurrenceView>(serde_json::json!({
            "occurrence_id": "example@manual",
            "workflow_id": "example",
            "scheduled_at_ms": 0,
            "status": "failed",
            "finished_at_ms": null,
            "worker_receipt_id": null,
            "worktree": null,
            "error": null
        }))
        .unwrap();

        assert_eq!(occurrence_time(&occurrence), "Manual");
    }
}
