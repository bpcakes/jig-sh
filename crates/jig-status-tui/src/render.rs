use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState, Tabs,
        Wrap,
    },
};

use crate::model::{
    App, BlockerItemView, CategoryCounts, FacetView, InputFreshnessView, PackageView, ProviderView,
    SourceView, Tab,
};

pub(crate) const MIN_WIDTH: u16 = 72;
pub(crate) const MIN_HEIGHT: u16 = 20;

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;
const GOOD: Color = Color::Green;
const WARN: Color = Color::Yellow;
const BAD: Color = Color::Red;

pub(crate) fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        draw_too_small(frame, area);
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
    draw_header(frame, chunks[0], app);
    draw_tabs(frame, chunks[1], app);
    draw_content(frame, chunks[2], app);
    draw_footer(frame, chunks[3], app);
}

fn draw_too_small(frame: &mut Frame, area: Rect) {
    let text = format!(
        "Terminal too small: {}x{}.\nJig Status needs at least {MIN_WIDTH}x{MIN_HEIGHT}.\nResize the terminal, or press q to quit.",
        area.width, area.height
    );
    frame.render_widget(
        Paragraph::new(text)
            .block(panel("Jig Status"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let (repo, provider, outcome) = match (&app.dashboard, app.current_provider()) {
        (Some(dashboard), provider) => (
            repository_label(dashboard),
            provider_label(provider, app),
            dashboard.outcome.as_str(),
        ),
        (None, _) => (
            "waiting for first snapshot".to_owned(),
            "no provider yet".to_owned(),
            "loading",
        ),
    };
    let refresh = if app.refreshing {
        "[refreshing]"
    } else {
        "[ready]"
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
            Span::styled(
                format!(" {refresh}"),
                if app.refreshing {
                    Style::default().fg(WARN)
                } else {
                    Style::default().fg(GOOD)
                },
            ),
            Span::raw("  "),
            Span::styled(provider, Style::default().fg(MUTED)),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_tabs(frame: &mut Frame, area: Rect, app: &App) {
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

fn draw_content(frame: &mut Frame, area: Rect, app: &App) {
    if app.dashboard.is_none() {
        let message = app
            .last_error
            .as_deref()
            .unwrap_or("Collecting the first read-only status snapshot...");
        frame.render_widget(
            Paragraph::new(message)
                .block(panel("Loading"))
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }

    match app.tab {
        Tab::Overview => draw_overview(frame, area, app),
        Tab::Packages => draw_packages(frame, area, app),
        Tab::Blockers => draw_blockers(frame, area, app),
    }
}

fn draw_overview(frame: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(57), Constraint::Percentage(43)])
        .split(rows[0]);
    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(57), Constraint::Percentage(43)])
        .split(rows[1]);

    frame.render_widget(
        Paragraph::new(progress_lines(app.current_provider()))
            .block(panel("Rewrite progress"))
            .wrap(Wrap { trim: true }),
        top[0],
    );
    frame.render_widget(
        Paragraph::new(repository_and_work_lines(app))
            .block(panel("Repository and Jig"))
            .wrap(Wrap { trim: true }),
        top[1],
    );
    frame.render_widget(
        Paragraph::new(freshness_lines(app.current_provider()))
            .block(panel("Input freshness"))
            .wrap(Wrap { trim: true }),
        bottom[0],
    );
    frame.render_widget(
        Paragraph::new(issue_lines(app))
            .block(panel("Diagnostics"))
            .wrap(Wrap { trim: true }),
        bottom[1],
    );
}

fn progress_lines(provider: Option<&ProviderView>) -> Vec<Line<'static>> {
    let Some(provider) = provider else {
        return vec![Line::from("No status providers are configured.")];
    };
    let summary = &provider.summary;
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{} packages", summary.work_packages),
                Style::default().bold(),
            ),
            Span::raw(format!(
                "  {} blockers across {} packages",
                summary.blockers, summary.work_packages_with_blockers
            )),
        ]),
        counts_line("Specification", &summary.specification),
        counts_line("Implementation", &summary.implementation),
        counts_line("Verification", &summary.verification),
        counts_line("Acceptance", &summary.acceptance),
        Line::from(format!(
            "Checks: {} total  Diagnostics: {} ({} info, {} warning, {} error)",
            summary.acceptance_checks,
            summary.diagnostics.total,
            summary.diagnostics.info,
            summary.diagnostics.warning,
            summary.diagnostics.error
        )),
    ];
    if let Some(version) = &provider.adapter_version {
        lines.push(Line::from(format!("Adapter version: {version}")));
    }
    lines
}

