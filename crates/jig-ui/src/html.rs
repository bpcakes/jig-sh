//! Shared HTML building blocks for the flight-recorder pages.
//!
//! Everything is server-rendered with inline CSS and no scripts, so pages work
//! under the strict `default-src 'none'` policy the server sends. The meta
//! refresh keeps long-running check output current without JavaScript.

use std::fmt::Write as _;

use time::OffsetDateTime;

use crate::GatesView;

mod dashboard;
mod plan;

pub(crate) use dashboard::render_dashboard;
pub(crate) use plan::render_plan_page;

const STYLES: &str = "\
/* Hallmark · pre-emit critique: P5 H4 E5 S5 R5 V4 · macrostructure: existing flight recorder · tone: technical-austere · anchor hue: vermilion · contrast: pass (46–50) · tokens: pass (58) · mobile: pass (36,59,61–69) */\
:root{color-scheme:dark;\
--color-ink:oklch(92.532% .02228 89.798);--color-ink-soft:oklch(74.914% .03227 88.401);\
--color-ink-faint:oklch(46.131% .02607 89.912);--color-paper:oklch(15.908% .00637 91.691);\
--color-paper-2:oklch(19.208% .00846 84.577);--color-rule:oklch(26.997% .01701 86.869);\
--color-rule-2:oklch(33.007% .02289 89.487);--color-hot:oklch(66.864% .22107 34.593);\
--color-hot-deep:oklch(53.617% .18213 34.408);--color-acid:oklch(87.738% .16389 113.853);\
--color-steel:oklch(74.801% .06369 215.707);--color-terminal:oklch(14.014% .00517 86.596);\
--font-body:\"JetBrains Mono\",ui-monospace,SFMono-Regular,Menlo,monospace}\
html,body{overflow-x:clip}\
body{font-family:var(--font-body);margin:0;padding:1.2rem;background:var(--color-paper);\
color:var(--color-ink);font-size:14px;line-height:1.45;font-variant-numeric:tabular-nums}\
::selection{background:var(--color-hot);color:var(--color-paper)}\
h1{font-size:1.2rem;margin:0 0 .2rem}h1,h2{overflow-wrap:anywhere;min-width:0}\
h2{font-size:1rem;margin:1.6rem 0 .5rem;color:var(--color-ink-soft)}\
a{color:var(--color-steel);text-decoration:none;white-space:nowrap}\
a:hover{color:var(--color-hot);text-decoration:underline}a:active{color:var(--color-hot-deep)}\
a:focus-visible,summary:focus-visible{outline:2px solid var(--color-hot);outline-offset:3px}\
table{border-collapse:collapse;width:100%;margin:.4rem 0}\
th,td{text-align:left;padding:.25rem .6rem .25rem 0;vertical-align:top;\
border-bottom:1px solid var(--color-rule)}th{color:var(--color-ink-soft);font-weight:normal}\
.muted{color:var(--color-ink-soft)}.mono{white-space:nowrap}\
.badge{display:inline-block;padding:0 .45rem;border-radius:.6rem;font-size:.85em}\
.ok{background:color-mix(in oklch,var(--color-acid) 12%,var(--color-paper-2));color:var(--color-acid)}\
.fail{background:color-mix(in oklch,var(--color-hot) 7%,var(--color-paper-2));color:var(--color-hot)}\
.warn{background:color-mix(in oklch,var(--color-steel) 12%,var(--color-paper-2));color:var(--color-steel)}\
.idle{background:var(--color-rule);color:var(--color-ink-soft)}\
.card{border:1px solid var(--color-rule);border-radius:.5rem;padding:.7rem .9rem;margin:.6rem 0;\
background:var(--color-paper-2);overflow-x:auto}\
.hint{background:color-mix(in oklch,var(--color-steel) 7%,var(--color-paper-2));\
border-left:3px solid var(--color-steel);padding:.4rem .7rem;margin:.4rem 0;color:var(--color-steel)}\
pre{margin:.3rem 0;padding:.4rem .6rem;background:var(--color-terminal);border-radius:.3rem;\
overflow-x:auto;color:var(--color-ink);max-width:100%}pre.err{color:var(--color-hot)}\
details{margin:.2rem 0}summary{cursor:pointer;color:var(--color-ink-soft)}summary:active{color:var(--color-hot-deep)}\
.chip{display:inline-block;padding:.05rem .6rem;border:1px solid var(--color-rule-2);border-radius:.8rem;\
margin-right:.35rem;color:var(--color-ink-soft);white-space:nowrap}\
.chip.active{border-color:var(--color-hot);color:var(--color-hot)}\
header{display:flex;flex-wrap:wrap;gap:1.4rem;align-items:baseline}\
header .stat{color:var(--color-ink-soft)}header .stat b{color:var(--color-ink)}";

