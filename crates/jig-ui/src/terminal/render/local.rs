use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{List, ListItem, ListState, Paragraph, Wrap},
};

use super::{ACCENT, BAD, panel};
use crate::terminal::model::{
    App, BaseDetail, DetailDocument, PlanDetailView, PlanSection, WorkState,
};

pub(super) fn draw_work(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Percentage(40),
        Constraint::Percentage(35),
        Constraint::Percentage(25),
    ])
    .split(area);
    let Some(local) = app.recorder.data.as_ref() else {
        return;
    };
    let open_len = local
        .work
        .iter()
        .take_while(|plan| plan.state == WorkState::Open)
        .count();
    let open_items = local.work[..open_len]
        .iter()
        .map(work_item)
        .collect::<Vec<_>>();
    draw_list_selection(
        frame,
        chunks[0],
        format!(
            "Open work · schema {} · {} sessions / {} open / {} decisions · {}",
            local.schema_version,
            local.counts.sessions,
            local.counts.open_plans,
            local.counts.decisions,
            local.limits.open_plans.label("open plans")
        ),
        open_items,
        (app.work_index < open_len).then_some(app.work_index),
    );
    let mut lines = app
        .selected_work()
        .map_or_else(|| vec![Line::from("No plan is selected.")], work_preview);
    lines.extend([
        Line::from(format!(
            "Repository: {} · source {}",
            local.repo.name,
            local.repo.source_path.as_deref().unwrap_or("—")
        )),
        Line::from(format!(
            "Runtime: {} · contract {} · generated {} · session {}",
            local.harness.runtime_version,
            local.harness.contract_version,
            crate::terminal::model::format_timestamp(Some(local.generated_at_ms)),
            local.current_session_id.as_deref().unwrap_or("none")
        )),
    ]);
    append_local_notices(&mut lines, app);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel("Plan preview · Enter for detail"))
            .wrap(Wrap { trim: true }),
        chunks[1],
    );
    let history_items = local.work[open_len..]
        .iter()
        .map(work_item)
        .collect::<Vec<_>>();
    draw_list_selection(
        frame,
        chunks[2],
        format!(
            "Recently completed · {}",
            local.limits.history.label("history rows")
        ),
        history_items,
        (app.work_index >= open_len).then_some(app.work_index.saturating_sub(open_len)),
    );
}

pub(super) fn draw_compact_work(frame: &mut Frame, area: Rect, app: &App) {
    let items = app
        .recorder
        .data
        .iter()
        .flat_map(|local| &local.work)
        .map(work_item)
        .collect();
    draw_list(
        frame,
        area,
        "Work · Enter opens".to_string(),
        items,
        app.work_index,
    );
}

fn work_item(plan: &crate::terminal::model::WorkPlanView) -> ListItem<'static> {
    let timing = if plan.state == WorkState::Open {
        plan.opened_at.clone()
    } else {
        format!(
            "{} · {} · {}",
            plan.closed_at,
            plan.duration,
            plan.resolution.as_deref().unwrap_or("no resolution")
        )
    };
    ListItem::new(format!(
        "{:<8} {}  {}  {timing}",
        plan.state_label.to_uppercase(),
        plan.display_plan_id,
        plan.title
    ))
}

fn work_preview(plan: &crate::terminal::model::WorkPlanView) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(format!("{}  {}", plan.display_plan_id, plan.title)),
        Line::from(format!(
            "State: {} · opened {} · closed {} · duration {}",
            plan.state_label, plan.opened_at, plan.closed_at, plan.duration
        )),
    ];
    if let Some(resolution) = &plan.resolution {
        lines.push(Line::from(format!("Resolution: {resolution}")));
    }
    if let Some(reference) = &plan.baseline_ref {
        lines.push(Line::from(format!(
            "Baseline: {reference} {}",
            plan.baseline_oid.as_deref().unwrap_or("—")
        )));
    }
    if let Some(error) = &plan.baseline_error {
        lines.push(Line::from(format!("Baseline error: {error}")));
    }
    if let Some(gates) = &plan.gates {
        lines.push(Line::from(format!(
            "Gates: {} · {}",
            gates.overall,
            gates.limit.label("gates")
        )));
        lines.extend(gates.gates.iter().take(6).map(|gate| {
            Line::from(format!(
                "  {} [{}] {} · {} · {}",
                gate.id, gate.status, gate.subject, gate.freshness, gate.ended_at
            ))
        }));
        if gates.gates.len() > 6 {
            lines.push(Line::from(format!(
                "  {} more retained gates; Enter for full detail",
                gates.gates.len() - 6
            )));
        }
    }
    if let Some(error) = &plan.gates_error {
        lines.push(Line::from(format!("Gate collection error: {error}")));
    }
    lines
}