fn counts_line(label: &str, counts: &CategoryCounts) -> Line<'static> {
    let mut spans = vec![Span::styled(format!("{label}: "), Style::default().bold())];
    for (name, count) in [
        ("complete", counts.complete),
        ("active", counts.active),
        ("ready", counts.ready),
        ("pending", counts.pending),
        ("blocked", counts.blocked),
        ("failed", counts.failed),
        ("unknown", counts.unknown),
    ] {
        if count > 0 {
            spans.push(Span::styled(
                format!("{count} {name}  "),
                status_style(name),
            ));
        }
    }
    if counts.total() == 0 {
        spans.push(Span::styled("none", Style::default().fg(MUTED)));
    }
    Line::from(spans)
}

fn repository_and_work_lines(app: &App) -> Vec<Line<'static>> {
    let Some(dashboard) = &app.dashboard else {
        return Vec::new();
    };
    let repo = &dashboard.repository;
    let revision = repo
        .head_revision
        .as_deref()
        .map(short_revision)
        .unwrap_or_else(|| "no HEAD".to_owned());
    let branch = repo.branch.as_deref().unwrap_or(if repo.detached {
        "detached"
    } else {
        repo.default_branch.as_str()
    });
    let cleanliness = match repo.dirty {
        Some(true) => "dirty",
        Some(false) => "clean",
        None => "unknown",
    };
    let tracking = repo.upstream.as_ref().map_or_else(
        || "Tracking: none (no remote fetch is performed)".to_owned(),
        |upstream| {
            format!(
                "Tracking: {} [{}], ahead {}, behind {} ({})",
                upstream.reference, upstream.state, upstream.ahead, upstream.behind, upstream.basis
            )
        },
    );
    let session = dashboard
        .work
        .current_session_id
        .as_deref()
        .unwrap_or("none");
    vec![
        Line::from(format!("{branch}@{revision} [{cleanliness}]")),
        Line::from(tracking),
        Line::from(format!(
            "Work: {} open plans, session {session}",
            dashboard.work.open_plans
        )),
        Line::from(format!(
            "Gates: {} snapshots, {} collection errors",
            dashboard.work.gate_snapshots, dashboard.work.gate_errors
        )),
        Line::from(format!(
            "Loops: {} leases, {} attempts, {} exhausted",
            dashboard.loops.leases, dashboard.loops.attempts, dashboard.loops.exhausted_attempts
        )),
        Line::from(format!("Observed: {}", age_label(dashboard.observed_at_ms))),
    ]
}

fn freshness_lines(provider: Option<&ProviderView>) -> Vec<Line<'static>> {
    let Some(provider) = provider else {
        return vec![Line::from("No provider inputs to compare.")];
    };
    if provider.input_freshness.is_empty() {
        return vec![Line::from("Provider reported no comparable inputs.")];
    }

    provider
        .input_freshness
        .iter()
        .flat_map(input_lines)
        .collect()
}

fn input_lines(input: &InputFreshnessView) -> Vec<Line<'static>> {
    let location = input.path.as_deref().unwrap_or(".");
    let dirty = match input.dirty {
        Some(true) => ", dirty",
        Some(false) => ", clean",
        None => "",
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(format!("{} ", input.name), Style::default().bold()),
        Span::styled(format!("[{}]", input.status), status_style(&input.status)),
        Span::raw(format!("  {} at {location}{dirty}", input.kind)),
    ])];
    let expected = input
        .expected_revision
        .as_deref()
        .map(short_revision)
        .unwrap_or_else(|| "-".to_owned());
    let observed = input
        .observed_revision
        .as_deref()
        .map(short_revision)
        .unwrap_or_else(|| "-".to_owned());
    lines.push(Line::from(format!(
        "  expected {expected}  observed {observed}"
    )));
    if let Some(reason) = &input.reason {
        lines.push(Line::from(format!("  {reason}")));
    }
    lines
}

