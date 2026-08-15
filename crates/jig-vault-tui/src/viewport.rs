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

#[cfg(test)]
mod tests {
    use super::ViewportSize;

    #[test]
    fn full_ui_support_requires_both_minimum_dimensions() {
        assert!(ViewportSize::new(46, 12).supports_full_ui());
        assert!(!ViewportSize::new(45, 12).supports_full_ui());
        assert!(!ViewportSize::new(46, 11).supports_full_ui());
    }
}