/// Wrap a page body in the shared document shell.
pub(super) fn page_shell(title: &str, body: &str) -> String {
    let mut page = String::with_capacity(body.len() + 4 * 1024);
    page.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    page.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    page.push_str("<meta http-equiv=\"refresh\" content=\"10\">\n");
    let _ = writeln!(page, "<title>jig ui — {}</title>", escape(title));
    page.push_str("<style>");
    page.push_str(STYLES);
    page.push_str("</style>\n</head>\n<body>\n");
    page.push_str(body);
    page.push_str("</body>\n</html>\n");
    page
}

/// Gate table shared by the dashboard plan cards and the plan detail page.
pub(super) fn render_gates_table(page: &mut String, gates: &GatesView) {
    if gates.gates.is_empty() {
        page.push_str("<div class=\"muted\">No work gates configured.</div>\n");
        return;
    }
    page.push_str(
        "<table><tr><th>gate</th><th>tool / skill</th><th>required</th><th>status</th>\
<th>freshness</th><th>last run</th><th>diff</th></tr>\n",
    );
    for gate in &gates.gates {
        let status = gate.status.as_str();
        let subject = gate
            .tool
            .as_deref()
            .or(gate.skill.as_deref())
            .unwrap_or("—");
        let _ = writeln!(
            page,
            "<tr><td>{}</td><td class=\"mono\">{}</td><td>{}</td>\
<td><span class=\"badge {}\">{}</span></td><td class=\"muted\">{}</td>\
<td class=\"mono muted\">{}</td><td class=\"muted\">{}</td></tr>",
            escape(&gate.id),
            escape(subject),
            if gate.required { "yes" } else { "no" },
            status_badge_class(status),
            escape(status),
            escape(gate.freshness.as_deref().unwrap_or("—")),
            format_ms(gate.ended_at_ms),
            escape(gate.diff_summary.as_deref().unwrap_or("")),
        );
    }
    page.push_str("</table>\n");
}

pub(super) fn status_badge_class(status: &str) -> &'static str {
    match status {
        "passed" | "succeeded" | "acknowledged" => "ok",
        "failed" | "invalid_output" => "fail",
        "missing" | "unsupported" => "idle",
        _ => "warn",
    }
}

/// Render a plan id as a link to its detail page; empty when absent.
pub(super) fn plan_link(namespace: &str, plan_id: Option<&str>) -> String {
    match plan_id {
        Some(plan_id) => format!(
            "<a class=\"mono\" href=\"{namespace}plan/{id}\">{id}</a>",
            id = escape(plan_id)
        ),
        None => String::new(),
    }
}

pub(super) fn format_duration(duration_ms: Option<u64>) -> String {
    let Some(ms) = duration_ms else {
        return String::new();
    };
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{}m {}s", ms / 60_000, (ms % 60_000) / 1000)
    }
}

pub(super) fn format_ms(timestamp_ms: Option<u64>) -> String {
    let Some(ms) = timestamp_ms else {
        return "—".to_string();
    };
    let Ok(time) = OffsetDateTime::from_unix_timestamp((ms / 1000) as i64) else {
        return format!("{ms}ms");
    };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}Z",
        time.year(),
        u8::from(time.month()),
        time.day(),
        time.hour(),
        time.minute(),
        time.second(),
    )
}

pub(super) fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::page_shell;

    #[test]
    fn page_shell_uses_the_landing_page_palette() {
        let page = page_shell("palette", "");

        for color in [
            "oklch(15.908% .00637 91.691)",
            "oklch(92.532% .02228 89.798)",
            "oklch(66.864% .22107 34.593)",
            "oklch(87.738% .16389 113.853)",
            "oklch(74.801% .06369 215.707)",
        ] {
            assert!(page.contains(color), "missing landing color {color}");
        }
        assert!(!page.contains("#101418"));
        assert!(!page.contains("#7fb3e8"));
    }
}
