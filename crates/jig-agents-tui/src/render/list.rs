use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{List, ListItem, ListState, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use super::{
    View,
    layout::ListStyle,
    meter::{Bar, Look, meter, used_label},
    pad, panel, stale_projection_style, stale_snapshot_label, truncate,
};
use crate::model::{Focus, Inspection, WindowRole};

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
/// Selection mark, best badge, current badge, and a space.
const PREFIX_WIDTH: usize = 4;
const GAP: usize = 2;
const MIN_METER: usize = 8;
const MAX_METER: usize = 24;

/// One home's list content, shared by every list style.
struct HomeLine {
    selected: bool,
    best: bool,
    current: bool,
    name: String,
    account: String,
    windows: Vec<WindowLine>,
    /// The home's overall outcome: its worst window, or its inspection state.
    outcome: (String, Style),
}

struct WindowLine {
    label: String,
    gauge: crate::model::Gauge,
    stale: bool,
    outcome: (String, Style),
}

impl HomeLine {
    fn at(view: &View<'_>, index: usize) -> Self {
        let app = view.app;
        let row = &app.rows[index];
        let assessment = row.usage_snapshot_assessment_at(view.now);
        let projection = assessment.projection();
        let windows = match row.inspection() {
            Inspection::Ready(details)
                if details.inspection_error.is_none() && details.usage_error.is_none() =>
            {
                details
                    .primary_windows_at(view.now)
                    .into_iter()
                    .take(2)
                    .map(|window| {
                        let projection = window.assessment.projection();
                        // Say "stale" in text, not only in muted color.
                        let stale = window.assessment.projection_is_stale()
                            || window.assessment.quota_is_stale();
                        WindowLine {
                            label: short_role(window.role),
                            gauge: window.gauge,
                            stale: window.assessment.quota_is_stale(),
                            outcome: (
                                stale_snapshot_label(projection.outcome_label(), stale),
                                stale_projection_style(view.theme, projection, stale),
                            ),
                        }
                    })
                    .collect()
            }
            _ => Vec::new(),
        };
        let outcome = match row.inspection() {
            Inspection::Loading => (
                format!("{} inspecting…", SPINNER[app.tick % SPINNER.len()]),
                Style::default().fg(view.theme.muted()),
            ),
            _ => {
                let stale = assessment.projection_is_stale() || assessment.quota_is_stale();
                (
                    stale_snapshot_label(projection.short_label(short_role), stale),
                    stale_projection_style(view.theme, projection, stale),
                )
            }
        };
        Self {
            selected: app.selected == Some(index),
            best: view.best == Some(index),
            current: row.is_current(),
            name: row.display_name().to_owned(),
            account: row.account(),
            windows,
            outcome,
        }
    }

    fn prefix(&self, view: &View<'_>) -> Vec<Span<'static>> {
        let theme = view.theme;
        vec![
            Span::styled(
                if self.selected { "›" } else { " " },
                Style::default()
                    .fg(theme.accent())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if self.best { "◆" } else { " " },
                Style::default().fg(theme.best()),
            ),
            Span::styled(
                if self.current { "●" } else { " " },
                Style::default().fg(theme.current()),
            ),
            Span::raw(" "),
        ]
    }

    fn text_style(&self, view: &View<'_>) -> Style {
        if self.selected {
            Style::default()
                .fg(view.theme.bright())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        }
    }

    fn secondary_style(&self, view: &View<'_>) -> Style {
        if self.selected {
            Style::default().fg(view.theme.bright())
        } else {
            Style::default().fg(view.theme.muted())
        }
    }
}

pub(super) fn draw_list(frame: &mut Frame, area: Rect, view: &View<'_>) {
    let app = view.app;
    let visible = app.visible_indices();
    let focused = app.focus == Focus::Homes;
    let style = ListStyle::for_width(area.width);
    let legend = if app.static_configuration {
        " ● current "
    } else if view.best.is_some() {
        " ◆ best pace  ● current "
    } else {
        " ● current · no rankable projection "
    };
    let block = panel(view, "homes", &visible.len().to_string(), focused).title(
        Line::from(Span::styled(
            legend,
            Style::default().fg(view.theme.muted()),
        ))
        .right_aligned(),
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if visible.is_empty() {
        frame.render_widget(
            Paragraph::new("No homes match the current search.").wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }
    let width = usize::from(inner.width);
    let homes = visible
        .iter()
        .map(|index| HomeLine::at(view, *index))
        .collect::<Vec<_>>();
    let (header, items): (Line<'static>, Vec<ListItem<'static>>) = if app.static_configuration {
        configuration_rows(view, &visible, width)
    } else {
        match style {
            ListStyle::Compact => compact_rows(view, &homes, width),
            ListStyle::TwoLine => two_line_rows(view, &homes, width),
            ListStyle::Full => full_rows(view, &homes, width),
        }
    };
    let [header_area, list_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(inner);
    frame.render_widget(
        Paragraph::new(header.style(Style::default().fg(view.theme.muted()).bold())),
        header_area,
    );
    let mut state = ListState::default()
        .with_offset(app.list_offset_for_viewport(list_area.height))
        .with_selected(
            app.selected
                .and_then(|selected| visible.iter().position(|index| *index == selected)),
        );
    frame.render_stateful_widget(
        List::new(items).highlight_style(view.theme.selection()),
        list_area,
        &mut state,
    );
    app.set_list_offset(state.offset());
}

/// One line per home: name, account, both windows, and the overall outcome.
fn full_rows(
    view: &View<'_>,
    homes: &[HomeLine],
    width: usize,
) -> (Line<'static>, Vec<ListItem<'static>>) {
    let name_width = column(homes, "home", |home| &home.name).min(24);
    let account_width = column(homes, "account", |home| &home.account).min(26);
    let label_width = label_width(homes);
    let outcome_width = column(homes, "at current pace", |home| &home.outcome.0).min(28);
    let fixed = PREFIX_WIDTH + name_width + GAP + account_width + GAP + outcome_width;
    // Each window: label, space, meter, space, percentage, gap.
    let meter_width = (width.saturating_sub(fixed) / 2)
        .saturating_sub(label_width + 1 + 1 + 4 + GAP)
        .clamp(MIN_METER, MAX_METER);
    let window_width = label_width + 1 + meter_width + 1 + 4;
    let header = Line::from(vec![
        Span::raw(" ".repeat(PREFIX_WIDTH)),
        Span::raw(pad("home", name_width + GAP)),
        Span::raw(pad("account", account_width + GAP)),
        Span::raw(pad("usage", (window_width + GAP) * 2)),
        Span::raw("at current pace"),
    ]);
    let items = homes
        .iter()
        .map(|home| {
            let mut spans = home.prefix(view);
            spans.push(Span::styled(
                pad(&truncate(&home.name, name_width), name_width + GAP),
                home.text_style(view),
            ));
            spans.push(Span::styled(
                pad(&truncate(&home.account, account_width), account_width + GAP),
                home.secondary_style(view),
            ));
            if home.windows.is_empty() {
                spans.push(Span::styled(home.outcome.0.clone(), home.outcome.1));
            } else {
                for slot in 0..2 {
                    match home.windows.get(slot) {
                        Some(window) => {
                            spans.extend(window_spans(
                                view,
                                home,
                                window,
                                label_width,
                                meter_width,
                            ));
                            spans.push(Span::raw(" ".repeat(GAP)));
                        }
                        None => spans.push(Span::raw(" ".repeat(window_width + GAP))),
                    }
                }
                spans.push(Span::styled(home.outcome.0.clone(), home.outcome.1));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    (header, items)
}

/// Two lines per home: name and account beside one window each, with that
/// window's outcome.
fn two_line_rows(
    view: &View<'_>,
    homes: &[HomeLine],
    width: usize,
) -> (Line<'static>, Vec<ListItem<'static>>) {
    let identity_width = homes
        .iter()
        .flat_map(|home| [home.name.width(), home.account.width()])
        .chain(["home / account".width()])
        .max()
        .unwrap_or(0)
        .min(30);
    let label_width = label_width(homes);
    let outcome_width = homes
        .iter()
        .flat_map(|home| home.windows.iter().map(|window| window.outcome.0.width()))
        .max()
        .unwrap_or(0)
        .min(26);
    let meter_width = width
        .saturating_sub(
            PREFIX_WIDTH + identity_width + GAP + label_width + 1 + 1 + 4 + GAP + outcome_width,
        )
        .clamp(MIN_METER, MAX_METER);
    let header = Line::from(vec![
        Span::raw(" ".repeat(PREFIX_WIDTH)),
        Span::raw(pad("home / account", identity_width + GAP)),
        Span::raw("usage · at current pace"),
    ]);
    let items = homes
        .iter()
        .map(|home| {
            let mut first = home.prefix(view);
            first.push(Span::styled(
                pad(&truncate(&home.name, identity_width), identity_width + GAP),
                home.text_style(view),
            ));
            let mut second = vec![Span::raw(" ".repeat(PREFIX_WIDTH))];
            second.push(Span::styled(
                pad(
                    &truncate(&home.account, identity_width),
                    identity_width + GAP,
                ),
                home.secondary_style(view),
            ));
            if home.windows.is_empty() {
                first.push(Span::styled(home.outcome.0.clone(), home.outcome.1));
            }
            for (line, window) in [&mut first, &mut second].into_iter().zip(&home.windows) {
                line.extend(window_spans(view, home, window, label_width, meter_width));
                line.push(Span::raw(" ".repeat(GAP)));
                line.push(Span::styled(window.outcome.0.clone(), window.outcome.1));
            }
            ListItem::new(Text::from(vec![Line::from(first), Line::from(second)]))
        })
        .collect();
    (header, items)
}

/// Two lines per home: name and account, then the overall outcome.
fn compact_rows(
    view: &View<'_>,
    homes: &[HomeLine],
    width: usize,
) -> (Line<'static>, Vec<ListItem<'static>>) {
    let header = Line::from(vec![
        Span::raw(" ".repeat(PREFIX_WIDTH)),
        Span::raw("home · account / projection"),
    ]);
    let text_width = width.saturating_sub(PREFIX_WIDTH);
    let items = homes
        .iter()
        .map(|home| {
            let mut first = home.prefix(view);
            first.push(Span::styled(
                truncate(&format!("{} · {}", home.name, home.account), text_width),
                home.text_style(view),
            ));
            let mut second = vec![Span::raw(" ".repeat(PREFIX_WIDTH))];
            // The busiest window's meter, when it fits beside the outcome.
            let busiest = home.windows.iter().max_by(|a, b| {
                a.gauge
                    .used
                    .unwrap_or(0.0)
                    .total_cmp(&b.gauge.used.unwrap_or(0.0))
            });
            let label_width = label_width(homes);
            let outcome_width = home.outcome.0.width();
            let meter_width = text_width
                .saturating_sub(label_width + 1 + 1 + 4 + GAP + outcome_width)
                .min(12);
            match busiest {
                Some(window) if meter_width >= MIN_METER => {
                    second.extend(window_spans(view, home, window, label_width, meter_width));
                    second.push(Span::raw(" ".repeat(GAP)));
                    second.push(Span::styled(home.outcome.0.clone(), home.outcome.1));
                }
                _ => second.push(Span::styled(
                    truncate(&home.outcome.0, text_width),
                    home.outcome.1,
                )),
            }
            ListItem::new(Text::from(vec![Line::from(first), Line::from(second)]))
        })
        .collect();
    (header, items)
}

/// Static configurations: name over path, without inspection.
fn configuration_rows(
    view: &View<'_>,
    visible: &[usize],
    width: usize,
) -> (Line<'static>, Vec<ListItem<'static>>) {
    let header = Line::from(vec![
        Span::raw(" ".repeat(PREFIX_WIDTH)),
        Span::raw("home / path"),
    ]);
    let text_width = width.saturating_sub(PREFIX_WIDTH);
    let items = visible
        .iter()
        .map(|index| {
            let row = &view.app.rows[*index];
            let home = HomeLine {
                selected: view.app.selected == Some(*index),
                best: false,
                current: row.is_current(),
                name: row.display_name().to_owned(),
                account: String::new(),
                windows: Vec::new(),
                outcome: (String::new(), Style::default()),
            };
            let mut first = home.prefix(view);
            first.push(Span::styled(
                truncate(&home.name, text_width),
                home.text_style(view),
            ));
            let second = vec![
                Span::raw(" ".repeat(PREFIX_WIDTH)),
                Span::styled(
                    truncate(row.display_path(), text_width),
                    home.secondary_style(view),
                ),
            ];
            ListItem::new(Text::from(vec![Line::from(first), Line::from(second)]))
        })
        .collect();
    (header, items)
}

/// `5h ▕███▌──│───▏ 42%`: the window label, its meter, and used quota.
fn window_spans(
    view: &View<'_>,
    home: &HomeLine,
    window: &WindowLine,
    label_width: usize,
    meter_width: usize,
) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        pad(&window.label, label_width + 1),
        home.secondary_style(view),
    )];
    spans.extend(meter(
        view.theme,
        window.gauge,
        meter_width,
        Look {
            bar: Bar::Line,
            stale: window.stale,
            selected: home.selected,
        },
    ));
    spans.push(Span::styled(
        format!(" {}", used_label(window.gauge)),
        home.text_style(view),
    ));
    spans
}

fn short_role(role: WindowRole) -> String {
    match role {
        WindowRole::Weekly => "wk".to_owned(),
        role => role.to_string(),
    }
}

fn label_width(homes: &[HomeLine]) -> usize {
    homes
        .iter()
        .flat_map(|home| home.windows.iter().map(|window| window.label.width()))
        .max()
        .unwrap_or(2)
}

fn column(homes: &[HomeLine], header: &str, text: impl Fn(&HomeLine) -> &String) -> usize {
    homes
        .iter()
        .map(|home| text(home).width())
        .chain([header.width()])
        .max()
        .unwrap_or(0)
}
