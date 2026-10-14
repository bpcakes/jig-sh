//! The complete fleet explanation remains readable even when the overview is compact.

use ratatui::{Frame, layout::Rect, style::Style, widgets::Paragraph};

use crate::render::{
    View, panel,
    wrap::{Detail, wrap},
};

pub(in crate::render) fn draw_details(frame: &mut Frame, area: Rect, view: &View<'_>) {
    let Some(fleet) = &view.fleet else {
        return;
    };
    let app = view.app;
    app.set_hit_areas(|areas| areas.fleet_details = Some(area));
    let block = panel(view, "fleet forecast", &fleet.coverage_label(), true);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let width = usize::from(inner.width);
    let facts = fleet.detail_lines_at(view.now);
    let label_width = facts
        .iter()
        .map(|(label, _)| label.len())
        .max()
        .unwrap_or(0);
    let lines = facts
        .into_iter()
        .flat_map(|(label, value)| {
            wrap(
                Detail::labeled(
                    &label,
                    &value,
                    label_width,
                    Style::default().fg(view.theme.muted()),
                ),
                width,
            )
        })
        .collect::<Vec<_>>();
    let limit =
        u16::try_from(lines.len().saturating_sub(usize::from(inner.height))).unwrap_or(u16::MAX);
    app.set_fleet_scroll_limit(limit);
    frame.render_widget(
        Paragraph::new(lines).scroll((app.fleet_scroll.min(limit), 0)),
        inner,
    );
}