fn issue_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(error) = &app.last_error {
        lines.push(Line::from(vec![
            Span::styled("Refresh error: ", Style::default().fg(BAD).bold()),
            Span::raw(error.clone()),
        ]));
    }
    if let Some(provider) = app.current_provider() {
        if let Some(error) = &provider.error {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("Provider {}: ", error.code),
                    Style::default().fg(BAD).bold(),
                ),
                Span::raw(error.message.clone()),
            ]));
            if let Some(stderr) = &error.stderr {
                let suffix = if error.stderr_truncated {
                    " [truncated]"
                } else {
                    ""
                };
                lines.push(Line::from(format!("stderr: {stderr}{suffix}")));
            }
        }
        for diagnostic in &provider.diagnostics {
            let package = diagnostic
                .work_package
                .as_deref()
                .map(|id| format!(" {id}"))
                .unwrap_or_default();
            let source = source_suffix(diagnostic.source.as_ref());
            lines.push(Line::from(vec![
                Span::styled(
                    format!("[{}] {}{package}: ", diagnostic.level, diagnostic.code),
                    status_style(&diagnostic.level),
                ),
                Span::raw(format!("{}{}", diagnostic.message, source)),
            ]));
        }
    }
    if let Some(dashboard) = &app.dashboard {
        for error in &dashboard.errors {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{} / {}: ", error.scope, error.code),
                    Style::default().fg(BAD),
                ),
                Span::raw(error.message.clone()),
            ]));
        }
    }
    if lines.is_empty() {
        lines.push(Line::from("No provider diagnostics or collection errors."));
    }
    lines
}

fn draw_packages(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(area);
    let packages = app.package_rows();
    if packages.is_empty() {
        let message = if app.current_provider().is_none() {
            "No status provider is selected."
        } else if app.blocked_only {
            "No packages with blockers."
        } else {
            "The selected provider reported no work packages."
        };
        frame.render_widget(Paragraph::new(message).block(panel("Packages")), chunks[0]);
    } else {
        draw_package_table(frame, chunks[0], app, &packages);
    }
    draw_package_detail(frame, chunks[1], app.selected_package());
}

fn draw_package_table(frame: &mut Frame, area: Rect, app: &App, packages: &[&PackageView]) {
    let compact = area.width < 108;
    let rows = packages.iter().map(|package| {
        let mut cells = vec![
            Cell::from(package.id.clone()),
            Cell::from(package.title.clone()),
            Cell::from(package.specification.state.clone())
                .style(status_style(&package.specification.category)),
            Cell::from(package.implementation.state.clone())
                .style(status_style(&package.implementation.category)),
            Cell::from(package.verification.state.clone())
                .style(status_style(&package.verification.category)),
        ];
        if !compact {
            cells.push(Cell::from(format!(
                "{}/{}",
                package.acceptance_complete, package.acceptance_total
            )));
        }
        cells.push(Cell::from(package.blockers.len().to_string()).style(
            if package.blockers.is_empty() {
                Style::default().fg(MUTED)
            } else {
                Style::default().fg(BAD).bold()
            },
        ));
        Row::new(cells)
    });
    let mut headings = vec!["ID", "Title", "Spec", "Implementation", "Verification"];
    let constraints = if compact {
        headings.push("Blk");
        vec![
            Constraint::Length(14),
            Constraint::Min(18),
            Constraint::Length(11),
            Constraint::Length(13),
            Constraint::Length(12),
            Constraint::Length(4),
        ]
    } else {
        headings.extend(["Checks", "Blk"]);
        vec![
            Constraint::Length(16),
            Constraint::Min(24),
            Constraint::Length(14),
            Constraint::Length(16),
            Constraint::Length(15),
            Constraint::Length(9),
            Constraint::Length(4),
        ]
    };
    let filter = if app.blocked_only {
        "blocked only"
    } else {
        "all"
    };
    let title = format!("Packages ({filter})");
    let table = Table::new(rows, constraints)
        .header(
            Row::new(headings)
                .style(Style::default().fg(ACCENT).bold())
                .bottom_margin(1),
        )
        .block(panel(&title))
        .highlight_style(Style::default().bg(Color::DarkGray).bold())
        .highlight_symbol("> ");
    let mut state = TableState::default();
    state.select(Some(app.package_index));
    frame.render_stateful_widget(table, area, &mut state);
}

