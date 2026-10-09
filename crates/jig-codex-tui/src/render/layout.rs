//! The picker's single layout decision: which panes are visible, where they
//! sit, and which list style fits. Every breakpoint lives here so the tiers
//! stay monotonic: a wider terminal never yields a poorer list.

use ratatui::layout::{Constraint, Layout, Rect};

use crate::model::{App, Focus};

pub(super) const MIN_WIDTH: u16 = 46;
pub(super) const MIN_HEIGHT: u16 = 12;
pub(super) const STACKED_ROW_HEIGHT: u16 = 2;
const HEADER_HEIGHT: u16 = 2;
const TABLE_CHROME_HEIGHT: u16 = 3;
/// Narrowest list that keeps the two-line table's columns readable.
const TWO_LINE_LIST_MIN_WIDTH: u16 = 60;
/// Narrowest list that fits the one-line table with its own Account column.
const FULL_LIST_MIN_WIDTH: u16 = 104;
/// Detail lines read well between these widths; extra width goes to the list.
const DETAILS_MIN_WIDTH: u16 = 44;
const DETAILS_MAX_WIDTH: u16 = 64;
const DETAILS_WIDTH_PERCENT: u32 = 40;
/// Panes sit side by side once the list keeps its two-line table beside readable details.
const SIDE_BY_SIDE_MIN_WIDTH: u16 = TWO_LINE_LIST_MIN_WIDTH + DETAILS_MIN_WIDTH;
// A stacked list is narrower than the side-by-side breakpoint, so it must not
// earn a richer style than the side-by-side list gets at that breakpoint.
const _: () = assert!(SIDE_BY_SIDE_MIN_WIDTH <= FULL_LIST_MIN_WIDTH);
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
        3
    } else {
        2
    };
    let [header, content, footer] = Layout::vertical([
        Constraint::Length(HEADER_HEIGHT),
        Constraint::Min(0),
        Constraint::Length(footer_height),
    ])
    .areas(area);
    let (list, details) = if content.width >= SIDE_BY_SIDE_MIN_WIDTH {
        let [list, details] = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(details_width(content.width)),
        ])
        .areas(content);
        (Some(list), Some(details))
    } else if content.height >= MIN_STACKED_LIST_HEIGHT + MIN_STACKED_DETAILS_HEIGHT {
        let list_height = stacked_list_height(content.height, app.visible_indices().len());
        let [list, details] =
            Layout::vertical([Constraint::Length(list_height), Constraint::Fill(1)]).areas(content);
        (Some(list), Some(details))
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

fn details_width(width: u16) -> u16 {
    let proportional = u32::from(width) * DETAILS_WIDTH_PERCENT / 100;
    u16::try_from(proportional)
        .unwrap_or(u16::MAX)
        .clamp(DETAILS_MIN_WIDTH, DETAILS_MAX_WIDTH)
}

/// Fits the list to its rows, never taking more than its share of the height
/// and always leaving the details pane its minimum.
fn stacked_list_height(content_height: u16, rows: usize) -> u16 {
    let fitted = u16::try_from(rows.max(1))
        .unwrap_or(u16::MAX)
        .saturating_mul(STACKED_ROW_HEIGHT)
        .saturating_add(TABLE_CHROME_HEIGHT);
    let proportional = (u32::from(content_height) * STACKED_LIST_MAX_PERCENT + 50) / 100;
    let proportional = u16::try_from(proportional).unwrap_or(u16::MAX);
    fitted
        .min(proportional)
        .max(MIN_STACKED_LIST_HEIGHT)
        .min(content_height.saturating_sub(MIN_STACKED_DETAILS_HEIGHT))
}
