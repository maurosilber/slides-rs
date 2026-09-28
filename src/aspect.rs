//! The shape of the slides, as the frontmatter's `aspect-ratio` sets it:
//! `16:9`, `4:3`, `16/9` or a number, as `1.6`, keeps every slide that
//! shape, as large as the window lets it, and `none` fills the window.
//! Without it, the slides are as slides.css says, 16:9.

use std::fmt;

/// The shape the slides keep.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AspectRatio {
    /// As wide as `width` is to `height`.
    Fixed { width: f64, height: f64 },
    /// As wide and as tall as the window.
    Fill,
}

impl AspectRatio {
    /// The ratio written as `16:9`, `16/9`, `1.6` or `none`, if it is one.
    pub fn parse(text: &str) -> Option<AspectRatio> {
        let text = text.trim();
        if text == "none" {
            return Some(AspectRatio::Fill);
        }
        let (width, height) = match text.split_once([':', '/']) {
            Some((width, height)) => (width.trim().parse().ok()?, height.trim().parse().ok()?),
            None => (text.parse().ok()?, 1.0),
        };
        AspectRatio::fixed(width, height)
    }

    /// A ratio, if both sides are sizes a slide can have.
    fn fixed(width: f64, height: f64) -> Option<AspectRatio> {
        let valid = |side: f64| side.is_finite() && side > 0.0;
        (valid(width) && valid(height)).then_some(AspectRatio::Fixed { width, height })
    }

    /// The custom properties slides.css reads to lay the slides out.
    pub fn css(&self) -> String {
        match self {
            AspectRatio::Fixed { width, height } => format!("--aspect-ratio: {width} / {height}"),
            AspectRatio::Fill => "--slide-width: 100vw; --slide-height: 100vh".to_string(),
        }
    }
}

/// The ratio as the frontmatter has it, which YAML reads as a number or as
/// text, as it is written.
#[derive(serde::Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum Written {
    Number(f64),
    Text(String),
}

impl Written {
    pub fn aspect_ratio(&self) -> Option<AspectRatio> {
        match self {
            Written::Number(number) => AspectRatio::fixed(*number, 1.0),
            Written::Text(text) => AspectRatio::parse(text),
        }
    }
}

impl fmt::Display for Written {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Written::Number(number) => write!(f, "{number}"),
            Written::Text(text) => f.write_str(text),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed(width: f64, height: f64) -> Option<AspectRatio> {
        Some(AspectRatio::Fixed { width, height })
    }

    #[test]
    fn a_ratio_is_written_as_two_sides_or_one_number() {
        assert_eq!(AspectRatio::parse("16:9"), fixed(16.0, 9.0));
        assert_eq!(AspectRatio::parse("4 / 3"), fixed(4.0, 3.0));
        assert_eq!(AspectRatio::parse("1.6"), fixed(1.6, 1.0));
        assert_eq!(AspectRatio::parse("none"), Some(AspectRatio::Fill));
        for text in ["", "16:", "a:b", "0:1", "-4:3", "16:9:1", "inf", "auto"] {
            assert_eq!(AspectRatio::parse(text), None, "{text}");
        }
    }

    #[test]
    fn a_ratio_is_laid_out_by_the_stylesheet() {
        assert_eq!(fixed(16.0, 9.0).unwrap().css(), "--aspect-ratio: 16 / 9");
        assert_eq!(fixed(1.6, 1.0).unwrap().css(), "--aspect-ratio: 1.6 / 1");
        assert_eq!(
            AspectRatio::Fill.css(),
            "--slide-width: 100vw; --slide-height: 100vh"
        );
    }
}
