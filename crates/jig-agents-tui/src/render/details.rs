use ratatui::{
    Frame,
    layout::Rect,
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{ACCENT, BAD, MUTED, WARN, panel, stale_projection_style, stale_usage_style};
use crate::model::{App, Focus, Inspection, WindowRole};

const STALE_PREFIX: &str = "stale · ";

/// A detail line and the column its wrapped continuation rows start at.
pub(super) struct Detail {
    line: Line<'static>,
    hang: usize,
}

pub(super) fn draw_details(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    now: u64,
    best: Option<usize>,
) {
    let block = panel(detail_title(app), app.focus == Focus::Details);
    if app.selected_row().is_none() {
        app.set_detail_scroll_limit(0);
        frame.render_widget(Paragraph::new("No home selected.").block(block), area);
        return;
    }
    let width = usize::from(area.width.saturating_sub(2));
    let lines = detail_lines(app, now, best)
        .into_iter()
        .flat_map(|detail| wrap(detail, width))
        .collect::<Vec<_>>();
    let visible_rows = usize::from(area.height.saturating_sub(2));
    let max_scroll = u16::try_from(lines.len().saturating_sub(visible_rows)).unwrap_or(u16::MAX);
    app.set_detail_scroll_limit(max_scroll);
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .scroll((app.detail_scroll.min(max_scroll), 0)),
        area,
    );
}

/// Account and usage first, since they decide the choice; the home's identity follows.
fn detail_lines(app: &App, now: u64, best: Option<usize>) -> Vec<Detail> {
    let Some(row) = app.selected_row() else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    if app.selected.is_some()
        && app.selected == best
        && let Some(recommendation) = row.usage_snapshot_assessment_at(now).recommendation()
    {
        lines.push(key_value("Recommendation", recommendation.label));
    }
    if !app.static_configuration {
        match row.inspection() {
            Inspection::Loading => lines.push(styled(
                "Loading account and usage… You can launch now.",
                Style::default().fg(WARN),
            )),
            Inspection::Unavailable => lines.push(styled(
                "Inspection stopped before this home completed.",
                Style::default().fg(BAD),
            )),
            Inspection::Ready(details) => {
                lines.extend([
                    key_value("Account", details.account_label()),
                    key_value("Type", &details.account_type),
                    key_value("Plan", &details.plan),
                    key_value("Status", &details.status),
                ]);
                for bucket in &details.buckets {
                    lines.push(blank());
                    lines.push(styled(
                        &format!("{} usage", bucket.label()),
                        Style::default().fg(ACCENT).bold(),
                    ));
                    if bucket.plan != "-" && bucket.plan != details.plan {
                        lines.push(key_value("Plan", &bucket.plan));
                    }
                    if bucket.reached != "-" {
                        lines.push(key_value("Reached", &bucket.reached));
                    }
                    for (index, window) in bucket.windows.iter().enumerate() {
                        let role = bucket.window_role(index);
                        let assessment =
                            details.window_usage_snapshot_assessment_at(bucket, index, now);
                        let projection = assessment.projection();
                        // A generic window's role does not name its duration.
                        let usage = if role == WindowRole::Window {
                            window.usage_detail()
                        } else {
                            window.usage_amounts()
                        };
                        lines.push(snapshot_line(
                            "  ",
                            &format!("{role}: "),
                            &format!("{usage} · {}", window.reset_label_at(now)),
                            assessment.quota_is_stale(),
                            stale_usage_style(assessment.quota_is_stale()),
                        ));
                        lines.push(snapshot_line(
                            "    ",
                            "At current pace: ",
                            &projection.outcome_label(),
                            assessment.projection_is_stale(),
                            stale_projection_style(projection, assessment.projection_is_stale()),
                        ));
                    }
                }
                if let Some(sample_age) = details.usage_sample_age_label_at(now) {
                    lines.push(key_value(
                        "Usage sample",
                        &format!("{sample_age} · reopen to refresh"),
                    ));
                }
                if let Some(error) = &details.inspection_error {
                    lines.push(error_line("Inspection", error));
                }
                if let Some(error) = &details.usage_error {
                    lines.push(error_line("Usage", error));
                }
            }
        }
        lines.push(blank());
    }
    lines.extend([
        key_value("Name", row.display_name()),
        key_value("Path", row.display_path()),
        key_value("Current", if row.is_current() { "yes" } else { "no" }),
    ]);
    if let Some(details) = &row.configuration_details {
        lines.extend(details.iter().map(|(label, value)| key_value(label, value)));
    }
    if let Some(error) = &app.inspection_error {
        lines.push(error_line("Worker", error));
    }
    for warning in &app.discovery_warnings {
        lines.push(Detail {
            hang: "Discovery warning: ".width(),
            line: Line::styled(
                format!("Discovery warning: {warning}"),
                Style::default().fg(WARN),
            ),
        });
    }
    lines
}

