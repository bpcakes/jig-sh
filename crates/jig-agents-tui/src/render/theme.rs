//! The picker's palette. Truecolor terminals get the full palette and smooth
//! meter gradients; 256-color and 16-color terminals get the nearest colors,
//! and `NO_COLOR` keeps only glyphs, weight, and text. Every status keeps its
//! text, so color only reinforces it.

use ratatui::style::{Color, Modifier, Style};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ColorDepth {
    Monochrome,
    Basic,
    Indexed,
    TrueColor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Theme {
    depth: ColorDepth,
}

type Rgb = (u8, u8, u8);

// Mid-tones that keep their contrast on dark and light terminal backgrounds.
const ACCENT: Rgb = (100, 140, 235);
const BORDER: Rgb = (90, 98, 135);
const MUTED: Rgb = (120, 128, 165);
const FAINT: Rgb = (110, 116, 150);
const BRIGHT: Rgb = (205, 214, 250);
const GOOD: Rgb = (110, 180, 80);
const WARN: Rgb = (215, 155, 60);
const BAD: Rgb = (235, 90, 110);
const BEST: Rgb = (165, 120, 235);
const CURRENT: Rgb = (60, 170, 220);
const SELECTION: Rgb = (41, 53, 89);
const CHIP: Rgb = (52, 59, 88);
const ON_ACCENT: Rgb = (26, 27, 38);

impl Default for Theme {
    fn default() -> Self {
        Self::new(ColorDepth::TrueColor)
    }
}

impl Theme {
    pub(crate) const fn new(depth: ColorDepth) -> Self {
        Self { depth }
    }

    /// Reads `NO_COLOR`, `COLORTERM`, and `TERM` as most terminal tools do.
    pub(crate) fn detect() -> Self {
        Self::from_env(|key| std::env::var(key).ok())
    }

    pub(crate) fn from_env(var: impl Fn(&str) -> Option<String>) -> Self {
        let set = |key| var(key).filter(|value| !value.is_empty());
        let depth = if set("NO_COLOR").is_some() {
            ColorDepth::Monochrome
        } else if set("COLORTERM")
            .is_some_and(|value| matches!(value.as_str(), "truecolor" | "24bit"))
        {
            ColorDepth::TrueColor
        } else if set("TERM").is_some_and(|value| value.contains("256color")) {
            ColorDepth::Indexed
        } else {
            ColorDepth::Basic
        };
        Self::new(depth)
    }

    fn pick(self, rgb: Rgb, basic: Color) -> Color {
        match self.depth {
            ColorDepth::Monochrome => Color::Reset,
            ColorDepth::Basic => basic,
            ColorDepth::Indexed => Color::Indexed(nearest_xterm(rgb)),
            ColorDepth::TrueColor => Color::Rgb(rgb.0, rgb.1, rgb.2),
        }
    }

    pub(crate) fn accent(self) -> Color {
        self.pick(ACCENT, Color::Cyan)
    }

    pub(crate) fn border(self) -> Color {
        self.pick(BORDER, Color::DarkGray)
    }

    /// Labels and secondary text.
    pub(crate) fn muted(self) -> Color {
        self.pick(MUTED, Color::DarkGray)
    }

    /// Empty meter track and separators.
    pub(crate) fn faint(self) -> Color {
        self.pick(FAINT, Color::DarkGray)
    }

    /// Text on the selection and on key chips.
    pub(crate) fn bright(self) -> Color {
        self.pick(BRIGHT, Color::White)
    }

    pub(crate) fn good(self) -> Color {
        self.pick(GOOD, Color::Green)
    }

    pub(crate) fn warn(self) -> Color {
        self.pick(WARN, Color::Yellow)
    }

    pub(crate) fn bad(self) -> Color {
        self.pick(BAD, Color::Red)
    }

    pub(crate) fn best(self) -> Color {
        self.pick(BEST, Color::Magenta)
    }

    pub(crate) fn current(self) -> Color {
        self.pick(CURRENT, Color::Cyan)
    }

    /// The selected row: a filled bar, or bold without color.
    pub(crate) fn selection(self) -> Style {
        match self.depth {
            ColorDepth::Monochrome => Style::default().add_modifier(Modifier::BOLD),
            _ => Style::default().bg(self.pick(SELECTION, Color::Blue)),
        }
    }

    /// A key-hint chip, or reversed text without color.
    pub(crate) fn chip(self) -> Style {
        match self.depth {
            ColorDepth::Monochrome => Style::default().add_modifier(Modifier::REVERSED),
            _ => Style::default()
                .bg(self.pick(CHIP, Color::DarkGray))
                .fg(self.bright()),
        }
    }

    /// The title badge in the header.
    pub(crate) fn badge(self) -> Style {
        match self.depth {
            ColorDepth::Monochrome => {
                Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
            }
            _ => Style::default()
                .bg(self.accent())
                .fg(self.pick(ON_ACCENT, Color::Black))
                .add_modifier(Modifier::BOLD),
        }
    }

    /// Green through yellow to red as `position` goes from 0 to 1.
    pub(crate) fn gradient(self, position: f64) -> Color {
        let position = position.clamp(0.0, 1.0);
        match self.depth {
            ColorDepth::Monochrome => Color::Reset,
            ColorDepth::Basic if position < 0.5 => Color::Green,
            ColorDepth::Basic if position < 0.8 => Color::Yellow,
            ColorDepth::Basic => Color::Red,
            ColorDepth::Indexed | ColorDepth::TrueColor => {
                self.pick(gradient_rgb(position), Color::Reset)
            }
        }
    }
}

fn gradient_rgb(position: f64) -> Rgb {
    let position = position.clamp(0.0, 1.0);
    if position < 0.5 {
        mix(GOOD, WARN, position * 2.0)
    } else {
        mix(WARN, BAD, (position - 0.5) * 2.0)
    }
}

fn mix(from: Rgb, to: Rgb, amount: f64) -> Rgb {
    let channel = |a: u8, b: u8| {
        let value = f64::from(a) + (f64::from(b) - f64::from(a)) * amount;
        // In range by construction: a blend of two u8 values.
        value.round().clamp(0.0, 255.0) as u8
    };
    (
        channel(from.0, to.0),
        channel(from.1, to.1),
        channel(from.2, to.2),
    )
}

/// The closest entry in the xterm 6x6x6 color cube or its gray ramp.
fn nearest_xterm((red, green, blue): Rgb) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let level = |value: u8| {
        LEVELS
            .iter()
            .enumerate()
            .min_by_key(|(_, level)| level.abs_diff(value))
            .map_or(0, |(index, _)| index)
    };
    let (r, g, b) = (level(red), level(green), level(blue));
    let cube = (LEVELS[r], LEVELS[g], LEVELS[b]);
    let gray_level =
        ((u16::from(red) + u16::from(green) + u16::from(blue)) / 3).saturating_sub(8) / 10;
    let gray_level = gray_level.min(23);
    let gray_value = 8 + gray_level * 10;
    let gray = (gray_value, gray_value, gray_value);
    let distance = |(a, b, c): (u16, u16, u16)| {
        let d = |x: u16, y: u8| u32::from(x.abs_diff(u16::from(y))).pow(2);
        d(a, red) + d(b, green) + d(c, blue)
    };
    let cube_distance = distance((u16::from(cube.0), u16::from(cube.1), u16::from(cube.2)));
    if distance(gray) < cube_distance {
        // At most 232 + 23.
        232 + gray_level as u8
    } else {
        // At most 16 + 215.
        16 + (36 * r + 6 * g + b) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn depth(vars: &[(&str, &str)]) -> ColorDepth {
        Theme::from_env(|key| {
            vars.iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value).to_owned())
        })
        .depth
    }

    #[test]
    fn color_depth_follows_the_terminal_conventions() {
        assert_eq!(depth(&[("COLORTERM", "truecolor")]), ColorDepth::TrueColor);
        assert_eq!(depth(&[("COLORTERM", "24bit")]), ColorDepth::TrueColor);
        assert_eq!(depth(&[("TERM", "xterm-256color")]), ColorDepth::Indexed);
        assert_eq!(depth(&[("TERM", "xterm")]), ColorDepth::Basic);
        assert_eq!(depth(&[]), ColorDepth::Basic);
        assert_eq!(
            depth(&[("NO_COLOR", "1"), ("COLORTERM", "truecolor")]),
            ColorDepth::Monochrome
        );
        assert_eq!(
            depth(&[("NO_COLOR", ""), ("COLORTERM", "truecolor")]),
            ColorDepth::TrueColor
        );
    }

    #[test]
    fn gradient_runs_from_good_through_warn_to_bad() {
        let theme = Theme::default();
        assert_eq!(theme.gradient(0.0), Color::Rgb(GOOD.0, GOOD.1, GOOD.2));
        assert_eq!(theme.gradient(0.5), Color::Rgb(WARN.0, WARN.1, WARN.2));
        assert_eq!(theme.gradient(1.0), Color::Rgb(BAD.0, BAD.1, BAD.2));
        assert_eq!(theme.gradient(7.0), theme.gradient(1.0));
        let basic = Theme::new(ColorDepth::Basic);
        assert_eq!(basic.gradient(0.1), Color::Green);
        assert_eq!(basic.gradient(0.6), Color::Yellow);
        assert_eq!(basic.gradient(0.9), Color::Red);
        assert_eq!(
            Theme::new(ColorDepth::Monochrome).gradient(0.9),
            Color::Reset
        );
    }

    #[test]
    fn indexed_colors_pick_the_nearest_cube_or_gray_entry() {
        assert_eq!(nearest_xterm((0, 0, 0)), 16);
        assert_eq!(nearest_xterm((255, 255, 255)), 231);
        assert_eq!(nearest_xterm((255, 0, 0)), 196);
        assert_eq!(nearest_xterm((128, 128, 128)), 244);
        assert_eq!(Theme::new(ColorDepth::Indexed).accent(), Color::Indexed(68));
    }
}