pub(super) fn draw_timeline(frame: &mut Frame, area: Rect, app: &App) {
    let chunks =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).split(area);
    let rows = app.timeline_rows();
    let items = rows
        .iter()
        .map(|row| {
            ListItem::new(format!(
                "{} {:<8} {} {}  {}",
                row.timestamp,
                row.kind.label(),
                row.display_identity,
                row.primary,
                row.secondary
            ))
        })
        .collect::<Vec<_>>();
    let title = app.recorder.data.as_ref().map_or_else(
        || "Timeline".to_string(),
        |local| {
            format!(
                "Timeline [{}] · {} · {}",
                app.timeline_filter.label(),
                local.timeline_limit,
                local.limits.timeline.label("rows")
            )
        },
    );
    draw_list(frame, chunks[0], title, items, app.timeline_index);
    let mut lines = app.selected_timeline().map_or_else(
        || vec![Line::from("No timeline row matches this filter.")],
        |row| document_lines(&row.detail),
    );
    append_local_notices(&mut lines, app);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel(
                "Timeline preview · f/F filter · +/- rows · Enter detail",
            ))
            .wrap(Wrap { trim: true }),
        chunks[1],
    );
}

pub(super) fn draw_compact_timeline(frame: &mut Frame, area: Rect, app: &App) {
    let items = app
        .timeline_rows()
        .iter()
        .map(|row| {
            ListItem::new(format!(
                "{} {}  {}",
                row.kind.label(),
                row.primary,
                row.secondary
            ))
        })
        .collect();
    draw_list(
        frame,
        area,
        format!("Timeline [{}] · Enter opens", app.timeline_filter.label()),
        items,
        app.timeline_index,
    );
}

pub(super) fn draw_health(frame: &mut Frame, area: Rect, app: &App) {
    let chunks =
        Layout::horizontal([Constraint::Percentage(48), Constraint::Percentage(52)]).split(area);
    let Some(local) = app.recorder.data.as_ref() else {
        return;
    };
    let mut previous = None;
    let items = local
        .health
        .iter()
        .map(|row| {
            let section = if previous == Some(row.section) {
                String::new()
            } else {
                previous = Some(row.section);
                format!("{} · ", row.section)
            };
            ListItem::new(format!("{section}{}  {}", row.primary, row.secondary))
        })
        .collect::<Vec<_>>();
    draw_list(
        frame,
        chunks[0],
        format!(
            "Health · {} failures / {} tools · {} · {}",
            local.failures.len(),
            local.tools.len(),
            local.limits.failures.label("failures"),
            local.limits.tools.label("tools")
        ),
        items,
        app.health_index,
    );
    let mut lines = app.selected_health().map_or_else(
        || vec![Line::from("No health observations were reported.")],
        |row| document_lines(&row.detail),
    );
    append_local_notices(&mut lines, app);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel("Health detail · Enter opens"))
            .wrap(Wrap { trim: true }),
        chunks[1],
    );
}

pub(super) fn draw_compact_health(frame: &mut Frame, area: Rect, app: &App) {
    let items = app
        .recorder
        .data
        .iter()
        .flat_map(|local| &local.health)
        .map(|row| {
            ListItem::new(format!(
                "{} · {}  {}",
                row.section, row.primary, row.secondary
            ))
        })
        .collect();
    draw_list(
        frame,
        area,
        "Health · Enter opens".to_string(),
        items,
        app.health_index,
    );
}

pub(super) fn detail_footer(app: &App) -> String {
    if app.detail.base.is_none() {
        return "q quit | Esc back | r retry detail | R refresh all".to_string();
    }
    if app.detail.leaf.is_some() || matches!(app.detail.base, Some(BaseDetail::Document(_))) {
        return "q quit | Esc back | j/k vertical | h/l horizontal | r/R refresh".to_string();
    }
    match app.detail.section {
        PlanSection::Decisions | PlanSection::Receipts => {
            "q quit | Esc back | Tab section | j/k select | Enter opens | r/R refresh".to_string()
        }
        PlanSection::Summary | PlanSection::Body | PlanSection::Gates => {
            "q quit | Esc back | Tab section | j/k vertical | h/l horizontal | r/R refresh"
                .to_string()
        }
    }
}

