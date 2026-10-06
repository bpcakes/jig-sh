//! Explicit format migration confirmation.

use jig_vault::LATEST_VAULT_FORMAT_VERSION;
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
};

use super::{WARN, panel};
use crate::model::App;

pub(super) fn draw_confirmation(frame: &mut Frame, area: Rect, app: &App) {
    let from = app
        .snapshot()
        .map(|snapshot| snapshot.format_version.to_string())
        .unwrap_or_else(|| "?".to_owned());
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                format!(
                    "Migrate vault from version {from} to version {LATEST_VAULT_FORMAT_VERSION}?"
                ),
                Style::default().fg(WARN).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("This one-way upgrade preserves values, field kinds, and audit history."),
            Line::from("Version 1 values become concealed fields."),
            Line::from("Older Jig versions will reject the migrated vault."),
            Line::from(""),
            Line::from("Enter migrate   Esc cancel"),
        ])
        .alignment(Alignment::Center)
        .block(panel("Confirm migration"))
        .wrap(Wrap { trim: true }),
        area,
    );
}
