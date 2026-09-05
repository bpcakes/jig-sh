use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Paragraph, Tabs},
};

use super::super::model::{App, Tab};
use super::{
    ACCENT, GOOD, MUTED, WARN, panel, provider_label, recorder_repository_label, repository_label,
    status_style,
};

pub(super) fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let (repo, provider, outcome) = if app.tab.is_status_domain() {
        match (&app.status.data, app.current_provider()) {
            (Some(dashboard), provider) => (
                repository_label(dashboard),
                provider_label(provider, app),
                dashboard.outcome.as_str(),
            ),
            (None, _) => (
                "waiting for status snapshot".to_owned(),
                "no provider yet".to_owned(),
                "loading",
            ),
        }
    } else {
        app.recorder.data.as_ref().map_or_else(
            || {
                (
                    "waiting for local snapshot".to_owned(),
                    "local recorder not loaded".to_owned(),
                    "loading",
                )
            },
            |recorder| {
                (
                    recorder_repository_label(recorder),
                    format!("local recorder epoch {}", recorder.epoch_id.get()),
                    if recorder.errors.is_empty() {
                        "current"
                    } else {
                        "partial"
                    },
                )
            },
        )
    };
    let domain = app.domain(app.tab);
    let (refresh, refresh_style) = if domain.refreshing {
        ("[refreshing]", Style::default().fg(WARN))
    } else if domain.error.is_some() && app.domain_has_data(app.tab) {
        ("[stale]", Style::default().fg(WARN))
    } else if domain.error.is_some() {
        ("[error]", Style::default().fg(Color::Red))
    } else {
        ("[ready]", Style::default().fg(GOOD))
    };
    let lines = vec![
        Line::from(vec![
            Span::styled(
                " Jig Status ",
                Style::default().fg(Color::Black).bg(ACCENT).bold(),
            ),
            Span::raw(" "),
            Span::styled(repo, Style::default().bold()),
            Span::raw("  "),
            Span::styled(format!("[{outcome}]"), status_style(outcome)),
        ]),
        Line::from(vec![
            Span::styled(format!(" {refresh}"), refresh_style),
            Span::raw("  "),
            Span::styled(provider, Style::default().fg(MUTED)),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

pub(super) fn draw_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let titles = Tab::ALL
        .into_iter()
        .map(|tab| Line::from(tab.title()))
        .collect::<Vec<_>>();
    let tabs = Tabs::new(titles)
        .select(app.tab.index())
        .block(panel("Views"))
        .highlight_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .divider("|");
    frame.render_widget(tabs, area);
}