pub(super) fn draw_detail(frame: &mut Frame, area: Rect, app: &App) {
    if let Some(document) = &app.detail.leaf {
        draw_document(
            frame,
            area,
            document,
            app.detail.leaf_scroll,
            app.detail.horizontal_scroll,
            "Leaf detail · Esc/Backspace returns",
        );
        return;
    }
    if app.detail.base.is_none()
        && let Some(plan_id) = &app.detail.loading_plan
    {
        let mut lines = vec![Line::from(format!(
            "Loading plan {}...",
            jig_tui::sanitize_text(plan_id)
        ))];
        if let Some(error) = &app.detail.error {
            lines.push(Line::from(format!("Last detail error: {error}")));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .block(panel("Plan detail"))
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    match app.detail.base.as_ref() {
        Some(BaseDetail::Document(document)) => {
            let observed = app.detail.item_generated_at_ms.map_or_else(
                || "unknown time".to_string(),
                |timestamp| crate::terminal::model::format_timestamp(Some(timestamp)),
            );
            let stale = app.detail.item_epoch.is_some_and(|epoch| {
                app.recorder
                    .data
                    .as_ref()
                    .is_some_and(|local| local.epoch_id != epoch)
            });
            let title = format!(
                "Detail · epoch {} · observed {observed}{} · Esc closes",
                app.detail
                    .item_epoch
                    .map_or(0, crate::dashboard::RecorderEpochId::get),
                if stale { " · stale" } else { "" }
            );
            draw_document(
                frame,
                area,
                document,
                app.detail.section_scroll[0],
                app.detail.horizontal_scroll,
                &title,
            );
        }
        Some(BaseDetail::Plan(plan)) => draw_plan_detail(frame, area, app, plan),
        None => {
            let message = app
                .detail
                .error
                .as_deref()
                .or(app.detail.notice.as_deref())
                .unwrap_or("No detail is available.");
            frame.render_widget(Paragraph::new(message).block(panel("Detail")), area);
        }
    }
}

fn draw_plan_detail(frame: &mut Frame, area: Rect, app: &App, plan: &PlanDetailView) {
    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(area);
    let section_tabs = PlanSection::ALL
        .iter()
        .map(|section| {
            if *section == app.detail.section {
                format!("[{}]", section.label())
            } else {
                section.label().to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("  ");
    let detail_state = if app.detail.loading_plan.is_some() {
        " · refreshing".to_string()
    } else if let Some(error) = &app.detail.error {
        format!(" · stale: {error}")
    } else {
        String::new()
    };
    let detail_title = format!("Plan detail{detail_state} · Tab sections · Esc closes");
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(format!(
                "{}  {} [{}] · basis epoch {}",
                plan.display_plan_id, plan.title, plan.state, plan.basis_epoch
            )),
            Line::from(section_tabs),
        ])
        .block(panel(&detail_title)),
        chunks[0],
    );
    let scroll = app.detail.section_scroll[app.detail.section.index()];
    match app.detail.section {
        PlanSection::Summary => draw_plan_document(
            frame,
            chunks[1],
            ("Summary", Some(&plan.summary), "Summary is unavailable."),
            plan,
            scroll,
            app.detail.horizontal_scroll,
        ),
        PlanSection::Body => draw_plan_document(
            frame,
            chunks[1],
            ("Body", plan.body.as_ref(), "Plan body is unavailable."),
            plan,
            scroll,
            app.detail.horizontal_scroll,
        ),
        PlanSection::Gates => draw_plan_document(
            frame,
            chunks[1],
            (
                "Gates",
                plan.gates_document.as_ref(),
                "Gate detail is unavailable.",
            ),
            plan,
            scroll,
            app.detail.horizontal_scroll,
        ),
        PlanSection::Decisions => {
            let items = plan
                .decisions
                .iter()
                .map(|decision| {
                    ListItem::new(format!(
                        "{}  {} → {}",
                        decision.display_id, decision.title, decision.selected
                    ))
                })
                .collect();
            draw_plan_list(
                frame,
                chunks[1],
                format!(
                    "Decisions · {} · Enter opens",
                    plan.decisions_limit.label("decisions")
                ),
                items,
                app.detail.decision_index,
                plan,
                "state.decisions",
            );
        }
        PlanSection::Receipts => {
            let items = plan
                .receipts
                .iter()
                .map(|receipt| {
                    ListItem::new(format!(
                        "{}  {} [{}]",
                        receipt.display_id, receipt.tool, receipt.status
                    ))
                })
                .collect();
            draw_plan_list(
                frame,
                chunks[1],
                format!(
                    "Receipts · {} · Enter opens",
                    plan.receipts_limit.label("receipts")
                ),
                items,
                app.detail.receipt_index,
                plan,
                "state.receipts",
            );
        }
    }
}

fn append_local_notices(lines: &mut Vec<Line<'static>>, app: &App) {
    if let Some(notice) = &app.detail.notice {
        lines.push(Line::from(notice.clone()).style(Style::default().fg(Color::Yellow)));
    }
    if let Some(local) = &app.recorder.data {
        lines.extend(local.errors.iter().map(|error| {
            Line::from(format!(
                "{}:{}{} — {}",
                error.scope,
                error.code,
                error
                    .subject
                    .as_deref()
                    .map(|subject| format!(" ({subject})"))
                    .unwrap_or_default(),
                error.message
            ))
            .style(Style::default().fg(BAD))
        }));
    }
}

fn document_lines(document: &DetailDocument) -> Vec<Line<'static>> {
    std::iter::once(
        Line::from(document.title.clone()).style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .chain(document.lines.iter().cloned().map(Line::from))
    .collect()
}

fn draw_document(
    frame: &mut Frame,
    area: Rect,
    document: &DetailDocument,
    scroll: u16,
    horizontal_scroll: u16,
    title: &str,
) {
    let visible = usize::from(area.height.saturating_sub(2).max(1));
    let lines = std::iter::once(document.title.as_str())
        .chain(document.lines.iter().map(String::as_str))
        .skip(usize::from(scroll))
        .take(visible)
        .map(|line| Line::from(line.to_string()))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel(title))
            .scroll((0, horizontal_scroll)),
        area,
    );
}

