use ratatui::layout::Rect;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ViewportSize {
    width: u16,
    height: u16,
}

impl ViewportSize {
    pub(crate) const MIN_WIDTH: u16 = 46;
    pub(crate) const MIN_HEIGHT: u16 = 12;

    pub(crate) const fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }

    pub(crate) const fn width(self) -> u16 {
        self.width
    }

    pub(crate) const fn height(self) -> u16 {
        self.height
    }

    pub(crate) const fn supports_full_ui(self) -> bool {
        self.width >= Self::MIN_WIDTH && self.height >= Self::MIN_HEIGHT
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

    use super::{ViewportSize, ratatui_viewport};

    #[test]
    fn full_ui_support_requires_both_minimum_dimensions() {
        assert!(ViewportSize::new(46, 12).supports_full_ui());
        assert!(!ViewportSize::new(45, 12).supports_full_ui());
        assert!(!ViewportSize::new(46, 11).supports_full_ui());
    }

    #[test]
    fn ratatui_viewport_bounds_linear_cell_indices() {
        let (area, viewport) = ratatui_viewport(Rect::new(0, 0, 608, 113));

        assert_eq!(area, Rect::new(0, 0, 608, 107));
        assert_eq!(viewport, ViewportSize::new(608, 107));
        assert!(area.area() <= u32::from(u16::MAX));

        let (_, clipped) = ratatui_viewport(Rect::new(0, 0, 6_000, 20));
        assert_eq!(clipped, ViewportSize::new(6_000, 10));
        assert!(!clipped.supports_full_ui());
    }
}
