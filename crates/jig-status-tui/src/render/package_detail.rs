use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};
use serde_json::Value;

use crate::model::{
    App, DETAIL_SECTION_ITEM_LIMIT, EXTENSION_ROW_LIMIT, FacetView, PackageView, SourceView,
    sanitize_text,
};

use super::{ACCENT, BAD, MUTED, panel, status_style};

const DETAIL_FIELD_CHAR_LIMIT: usize = 256;

pub(super) fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let Some(package) = app.detail_package() else {
        frame.render_widget(
            Paragraph::new("The selected package is no longer available. Press Esc to return.")
                .block(panel("Package detail")),
            area,
        );
        return;
    };

    let lines = detail_lines(app, package);
    let sizing_paragraph = Paragraph::new(lines.clone())
        .block(panel("Package detail"))
        .wrap(Wrap { trim: false });
    let max_scroll = sizing_paragraph
        .line_count(area.width.saturating_sub(2))
        .saturating_sub(usize::from(area.height));
    app.set_package_detail_scroll_limit(max_scroll);
    let scroll = app.package_detail_scroll().min(max_scroll);
    let scroll_position = if max_scroll == 0 {
        "all visible".to_owned()
    } else {
        format!("row {}/{}", scroll + 1, max_scroll + 1)
    };
    let title = format!(
        "Package detail — {} · {scroll_position}",
        bounded_text(&package.id)
    );
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel(&title))
            .wrap(Wrap { trim: false })
            .scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0)),
        area,
    );
}

pub(super) fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let first = Line::from(vec![
        Span::styled(
            " Esc/Enter back ",
            Style::default().fg(Color::Black).bg(ACCENT),
        ),
        Span::raw("  j/k scroll  PgUp/PgDn  Home/End  r refresh  q quit"),
    ]);
    let second = if let Some(error) = &app.last_error {
        Line::from(vec![
            Span::styled("Last refresh error: ", Style::default().fg(BAD)),
            Span::raw(error.clone()),
        ])
    } else if app.refreshing {
        Line::from("Refreshing in the background; package selection remains stable.")
            .style(Style::default().fg(MUTED))
    } else {
        Line::from("Read-only package observation from the selected status provider.")
            .style(Style::default().fg(MUTED))
    };
    frame.render_widget(
        Paragraph::new(vec![first, second]).alignment(Alignment::Left),
        area,
    );
}