fn draw_package_detail(frame: &mut Frame, area: Rect, package: Option<&PackageView>) {
    let Some(package) = package else {
        frame.render_widget(
            Paragraph::new("Select a package to inspect its facets and evidence.")
                .block(panel("Package detail")),
            area,
        );
        return;
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{}  ", package.id),
                Style::default().fg(ACCENT).bold(),
            ),
            Span::styled(package.title.clone(), Style::default().bold()),
        ]),
        facet_line("Specification", &package.specification),
        facet_line("Implementation", &package.implementation),
        facet_line("Verification", &package.verification),
        Line::from(format!(
            "Acceptance: {}/{} complete",
            package.acceptance_complete, package.acceptance_total
        )),
    ];
    if !package.dependencies.is_empty() {
        lines.push(Line::from(format!(
            "Dependencies: {}",
            package.dependencies.join(", ")
        )));
    }
    for blocker in &package.blockers {
        lines.push(Line::from(vec![
            Span::styled(
                format!("BLOCKER {}: ", blocker.code),
                Style::default().fg(BAD).bold(),
            ),
            Span::raw(format!(
                "{}{}{}",
                blocker.message,
                blocker
                    .related_work_package
                    .as_deref()
                    .map(|id| format!(" (related {id})"))
                    .unwrap_or_default(),
                source_suffix(blocker.source.as_ref())
            )),
        ]));
    }
    for evidence in package.evidence.iter().take(4) {
        lines.push(Line::from(format!(
            "Evidence [{}]: {}{}",
            evidence.kind,
            evidence.reference,
            source_suffix(evidence.source.as_ref())
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel("Package detail"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_blockers(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);
    let blockers = app
        .current_provider()
        .map(|provider| provider.blockers.as_slice())
        .unwrap_or(&[]);
    if blockers.is_empty() {
        frame.render_widget(
            Paragraph::new("No package blockers were reported.")
                .block(panel("Blocker queue"))
                .wrap(Wrap { trim: true }),
            chunks[0],
        );
    } else {
        let items = blockers
            .iter()
            .map(|item| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{} ", item.package_id), Style::default().fg(ACCENT)),
                    Span::styled(
                        format!("[{}] ", item.blocker.code),
                        Style::default().fg(BAD),
                    ),
                    Span::raw(item.blocker.message.clone()),
                ]))
            })
            .collect::<Vec<_>>();
        let title = format!("Blocker queue ({})", blockers.len());
        let list = List::new(items)
            .block(panel(&title))
            .highlight_style(Style::default().bg(Color::DarkGray).bold())
            .highlight_symbol("> ");
        let mut state = ListState::default();
        state.select(Some(app.blocker_index));
        frame.render_stateful_widget(list, chunks[0], &mut state);
    }
    draw_blocker_detail(frame, chunks[1], app.selected_blocker());
}

