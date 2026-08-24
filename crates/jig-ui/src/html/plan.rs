use super::{escape, format_duration, format_ms, page_shell, render_gates_table};
use crate::{DecisionView, PlanSnapshot, ReceiptView};
use std::fmt::Write as _;

pub(crate) fn render_plan_page(snapshot: &PlanSnapshot, namespace: &str) -> String {
    let p = &snapshot.plan;
    let mut body = String::with_capacity(16 * 1024);
    let _ = writeln!(
        body,
        "<div class=\"muted\"><a href=\"{namespace}\">← flight recorder</a></div><header><div><h1>{}</h1><div class=\"muted\">{} <span class=\"badge {}\">{}</span> · opened {}</div></div></header>",
        escape(&p.title),
        escape(&p.plan_id),
        if p.state == "open" { "warn" } else { "idle" },
        escape(&p.state),
        format_ms(p.opened_at_ms)
    );
    if let Some(r) = &p.resolution {
        let _ = writeln!(body, "<div class=\"hint\">Resolution: {}</div>", escape(r));
    }
    if let Some(text) = &snapshot.body
        && !text.trim().is_empty()
    {
        let _ = writeln!(body, "<h2>Plan body</h2><pre>{}</pre>", escape(text));
    }
    if let Some(e) = &snapshot.body_error {
        let _ = writeln!(
            body,
            "<div class=\"hint\">Plan body unavailable: {}</div>",
            escape(e)
        );
    }
    body.push_str("<h2>Gates</h2>");
    if let Some(e) = &snapshot.gates_error {
        let _ = writeln!(
            body,
            "<div class=\"hint\">Gate status unavailable: {}</div>",
            escape(e)
        );
    } else if let Some(g) = &snapshot.gates {
        render_gates_table(&mut body, g);
    }
    if !snapshot.decisions.is_empty() {
        body.push_str("<h2>Decisions</h2>");
        for d in &snapshot.decisions {
            render_decision(&mut body, d);
        }
    }
    body.push_str("<h2>Receipts</h2>");
    if snapshot.receipts.is_empty() {
        body.push_str("<div class=\"muted\">No receipts recorded for this plan.</div>");
    } else {
        body.push_str("<table><tr><th>when</th><th>tool</th><th>status</th><th>took</th><th>diff</th><th>output</th></tr>");
        for r in &snapshot.receipts {
            render_receipt(&mut body, r);
        }
        body.push_str("</table>");
    }
    page_shell(&p.title, &body)
}
fn render_decision(out: &mut String, d: &DecisionView) {
    let alternatives = if d.alternatives.is_empty() {
        String::new()
    } else {
        format!(
            "<div class=\"muted\">Alternatives: {}</div>",
            d.alternatives
                .iter()
                .map(|a| escape(a))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let _ = writeln!(
        out,
        "<div class=\"card\"><b>{}</b> → {}<div class=\"muted\">{}</div>{}</div>",
        escape(&d.title),
        escape(&d.selected_option),
        escape(&d.rationale),
        alternatives
    );
}
fn render_receipt(out: &mut String, r: &ReceiptView) {
    let (badge, label) = if r.exit_status == 0 {
        ("ok", "pass".into())
    } else {
        ("fail", format!("exit {}", r.exit_status))
    };
    let mut detail = String::new();
    if !r.stdout_preview.is_empty() || !r.stderr_preview.is_empty() || !r.changed_paths.is_empty() {
        detail.push_str("<details><summary>detail</summary>");
        if !r.stdout_preview.is_empty() {
            let _ = write!(detail, "<pre>{}</pre>", escape(&r.stdout_preview));
        }
        if !r.stderr_preview.is_empty() {
            let _ = write!(
                detail,
                "<pre class=\"err\">{}</pre>",
                escape(&r.stderr_preview)
            );
        }
        detail.push_str(
            &r.changed_paths
                .iter()
                .map(|p| escape(p))
                .collect::<Vec<_>>()
                .join("<br>"),
        );
        detail.push_str("</details>");
    }
    let _ = writeln!(
        out,
        "<tr><td class=\"mono muted\">{}</td><td class=\"mono\">{}</td><td><span class=\"badge {}\">{}</span></td><td class=\"muted\">{}</td><td class=\"muted\">{}</td><td>{}</td></tr>",
        format_ms(r.ended_at_ms),
        escape(&r.tool_name),
        badge,
        label,
        format_duration(r.duration_ms),
        escape(r.diff_summary.as_deref().unwrap_or("")),
        detail
    );
}
