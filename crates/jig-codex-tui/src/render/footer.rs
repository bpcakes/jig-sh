use ratatui::{
    Frame,
    layout::Rect,
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

use super::{ACCENT, MUTED};
use crate::model::{App, Focus};

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
    hint("↑/↓ j/k", "move", 2),
    hint("/", "search", 3),
    hint("Tab", "details", 4),
    hint("Enter", "launch", 0),
    hint("Esc/q", "cancel", 1),
];
const DETAILS: [Hint; 6] = [
    hint("↑/↓ j/k", "scroll", 2),
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

pub(super) fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let controls = if app.exit_state.is_some() && app.static_configuration {
        Line::styled("Restoring the terminal.", Style::default().fg(MUTED))
    } else if app.exit_state.is_some() {
        Line::styled(
            "Please wait while background inspection is stopped safely.",
            Style::default().fg(MUTED),
        )
    } else if app.searching {
        hint_line("SEARCH", &SEARCH, area.width)
    } else if app.focus == Focus::Details {
        hint_line("DETAILS", &DETAILS, area.width)
    } else {
        hint_line("HOMES", &HOMES, area.width)
    };
    let mut lines = vec![controls];
    if app.searching || !app.filter.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(SEARCH_PROMPT, Style::default().fg(ACCENT).bold()),
            Span::raw(app.filter.clone()),
            Span::styled(
                if app.searching { "▌" } else { "" },
                Style::default().fg(ACCENT),
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

/// The mode name and as many hints as fit, kept in their reading order.
fn hint_line(mode: &'static str, hints: &[Hint], width: u16) -> Line<'static> {
    let fits = |rank: u8| {
        let hints_width = hints
            .iter()
            .filter(|hint| hint.rank <= rank)
            .map(|hint| 2 + hint.key.width() + 1 + hint.action.width())
            .sum::<usize>();
        mode.width() + hints_width <= usize::from(width)
    };
    let mut rank = hints.iter().map(|hint| hint.rank).max().unwrap_or(0);
    while rank > 0 && !fits(rank) {
        rank -= 1;
    }
    let mut spans = vec![Span::styled(mode, Style::default().fg(MUTED).bold())];
    for hint in hints.iter().filter(|hint| hint.rank <= rank) {
        spans.extend([
            Span::raw("  "),
            Span::styled(hint.key, Style::default().fg(ACCENT)),
            Span::raw(" "),
            Span::styled(hint.action, Style::default().fg(MUTED)),
        ]);
    }
    Line::from(spans)
}
