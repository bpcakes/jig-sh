use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Wrap},
};

use super::model::{App, Dashboard, Tab};

mod local;
mod responsive;
mod shell;

#[cfg(test)]
pub(crate) use responsive::{LayoutTier, layout_tier};

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;
const GOOD: Color = Color::Green;
const WARN: Color = Color::Yellow;
const BAD: Color = Color::Red;

pub(crate) fn draw(frame: &mut Frame, app: &App) {
    let area = jig_tui::ratatui_render_area(frame.area());
    if responsive::draw_if_responsive(frame, area, app) {
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(area);
    shell::draw_header(frame, chunks[0], app);
    shell::draw_tabs(frame, chunks[1], app);
    draw_content(frame, chunks[2], app);
    draw_footer(frame, chunks[3], app);
}

fn draw_content(frame: &mut Frame, area: Rect, app: &App) {
    if !app.domain_has_data(app.tab) {
        let message = app
            .local_domain()
            .error
            .unwrap_or("Collecting the first read-only snapshot...");
        frame.render_widget(
            Paragraph::new(message)
                .block(panel("Loading"))
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    if app.detail_is_open() {
        local::draw_detail(frame, area, app);
        return;
    }

    match app.tab {
        Tab::Status => draw_overview(frame, area, app),
        Tab::Work => local::draw_work(frame, area, app),
        Tab::Timeline => local::draw_timeline(frame, area, app),
        Tab::Health => local::draw_health(frame, area, app),
    }
}

fn draw_overview(frame: &mut Frame, area: Rect, app: &App) {
    let Some(status) = &app.status else {
        return;
    };
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[0]);
    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);

    frame.render_widget(
        Paragraph::new(repository_lines(status))
            .block(panel("Repository"))
            .wrap(Wrap { trim: true }),
        top[0],
    );
    frame.render_widget(
        Paragraph::new(work_lines(status))
            .block(panel("Work"))
            .wrap(Wrap { trim: true }),
        top[1],
    );
    frame.render_widget(
        Paragraph::new(loop_lines(status))
            .block(panel("Loops"))
            .wrap(Wrap { trim: true }),
        bottom[0],
    );
    frame.render_widget(
        Paragraph::new(error_lines(status))
            .block(panel("Collection"))
            .wrap(Wrap { trim: true }),
        bottom[1],
    );
}

fn repository_lines(status: &Dashboard) -> Vec<Line<'static>> {
    let repo = &status.repository;
    let branch = repo.branch.as_deref().unwrap_or(if repo.detached {
        "detached"
    } else {
        "<unknown>"
    });
    let revision = repo
        .head_revision
        .as_deref()
        .map(short_revision)
        .unwrap_or_else(|| "no HEAD".to_string());
    let dirty = repo
        .dirty
        .map_or("unknown", |dirty| if dirty { "dirty" } else { "clean" });
    let mut lines = vec![
        Line::from(format!("{} · default {}", repo.name, repo.default_branch)),
        Line::from(format!("{branch}@{revision} · {dirty}")),
    ];
    if let Some(upstream) = &repo.upstream {
        lines.push(Line::from(format!(
            "{} · ahead {} · behind {} · {}",
            upstream.reference, upstream.ahead, upstream.behind, upstream.state
        )));
        lines.push(Line::from(format!("Basis: {}", upstream.basis)));
    } else {
        lines.push(Line::from("Tracking: none"));
    }
    lines
}

fn work_lines(status: &Dashboard) -> Vec<Line<'static>> {
    vec![
        Line::from(format!("Open plans: {}", status.work.open_plans)),
        Line::from(format!(
            "Current session: {}",
            status.work.current_session_id.as_deref().unwrap_or("none")
        )),
        Line::from(format!(
            "Gate snapshots: {} · errors {}",
            status.work.gate_snapshots, status.work.gate_errors
        )),
    ]
}

fn loop_lines(status: &Dashboard) -> Vec<Line<'static>> {
    vec![
        Line::from(format!("Workflows: {}", status.loops.workflows)),
        Line::from(format!("Leases: {}", status.loops.leases)),
        Line::from(format!(
            "Attempts: {} · waiting {}",
            status.loops.attempts, status.loops.waiting_attempts
        )),
        Line::from(format!("Exhausted: {}", status.loops.exhausted_attempts)),
    ]
}

fn error_lines(status: &Dashboard) -> Vec<Line<'static>> {
    if status.errors.is_empty() {
        return vec![
            Line::from("All local observations completed."),
            Line::from(format!("Observed {}", age_label(status.observed_at_ms))),
        ];
    }
    status
        .errors
        .iter()
        .map(|error| {
            Line::from(format!(
                "{} [{}]: {}",
                error.scope, error.code, error.message
            ))
        })
        .collect()
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let text = if app.detail_is_open() {
        local::detail_footer(app)
    } else {
        match app.tab {
            Tab::Status => "q quit  Tab views  r refresh".to_string(),
            Tab::Work => "q quit  Tab views  j/k select  Enter detail  r refresh".to_string(),
            Tab::Timeline => {
                "q quit  Tab views  j/k select  Enter detail  f/F filter  +/- rows  r refresh"
                    .to_string()
            }
            Tab::Health => "q quit  Tab views  j/k select  Enter detail  r refresh".to_string(),
        }
    };
    frame.render_widget(Paragraph::new(text).style(Style::default().fg(MUTED)), area);
}

fn repository_label(status: &Dashboard) -> String {
    let repo = &status.repository;
    let branch = repo.branch.as_deref().unwrap_or(if repo.detached {
        "detached"
    } else {
        "<unknown>"
    });
    let revision = repo
        .head_revision
        .as_deref()
        .map(short_revision)
        .unwrap_or_else(|| "no-HEAD".to_owned());
    let clean = repo
        .dirty
        .map_or("unknown", |dirty| if dirty { "dirty" } else { "clean" });
    format!("{} {branch}@{revision} {clean}", repo.name)
}

fn recorder_repository_label(recorder: &super::model::LocalDashboard) -> String {
    let branch = recorder
        .repo
        .branch
        .as_deref()
        .unwrap_or(if recorder.repo.detached {
            "detached"
        } else {
            recorder.repo.default_branch.as_str()
        });
    let revision = recorder
        .repo
        .source_commit
        .as_deref()
        .map(short_revision)
        .unwrap_or_else(|| "no-HEAD".to_owned());
    format!(
        "{} {}@{} default {}",
        recorder.repo.name, branch, revision, recorder.repo.default_branch
    )
}

pub(super) fn age_label(observed_at_ms: u64) -> String {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0);
    age_label_at(observed_at_ms, now_ms)
}

pub(super) fn age_label_at(observed_at_ms: u64, now_ms: u64) -> String {
    if observed_at_ms == 0 || observed_at_ms > now_ms {
        return "unknown".to_owned();
    }
    let seconds = (now_ms - observed_at_ms) / 1_000;
    match seconds {
        0..=59 => format!("{seconds}s ago"),
        60..=3_599 => format!("{}m ago", seconds / 60),
        _ => format!("{}h ago", seconds / 3_600),
    }
}

fn short_revision(revision: &str) -> String {
    revision.chars().take(12).collect()
}

fn status_style(status: &str) -> Style {
    let color = match status {
        "complete" | "current" | "clean" | "in_sync" => GOOD,
        "dirty" | "ahead" | "partial" => WARN,
        "failed" | "behind" | "diverged" | "error" | "unavailable" => BAD,
        _ => MUTED,
    };
    Style::default().fg(color)
}

fn panel(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(MUTED))
        .title(title)
}