fn draw_plan_document(
    frame: &mut Frame,
    area: Rect,
    content: (&str, Option<&DetailDocument>, &str),
    plan: &PlanDetailView,
    scroll: u16,
    horizontal_scroll: u16,
) {
    let (title, document, unavailable) = content;
    let visible = usize::from(area.height.saturating_sub(2).max(1));
    let document_len = document.map_or(1, |document| 1 + document.lines.len());
    let total = document_len + plan.errors.len();
    let start = usize::from(scroll).min(total.saturating_sub(1));
    let end = (start + visible).min(total);
    let mut lines = Vec::with_capacity(end.saturating_sub(start));
    for index in start..end {
        if index < document_len {
            let line = document.map_or_else(
                || unavailable.to_string(),
                |document| {
                    if index == 0 {
                        document.title.clone()
                    } else {
                        document.lines[index - 1].clone()
                    }
                },
            );
            lines.push(Line::from(line));
        } else {
            let error = &plan.errors[index - document_len];
            lines.push(Line::from(format!(
                "Error {}:{}{} — {}",
                error.scope,
                error.code,
                error
                    .subject
                    .as_deref()
                    .map(|subject| format!(" ({subject})"))
                    .unwrap_or_default(),
                error.message
            )));
        }
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel(title))
            .scroll((0, horizontal_scroll)),
        area,
    );
}

fn draw_plan_list(
    frame: &mut Frame,
    area: Rect,
    title: String,
    items: Vec<ListItem<'static>>,
    selected: usize,
    plan: &PlanDetailView,
    scope: &str,
) {
    let errors = plan
        .errors
        .iter()
        .filter(|error| error.scope == scope)
        .map(|error| {
            Line::from(format!("Error {} — {}", error.code, error.message))
                .style(Style::default().fg(BAD))
        })
        .collect::<Vec<_>>();
    if errors.is_empty() {
        draw_list(frame, area, title, items, selected);
        return;
    }
    let chunks = Layout::vertical([
        Constraint::Length(
            u16::try_from(errors.len())
                .unwrap_or(u16::MAX)
                .saturating_add(2),
        ),
        Constraint::Min(1),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new(errors).block(panel("Collection errors")),
        chunks[0],
    );
    draw_list(frame, chunks[1], title, items, selected);
}

fn draw_list(
    frame: &mut Frame,
    area: Rect,
    title: String,
    items: Vec<ListItem<'static>>,
    selected: usize,
) {
    draw_list_selection(frame, area, title, items, Some(selected));
}

fn draw_list_selection(
    frame: &mut Frame,
    area: Rect,
    title: String,
    items: Vec<ListItem<'static>>,
    selected: Option<usize>,
) {
    let selected = selected
        .filter(|_| !items.is_empty())
        .map(|index| index.min(items.len().saturating_sub(1)));
    let mut state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(
        List::new(items)
            .block(panel(&title))
            .highlight_symbol("▶ ")
            .highlight_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        area,
        &mut state,
    );
}