fn draw_blocker_detail(frame: &mut Frame, area: Rect, item: Option<&BlockerItemView>) {
    let Some(item) = item else {
        frame.render_widget(
            Paragraph::new("Select a blocker to inspect it.").block(panel("Blocker detail")),
            area,
        );
        return;
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{}  ", item.package_id),
                Style::default().fg(ACCENT).bold(),
            ),
            Span::styled(item.package_title.clone(), Style::default().bold()),
        ]),
        Line::from(vec![
            Span::styled(
                format!("[{}] ", item.blocker.code),
                Style::default().fg(BAD).bold(),
            ),
            Span::raw(item.blocker.message.clone()),
        ]),
        facet_line("Specification", &item.specification),
        facet_line("Implementation", &item.implementation),
        facet_line("Verification", &item.verification),
    ];
    if let Some(related) = &item.blocker.related_work_package {
        lines.push(Line::from(format!("Related work package: {related}")));
    }
    if let Some(source) = &item.blocker.source {
        lines.push(Line::from(format!("Source: {}", source.display())));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel("Blocker detail"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn facet_line(label: &str, facet: &FacetView) -> Line<'static> {
    let summary = facet
        .summary
        .as_deref()
        .map(|summary| format!(" - {summary}"))
        .unwrap_or_default();
    let source = source_suffix(facet.source.as_ref());
    Line::from(vec![
        Span::styled(format!("{label}: "), Style::default().bold()),
        Span::styled(
            format!("{} [{}]", facet.state, facet.category),
            status_style(&facet.category),
        ),
        Span::raw(format!("{summary}{source}")),
    ])
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let selection_help = match app.tab {
        Tab::Overview => "",
        Tab::Packages => "  j/k move  b blocked-only",
        Tab::Blockers => "  j/k move",
    };
    let first = Line::from(vec![
        Span::styled(" q/Esc quit ", Style::default().fg(Color::Black).bg(ACCENT)),
        Span::raw(format!(
            "  r refresh  Tab views  [/] provider{selection_help}"
        )),
    ]);
    let second = if let Some(error) = &app.last_error {
        Line::from(vec![
            Span::styled("Last refresh error: ", Style::default().fg(BAD)),
            Span::raw(error.clone()),
        ])
    } else if app.refresh_queued {
        Line::from("Refresh queued; the active collection will finish or cancel first.")
    } else {
        Line::from("Read-only: no remote fetch, cache, receipt, or implementation launch.")
            .style(Style::default().fg(MUTED))
    };
    frame.render_widget(Paragraph::new(vec![first, second]), area);
}

fn repository_label(dashboard: &crate::model::Dashboard) -> String {
    let repo = &dashboard.repository;
    let branch = repo.branch.as_deref().unwrap_or(if repo.detached {
        "detached"
    } else {
        repo.default_branch.as_str()
    });
    let revision = repo
        .head_revision
        .as_deref()
        .map(short_revision)
        .unwrap_or_else(|| "no-HEAD".to_owned());
    let clean = match repo.dirty {
        Some(true) => "dirty",
        Some(false) => "clean",
        None => "unknown",
    };
    format!("{} {branch}@{revision} {clean}", repo.name)
}

fn provider_label(provider: Option<&ProviderView>, app: &App) -> String {
    let Some(provider) = provider else {
        return "no providers configured".to_owned();
    };
    let total = app
        .dashboard
        .as_ref()
        .map(|dashboard| dashboard.providers.len())
        .unwrap_or(0);
    let name = provider.display_name.as_deref().unwrap_or(&provider.id);
    format!(
        "provider {}/{}: {} [{} in {}]",
        app.provider_index + 1,
        total,
        name,
        provider.status,
        duration_label(provider.duration_ms)
    )
}

fn duration_label(duration_ms: u64) -> String {
    if duration_ms < 1_000 {
        format!("{duration_ms}ms")
    } else {
        format!("{:.1}s", duration_ms as f64 / 1_000.0)
    }
}

fn age_label(observed_at_ms: u64) -> String {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0);
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

fn source_suffix(source: Option<&SourceView>) -> String {
    source
        .map(|source| format!(" @ {}", source.display()))
        .unwrap_or_default()
}

fn status_style(status: &str) -> Style {
    let color = match status {
        "complete" | "current" | "clean" | "in_sync" | "info" => GOOD,
        "ready" | "active" | "dirty" | "ahead" | "warning" | "partial" => WARN,
        "blocked" | "failed" | "stale" | "behind" | "diverged" | "error" | "unavailable"
        | "timed_out" | "cancelled" => BAD,
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