fn detail_lines(app: &App, package: &PackageView) -> Vec<Line<'static>> {
    let provider = app
        .current_provider()
        .map(|provider| bounded_text(provider.display_name.as_deref().unwrap_or(&provider.id)))
        .unwrap_or_else(|| "<unknown>".to_owned());
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{}  ", bounded_text(&package.id)),
                Style::default().fg(ACCENT).bold(),
            ),
            Span::styled(bounded_text(&package.title), Style::default().bold()),
        ]),
        Line::from(format!("Provider: {provider}")),
        Line::default(),
        section_heading("Progress facets"),
    ];
    append_facet(&mut lines, "Specification", &package.specification);
    append_facet(&mut lines, "Implementation", &package.implementation);
    append_facet(&mut lines, "Verification", &package.verification);

    lines.extend([Line::default(), section_heading("Dependencies")]);
    if package.dependencies.is_empty() {
        lines.push(muted_line("None"));
    } else {
        lines.extend(
            package
                .dependencies
                .iter()
                .take(DETAIL_SECTION_ITEM_LIMIT)
                .map(|dependency| Line::from(format!("• {}", bounded_text(dependency)))),
        );
        append_omitted_count(&mut lines, package.dependencies.len(), "dependencies");
    }

    lines.extend([
        Line::default(),
        section_heading(&format!(
            "Acceptance checks ({}/{} complete)",
            package.acceptance_complete, package.acceptance_total
        )),
    ]);
    if package.acceptance_checks.is_empty() {
        lines.push(muted_line("No acceptance checks reported."));
    } else {
        for check in package
            .acceptance_checks
            .iter()
            .take(DETAIL_SECTION_ITEM_LIMIT)
        {
            let id = check
                .id
                .as_deref()
                .map(|id| format!(" {}", bounded_text(id)))
                .unwrap_or_default();
            let target = check
                .target
                .as_deref()
                .map(|target| format!("  target {}", bounded_text(target)))
                .unwrap_or_default();
            lines.push(Line::from(vec![
                Span::styled(
                    format!("#{}{}  ", check.ordinal, id),
                    Style::default().fg(ACCENT),
                ),
                Span::styled(
                    format!(
                        "{} [{}]",
                        bounded_text(&check.state),
                        bounded_text(&check.category)
                    ),
                    status_style(&check.category),
                ),
                Span::raw(format!(
                    "{target}{}",
                    bounded_source_suffix(check.source.as_ref())
                )),
            ]));
        }
        append_omitted_count(
            &mut lines,
            package.acceptance_checks.len(),
            "acceptance checks",
        );
    }

    lines.extend([
        Line::default(),
        section_heading(&format!("Blockers ({})", package.blockers.len())),
    ]);
    if package.blockers.is_empty() {
        lines.push(muted_line("None"));
    } else {
        for blocker in package.blockers.iter().take(DETAIL_SECTION_ITEM_LIMIT) {
            let related = blocker
                .related_work_package
                .as_deref()
                .map(|id| format!(" · related {}", bounded_text(id)))
                .unwrap_or_default();
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{}: ", bounded_text(&blocker.code)),
                    Style::default().fg(BAD).bold(),
                ),
                Span::raw(format!(
                    "{}{related}{}",
                    bounded_text(&blocker.message),
                    bounded_source_suffix(blocker.source.as_ref())
                )),
            ]));
        }
        append_omitted_count(&mut lines, package.blockers.len(), "blockers");
    }

    lines.extend([
        Line::default(),
        section_heading(&format!("Evidence ({})", package.evidence.len())),
    ]);
    if package.evidence.is_empty() {
        lines.push(muted_line("None"));
    } else {
        for evidence in package.evidence.iter().take(DETAIL_SECTION_ITEM_LIMIT) {
            let digest = evidence
                .digest
                .as_deref()
                .map(|digest| format!(" · digest {}", bounded_text(digest)))
                .unwrap_or_default();
            lines.push(Line::from(format!(
                "• [{}] {}{digest}{}",
                bounded_text(&evidence.kind),
                bounded_text(&evidence.reference),
                bounded_source_suffix(evidence.source.as_ref())
            )));
        }
        append_omitted_count(&mut lines, package.evidence.len(), "evidence entries");
    }

    if !package.extensions.is_empty() {
        lines.extend([
            Line::default(),
            section_heading("Provider-specific details"),
        ]);
        append_extensions(&mut lines, package);
    }

    lines
}

fn append_facet(lines: &mut Vec<Line<'static>>, label: &str, facet: &FacetView) {
    let summary = facet
        .summary
        .as_deref()
        .map(|summary| format!(" - {}", bounded_text(summary)))
        .unwrap_or_default();
    lines.push(Line::from(vec![
        Span::styled(format!("{label}: "), Style::default().bold()),
        Span::styled(
            format!(
                "{} [{}]",
                bounded_text(&facet.state),
                bounded_text(&facet.category)
            ),
            status_style(&facet.category),
        ),
        Span::raw(format!(
            "{summary}{}",
            bounded_source_suffix(facet.source.as_ref())
        )),
    ]));
    if let Some(digest) = &facet.digest {
        lines.push(Line::from(format!("  digest: {}", bounded_text(digest))));
    }
}

