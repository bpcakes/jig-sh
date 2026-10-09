use ratatui::{
    Frame,
    layout::Rect,
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

use super::{View, theme::Theme};
use crate::model::Focus;

const SEARCH_PROMPT: &str = "Search: ";

/// A key hint. When the footer is too narrow, hints with the highest `rank`
/// drop first, so launching and cancelling stay visible longest.
struct Hint {
    key: &'static str,
    action: &'static str,
    rank: u8,
}

const fn hint(key: &'static str, action: &'static str, rank: u8) -> Hint {
    Hint { key, action, rank }
}

const HOMES: [Hint; 5] = [
    hint("↑↓", "move", 2),
    hint("/", "search", 3),
    hint("Tab", "details", 4),
    hint("Enter", "launch", 0),
    hint("Esc/q", "cancel", 1),
];
const DETAILS: [Hint; 6] = [
    hint("↑↓", "scroll", 2),
    hint("PgUp/PgDn", "jump", 5),
    hint("Tab", "homes", 3),
    hint("/", "search", 4),
    hint("Enter", "launch", 0),
    hint("Esc/q", "cancel", 1),
];
const SEARCH: [Hint; 4] = [
    hint("Backspace", "edit", 3),
    hint("Ctrl-U", "clear", 2),
    hint("Enter", "launch", 0),
    hint("Esc", "finish search", 1),
];

pub(super) fn draw_footer(frame: &mut Frame, area: Rect, view: &View<'_>) {
    let app = view.app;
    let theme = view.theme;
    let controls = if app.exit_state.is_some() && app.static_configuration {
        Line::styled(
            "Restoring the terminal.",
            Style::default().fg(theme.muted()),
        )
    } else if app.exit_state.is_some() {
        Line::styled(
            "Please wait while background inspection is stopped safely.",
            Style::default().fg(theme.muted()),
        )
    } else if app.searching {
        hint_line(theme, &SEARCH, area.width)
    } else if app.focus == Focus::Details {
        hint_line(theme, &DETAILS, area.width)
    } else {
        hint_line(theme, &HOMES, area.width)
    };
    let mut lines = vec![controls];
    if app.searching || !app.filter.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(SEARCH_PROMPT, Style::default().fg(theme.accent()).bold()),
            Span::raw(app.filter.clone()),
            Span::styled(
                if app.searching { "▌" } else { "" },
                Style::default().fg(theme.accent()),
            ),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), area);
    if app.searching && area.height > 1 {
        let prompt_width = u16::try_from(SEARCH_PROMPT.width()).unwrap_or(u16::MAX);
        let filter_width =
            u16::try_from(Line::from(app.filter.as_str()).width()).unwrap_or(u16::MAX);
        let cursor_x = area
            .x
            .saturating_add(prompt_width)
            .saturating_add(filter_width)
            .min(area.right().saturating_sub(1));
        frame.set_cursor_position((cursor_x, area.y.saturating_add(1)));
    }
}

/// As many key chips as fit, kept in their reading order.
fn hint_line(theme: Theme, hints: &[Hint], width: u16) -> Line<'static> {
    // ` key ` chip, action, then a two-cell gap before the next chip.
    let hint_width = |hint: &Hint| 1 + hint.key.width() + 1 + 1 + hint.action.width() + 2;
    let fits = |rank: u8| {
        let used = hints
            .iter()
            .filter(|hint| hint.rank <= rank)
            .map(hint_width)
            .sum::<usize>();
        used <= usize::from(width)
    };
    let mut rank = hints.iter().map(|hint| hint.rank).max().unwrap_or(0);
    while rank > 0 && !fits(rank) {
        rank -= 1;
    }
    let mut spans = Vec::new();
    for hint in hints.iter().filter(|hint| hint.rank <= rank) {
        spans.extend([
            Span::styled(format!(" {} ", hint.key), theme.chip()),
            Span::styled(
                format!(" {}  ", hint.action),
                Style::default().fg(theme.muted()),
            ),
        ]);
    }
    Line::from(spans)
}