fn detail_title(app: &App) -> &'static str {
    if app.focus == Focus::Details {
        "Selected home  [focused]"
    } else {
        "Selected home"
    }
}

pub(super) fn key_value(label: &str, value: &str) -> Detail {
    let label = format!("{label}: ");
    Detail {
        hang: label.width(),
        line: Line::from(vec![
            Span::styled(label, Style::default().fg(MUTED)),
            Span::raw(value.to_owned()),
        ]),
    }
}

fn styled(text: &str, style: Style) -> Detail {
    Detail {
        line: Line::styled(text.to_owned(), style),
        hang: 0,
    }
}

fn blank() -> Detail {
    styled("", Style::default())
}

fn error_line(label: &str, error: &str) -> Detail {
    let label = format!("{label} error: ");
    Detail {
        hang: label.width(),
        line: Line::styled(format!("{label}{error}"), Style::default().fg(BAD)),
    }
}

/// An indented `label: value` usage line that names a stale sample in text, not only color.
fn snapshot_line(indent: &str, label: &str, value: &str, stale: bool, style: Style) -> Detail {
    let prefix = format!("{indent}{}{label}", if stale { STALE_PREFIX } else { "" });
    Detail {
        hang: prefix.width(),
        line: Line::styled(format!("{prefix}{value}"), style),
    }
}

/// Word-wraps a detail to `width`, starting continuation rows at its hang
/// column (at most half the width) so wrapped values stay under their label.
pub(super) fn wrap(detail: Detail, width: usize) -> Vec<Line<'static>> {
    if width == 0 || detail.line.width() <= width {
        return vec![detail.line];
    }
    let hang = detail.hang.min(width / 2);
    let line_style = detail.line.style;
    let mut rows = Vec::new();
    let mut row = WrappedRow::new(0);
    for span in detail.line.spans {
        for word in span.content.split_inclusive(' ') {
            let mut word = word;
            while !word.is_empty() {
                if row.width + word.trim_end().width() <= width {
                    row.push(word, span.style);
                    break;
                }
                let fits_a_fresh_row = word.trim_end().width() <= width - hang;
                if row.has_text && (fits_a_fresh_row || row.width >= width) {
                    rows.push(row.finish().style(line_style));
                    row = WrappedRow::new(hang);
                    continue;
                }
                // The word is wider than a whole row; break it at this row's end.
                let (head, tail) = split_at_width(word, width.saturating_sub(row.width));
                if head.is_empty() && row.has_text {
                    rows.push(row.finish().style(line_style));
                    row = WrappedRow::new(hang);
                    continue;
                }
                let (head, tail) = if head.is_empty() {
                    // Not even one character fits an empty row; take it anyway.
                    word.split_at(word.chars().next().map_or(0, char::len_utf8))
                } else {
                    (head, tail)
                };
                row.push(head, span.style);
                rows.push(row.finish().style(line_style));
                row = WrappedRow::new(hang);
                word = tail;
            }
        }
    }
    if row.has_text {
        rows.push(row.finish().style(line_style));
    }
    rows
}

struct WrappedRow {
    spans: Vec<Span<'static>>,
    width: usize,
    has_text: bool,
}

impl WrappedRow {
    fn new(indent: usize) -> Self {
        Self {
            spans: if indent == 0 {
                Vec::new()
            } else {
                vec![Span::raw(" ".repeat(indent))]
            },
            width: indent,
            has_text: false,
        }
    }

    fn push(&mut self, text: &str, style: Style) {
        self.width += text.width();
        self.has_text = true;
        match self.spans.last_mut() {
            Some(last) if last.style == style => last.content.to_mut().push_str(text),
            _ => self.spans.push(Span::styled(text.to_owned(), style)),
        }
    }

    fn finish(mut self) -> Line<'static> {
        while let Some(last) = self.spans.last_mut() {
            let trimmed = last.content.trim_end().len();
            if trimmed > 0 {
                last.content.to_mut().truncate(trimmed);
                break;
            }
            self.spans.pop();
        }
        Line::from(self.spans)
    }
}

/// Splits off the longest prefix that fits `width`.
fn split_at_width(word: &str, width: usize) -> (&str, &str) {
    let mut used = 0;
    let mut end = 0;
    for (offset, character) in word.char_indices() {
        let character_width = character.width().unwrap_or(0);
        if used + character_width > width {
            break;
        }
        used += character_width;
        end = offset + character.len_utf8();
    }
    word.split_at(end)
}
