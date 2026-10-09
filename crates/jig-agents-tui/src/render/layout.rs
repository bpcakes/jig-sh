//! The picker's single layout decision: which panes are visible, where they
//! sit, and which list style fits. Panes stack whenever the details keep a
//! comfortable height below the list; they sit side by side only on terminals
//! too short for that and wide enough for both panes at readable widths.

use ratatui::layout::{Constraint, Layout, Rect};

use crate::model::{App, Focus};

pub(super) const MIN_WIDTH: u16 = 46;
pub(super) const MIN_HEIGHT: u16 = 12;
pub(super) const STACKED_ROW_HEIGHT: u16 = 2;
const HEADER_HEIGHT: u16 = 2;
const FOOTER_HEIGHT: u16 = 2;
const SEARCH_FOOTER_HEIGHT: u16 = 3;
const TABLE_CHROME_HEIGHT: u16 = 3;
/// Narrowest list that keeps the two-line table's columns readable.
const TWO_LINE_LIST_MIN_WIDTH: u16 = 60;
/// Narrowest list that fits the one-line table with its own Account column.
const FULL_LIST_MIN_WIDTH: u16 = 104;
/// Usage, pace, and configuration-path lines fit unwrapped between these
/// widths; extra width goes to the list.
const DETAILS_MIN_WIDTH: u16 = 56;
const DETAILS_MAX_WIDTH: u16 = 72;
const DETAILS_WIDTH_PERCENT: u32 = 40;
/// Beside the details, the list keeps its two-line table with room for long names.
const SIDE_BY_SIDE_LIST_MIN_WIDTH: u16 = 64;
const _: () = assert!(SIDE_BY_SIDE_LIST_MIN_WIDTH >= TWO_LINE_LIST_MIN_WIDTH);
const SIDE_BY_SIDE_MIN_WIDTH: u16 = SIDE_BY_SIDE_LIST_MIN_WIDTH + DETAILS_MIN_WIDTH;
/// Details-pane rows, borders included, that show an inspected home's account,
/// usage windows, and identity without scrolling.
const COMFORTABLE_DETAILS_HEIGHT: u16 = 20;
const STACKED_LIST_MAX_PERCENT: u32 = 58;
const MIN_STACKED_LIST_HEIGHT: u16 = TABLE_CHROME_HEIGHT + STACKED_ROW_HEIGHT;
/// A bordered details pane with a few readable lines; below this, panes alternate.
const MIN_STACKED_DETAILS_HEIGHT: u16 = 6;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum ListStyle {
    /// One column: home and account over the projection.
    Compact,
    /// Home and account share a two-line column beside usage and projection.
    TwoLine,
    /// One line per home with its own Account column.
    Full,
}

impl ListStyle {
    pub(super) fn for_width(width: u16) -> Self {
        if width >= FULL_LIST_MIN_WIDTH {
            Self::Full
        } else if width >= TWO_LINE_LIST_MIN_WIDTH {
            Self::TwoLine
        } else {
            Self::Compact
        }
    }
}

/// Visible panes: both side by side, the list above details sized to its rows,
/// or, when too short for both, only the focused pane (Tab switches).
pub(super) struct PickerLayout {
    pub(super) header: Rect,
    pub(super) list: Option<Rect>,
    pub(super) details: Option<Rect>,
    pub(super) footer: Rect,
}

pub(super) fn picker_layout(area: Rect, app: &App) -> PickerLayout {
    let footer_height = if app.searching || !app.filter.is_empty() {
        SEARCH_FOOTER_HEIGHT
    } else {
        FOOTER_HEIGHT
    };
    let [header, content, footer] = Layout::vertical([
        Constraint::Length(HEADER_HEIGHT),
        Constraint::Min(0),
        Constraint::Length(footer_height),
    ])
    .areas(area);
    // Decided as if the search line were showing, and from every home rather
    // than the visible ones, so starting or typing a search never flips it.
    let height = area
        .height
        .saturating_sub(HEADER_HEIGHT + SEARCH_FOOTER_HEIGHT);
    let row_height = list_row_height(app, content.width);
    let list_height = stacked_list_height(height, app.rows.len(), row_height);
    let (list, details) = if height.saturating_sub(list_height) >= COMFORTABLE_DETAILS_HEIGHT {
        stacked(content, app, row_height)
    } else if content.width >= SIDE_BY_SIDE_MIN_WIDTH {
        let [list, details] = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(details_width(content.width)),
        ])
        .areas(content);
        (Some(list), Some(details))
    } else if height >= MIN_STACKED_LIST_HEIGHT + MIN_STACKED_DETAILS_HEIGHT {
        stacked(content, app, row_height)
    } else {
        match app.focus {
            Focus::Homes => (Some(content), None),
            Focus::Details => (None, Some(content)),
        }
    };
    PickerLayout {
        header,
        list,
        details,
        footer,
    }
}

fn stacked(content: Rect, app: &App, row_height: u16) -> (Option<Rect>, Option<Rect>) {
    let list_height = stacked_list_height(content.height, app.visible_indices().len(), row_height);
    let [list, details] =
        Layout::vertical([Constraint::Length(list_height), Constraint::Fill(1)]).areas(content);
    (Some(list), Some(details))
}

/// The one-line table uses one row per home; every other list uses two.
fn list_row_height(app: &App, width: u16) -> u16 {
    if !app.static_configuration && ListStyle::for_width(width) == ListStyle::Full {
        1
    } else {
        STACKED_ROW_HEIGHT
    }
}

fn details_width(width: u16) -> u16 {
    let proportional = u32::from(width) * DETAILS_WIDTH_PERCENT / 100;
    u16::try_from(proportional)
        .unwrap_or(u16::MAX)
        .clamp(DETAILS_MIN_WIDTH, DETAILS_MAX_WIDTH)
}

/// Fits the list to its rows, never taking more than its share of the height
/// and always leaving the details pane its minimum.
fn stacked_list_height(content_height: u16, rows: usize, row_height: u16) -> u16 {
    let fitted = u16::try_from(rows.max(1))
        .unwrap_or(u16::MAX)
        .saturating_mul(row_height)
        .saturating_add(TABLE_CHROME_HEIGHT);
    let proportional = (u32::from(content_height) * STACKED_LIST_MAX_PERCENT + 50) / 100;
    let proportional = u16::try_from(proportional).unwrap_or(u16::MAX);
    fitted
        .min(proportional)
        .max(MIN_STACKED_LIST_HEIGHT)
        .min(content_height.saturating_sub(MIN_STACKED_DETAILS_HEIGHT))
}
