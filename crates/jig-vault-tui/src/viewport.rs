use ratatui::layout::Rect;

use crate::model::Screen;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScreenLayout {
    width: u16,
    height: u16,
    label: &'static str,
}

impl ScreenLayout {
    const fn new(width: u16, height: u16, label: &'static str) -> Self {
        Self {
            width,
            height,
            label,
        }
    }

    pub(crate) const fn width(self) -> u16 {
        self.width
    }

    pub(crate) const fn height(self) -> u16 {
        self.height
    }

    pub(crate) const fn label(self) -> &'static str {
        self.label
    }
}

const BROWSER_LAYOUT: ScreenLayout = ScreenLayout::new(46, 12, "Vault browser");
const LOADING_LAYOUT: ScreenLayout = ScreenLayout::new(64, 16, "Vault operation");

pub(crate) const fn screen_layout(screen: &Screen) -> ScreenLayout {
    match screen {
        Screen::Browse => BROWSER_LAYOUT,
        Screen::Missing => ScreenLayout::new(72, 20, "Vault setup"),
        Screen::Locked(_) => ScreenLayout::new(72, 20, "Vault unlock"),
        Screen::Initialize { .. } => ScreenLayout::new(72, 20, "Vault initialization"),
        Screen::Loading(_) => LOADING_LAYOUT,
        Screen::ConfirmMigration => ScreenLayout::new(64, 18, "Migration confirmation"),
        Screen::Help => ScreenLayout::new(64, 16, "Vault help"),
        Screen::Form(_) => ScreenLayout::new(80, 24, "Secret editor"),
        Screen::ConfirmMutation(_) => ScreenLayout::new(80, 24, "Mutation confirmation"),
        Screen::ConfirmDelete(_) => ScreenLayout::new(80, 24, "Permanent deletion"),
        Screen::Commands(_) => ScreenLayout::new(64, 16, "Command palette"),
        Screen::QuickAccess(_) => ScreenLayout::new(64, 16, "Quick Access"),
        Screen::ToolForm(_) => ScreenLayout::new(80, 24, "Vault tool"),
        Screen::ImportPreview(_) => ScreenLayout::new(80, 24, "1Password import preview"),
        Screen::Activity(_) => ScreenLayout::new(64, 18, "Vault activity"),
        Screen::AuditResult(_) => ScreenLayout::new(64, 18, "Audit result"),
        Screen::ConfirmPeek(_) => ScreenLayout::new(80, 24, "Peek confirmation"),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ViewportSize {
    width: u16,
    height: u16,
}

impl ViewportSize {
    pub(crate) const fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }

    pub(crate) const fn width(self) -> u16 {
        self.width
    }

    pub(crate) const fn height(self) -> u16 {
        self.height
    }

    pub(crate) const fn supports(self, layout: ScreenLayout) -> bool {
        self.width >= layout.width && self.height >= layout.height
    }
}

/// Ratatui 0.29 allocates buffers from a `u32` area, but `Buffer::pos_of`
/// narrows the linear cell index to `u16` while producing terminal diffs.
/// Keep rendering and input capability checks on the same non-wrapping area.
pub(crate) const fn ratatui_viewport(area: Rect) -> (Rect, ViewportSize) {
    let render_area = match u16::MAX.checked_div(area.width) {
        None => area,
        Some(safe_height) => {
            let height = if area.height < safe_height {
                area.height
            } else {
                safe_height
            };
            Rect::new(area.x, area.y, area.width, height)
        }
    };
    (
        render_area,
        ViewportSize::new(render_area.width, render_area.height),
    )
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use crate::model::Screen;

    use super::{ViewportSize, ratatui_viewport, screen_layout};

    #[test]
    fn browser_support_requires_both_minimum_dimensions() {
        let browser = screen_layout(&Screen::Browse);
        assert!(ViewportSize::new(46, 12).supports(browser));
        assert!(!ViewportSize::new(45, 12).supports(browser));
        assert!(!ViewportSize::new(46, 11).supports(browser));
    }

    #[test]
    fn ratatui_viewport_bounds_linear_cell_indices() {
        let (area, viewport) = ratatui_viewport(Rect::new(0, 0, 608, 113));

        assert_eq!(area, Rect::new(0, 0, 608, 107));
        assert_eq!(viewport, ViewportSize::new(608, 107));
        assert!(area.area() <= u32::from(u16::MAX));

        let (_, clipped) = ratatui_viewport(Rect::new(0, 0, 6_000, 20));
        assert_eq!(clipped, ViewportSize::new(6_000, 10));
        assert!(!clipped.supports(screen_layout(&Screen::Browse)));
    }
}
