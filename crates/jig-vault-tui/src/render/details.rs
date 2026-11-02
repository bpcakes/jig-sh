use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use super::{MUTED, WARN, key_value, panel};
use crate::model::{App, Focus};

pub(super) fn draw_details(frame: &mut Frame, area: Rect, app: &App) {
    let title = if app.focus == Focus::Details {
        "Details •"
    } else {
        "Details"
    };
    let lines = if let Some(field) = app.selected_field() {
        vec![
            key_value("Reference", &field.reference.to_string()),
            key_value("Item", field.reference.item()),
            key_value("Field", field.reference.field()),
            key_value("Kind", field.kind.as_str()),
            key_value("Length", &format!("{} bytes", field.value_len)),
            key_value("Created", &format!("{} ms", field.created_at_ms)),
            key_value("Updated", &format!("{} ms", field.updated_at_ms)),
            Line::from(""),
            match app
                .snapshot()
                .and_then(|snapshot| snapshot.text_value(&field.reference))
            {
                Some(text) => key_value("Value", if text.is_empty() { "(empty)" } else { text }),
                None => Line::from(Span::styled(
                    "Value hidden. Use Peek to view it.",
                    Style::default().fg(MUTED),
                )),
            },
        ]
    } else if let Some(secret) = app.selected_legacy() {
        vec![
            key_value("Legacy name", &secret.name),
            key_value("Length", &format!("{} bytes", secret.value_len)),
            key_value("Created", &format!("{} ms", secret.created_at_ms)),
            key_value("Updated", &format!("{} ms", secret.updated_at_ms)),
            Line::from(""),
            Line::from(Span::styled(
                "Unrepresentable legacy entry; convert it to a canonical field for export.",
                Style::default().fg(WARN),
            )),
        ]
    } else {
        vec![Line::from("No entry selected.")]
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel(title))
            .wrap(Wrap { trim: false }),
        area,
    );
}