fn append_extensions(lines: &mut Vec<Line<'static>>, package: &PackageView) {
    let mut remaining = EXTENSION_ROW_LIMIT;
    let mut truncated = false;
    for (namespace, value) in &package.extensions {
        if remaining == 0 {
            truncated = true;
            break;
        }
        lines.push(Line::from(Span::styled(
            bounded_text(namespace),
            Style::default().fg(ACCENT).bold(),
        )));
        remaining -= 1;
        if !append_extension_value(lines, value, 1, &mut remaining) {
            truncated = true;
            break;
        }
    }
    if truncated {
        lines.push(muted_line(&format!(
            "Provider-specific details truncated after {EXTENSION_ROW_LIMIT} rows."
        )));
    }
}

fn append_extension_value(
    lines: &mut Vec<Line<'static>>,
    value: &Value,
    depth: usize,
    remaining: &mut usize,
) -> bool {
    if *remaining == 0 {
        return false;
    }
    let indent = "  ".repeat(depth.min(8));
    match value {
        Value::Object(values) if values.is_empty() => push_row(lines, "{ }", &indent, remaining),
        Value::Object(values) => {
            for (key, nested) in values {
                if *remaining == 0 {
                    return false;
                }
                let key = bounded_text(key);
                if is_scalar(nested) {
                    push_row(
                        lines,
                        &format!("{key}: {}", scalar_text(nested)),
                        &indent,
                        remaining,
                    );
                } else {
                    push_row(lines, &format!("{key}:"), &indent, remaining);
                    if !append_extension_value(lines, nested, depth + 1, remaining) {
                        return false;
                    }
                }
            }
        }
        Value::Array(values) if values.is_empty() => push_row(lines, "[ ]", &indent, remaining),
        Value::Array(values) => {
            for (index, nested) in values.iter().enumerate() {
                if *remaining == 0 {
                    return false;
                }
                if is_scalar(nested) {
                    push_row(
                        lines,
                        &format!("• {}", scalar_text(nested)),
                        &indent,
                        remaining,
                    );
                } else {
                    push_row(lines, &format!("[{index}]"), &indent, remaining);
                    if !append_extension_value(lines, nested, depth + 1, remaining) {
                        return false;
                    }
                }
            }
        }
        scalar => push_row(lines, &scalar_text(scalar), &indent, remaining),
    }
    true
}

fn push_row(lines: &mut Vec<Line<'static>>, text: &str, indent: &str, remaining: &mut usize) {
    if *remaining == 0 {
        return;
    }
    lines.push(Line::from(format!("{indent}{text}")));
    *remaining -= 1;
}

fn scalar_text(value: &Value) -> String {
    match value {
        Value::String(text) => bounded_text(text),
        Value::Null => "null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Array(_) | Value::Object(_) => "<nested>".to_owned(),
    }
}

fn is_scalar(value: &Value) -> bool {
    !matches!(value, Value::Array(_) | Value::Object(_))
}

fn append_omitted_count(lines: &mut Vec<Line<'static>>, total: usize, label: &str) {
    let omitted = total.saturating_sub(DETAIL_SECTION_ITEM_LIMIT);
    if omitted > 0 {
        lines.push(muted_line(&format!(
            "{omitted} additional {label} omitted from the terminal detail view."
        )));
    }
}

fn bounded_text(text: &str) -> String {
    let sanitized = sanitize_text(text);
    let mut characters = sanitized.chars();
    let mut bounded = characters
        .by_ref()
        .take(DETAIL_FIELD_CHAR_LIMIT)
        .collect::<String>();
    if characters.next().is_some() {
        bounded.push('…');
    }
    bounded
}

fn bounded_source_suffix(source: Option<&SourceView>) -> String {
    let Some(source) = source else {
        return String::new();
    };
    let path = bounded_text(&source.path);
    match (source.line, source.column) {
        (Some(line), Some(column)) => format!(" @ {path}:{line}:{column}"),
        (Some(line), None) => format!(" @ {path}:{line}"),
        _ => format!(" @ {path}"),
    }
}

fn section_heading(title: &str) -> Line<'static> {
    Line::from(Span::styled(
        title.to_owned(),
        Style::default().fg(ACCENT).bold(),
    ))
}

fn muted_line(text: &str) -> Line<'static> {
    Line::from(text.to_owned()).style(Style::default().fg(MUTED))
}
