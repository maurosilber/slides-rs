//! When a step of a slide shows, written as a range of the steps of its
//! column, as in Rust: `3..5` shows from step 3 and hides again at step 5,
//! `..3` shows from the start and hides at step 3, `3..` shows from step 3 to
//! the end, and so does a bare `3`. The page reads them the same way.
//!
//! Without a range, a step comes one after the latest one so far, and an
//! `also` along with it: `latest + 1..` and `latest..`, where the latest is
//! the highest step any range so far starts at. An `also` before any step is
//! the first step.
//!
//! Hidden, a step keeps its space, so that what is around it stays in place.
//! Starred, as `step*` or `also*`, it takes none, so that another can show in
//! its place.

use std::fmt;

mod html;

pub use html::{number, number_figure, unnumber_figure};

/// How an element marks itself as a step: by the range it shows in, or as
/// the next step, or along with the latest one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Range(Range),
    Next,
    Also,
}

/// A step as an element is marked with, and whether, hidden, it takes no
/// space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark {
    pub step: Step,
    pub collapse: bool,
}

impl Mark {
    /// The step marked as `step=3..5`, `step` or `also`, or starred, as
    /// `step*=3..5`, `step*` or `also*`, if it is one, as matplotlib's `gid`
    /// marks it.
    pub fn parse(text: &str) -> Option<Mark> {
        let text = text.trim();
        let (name, range) = match text.split_once('=') {
            Some((name, range)) => (name.trim_end(), Some(range)),
            None => (text, None),
        };
        let (name, collapse) = match name.strip_suffix('*') {
            Some(name) => (name, true),
            None => (name, false),
        };
        let step = match (name, range) {
            ("step", None) => Step::Next,
            ("also", None) => Step::Also,
            ("step", Some(range)) => Step::Range(Range::parse(range)?),
            _ => return None,
        };
        Some(Mark { step, collapse })
    }
}

/// The steps an element shows in, from `start`, or the start, up to `end`,
/// excluded, or the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub start: Option<u32>,
    pub end: Option<u32>,
}

impl Range {
    /// The range written as `text`, if it is one.
    pub fn parse(text: &str) -> Option<Range> {
        let text = text.trim();
        let bound = |text: &str| -> Option<Option<u32>> {
            if text.is_empty() {
                Some(None)
            } else if text.bytes().all(|byte| byte.is_ascii_digit()) {
                text.parse().ok().map(Some)
            } else {
                None
            }
        };
        match text.split_once("..") {
            Some((start, end)) => Some(Range {
                start: bound(start)?,
                end: bound(end)?,
            }),
            None => Some(Range {
                start: Some(bound(text)??),
                end: None,
            }),
        }
    }
}

impl fmt::Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.start, self.end) {
            (Some(start), None) => write!(f, "{start}"),
            (start, end) => {
                if let Some(start) = start {
                    write!(f, "{start}")?;
                }
                f.write_str("..")?;
                if let Some(end) = end {
                    write!(f, "{end}")?;
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(start: Option<u32>, end: Option<u32>) -> Option<Range> {
        Some(Range { start, end })
    }

    #[test]
    fn a_range_is_written_as_in_rust() {
        assert_eq!(Range::parse("3..5"), range(Some(3), Some(5)));
        assert_eq!(Range::parse("..3"), range(None, Some(3)));
        assert_eq!(Range::parse("3.."), range(Some(3), None));
        assert_eq!(Range::parse(".."), range(None, None));
        assert_eq!(Range::parse(" 2 "), range(Some(2), None));
    }

    #[test]
    fn anything_else_is_not_a_range() {
        for text in ["", "x", "1..x", "1...3", "-1", "1..=3", "1.5"] {
            assert_eq!(Range::parse(text), None, "{text}");
        }
    }

    #[test]
    fn a_step_is_a_range_the_next_step_or_along_with_the_latest() {
        let mark = |step| {
            Some(Mark {
                step,
                collapse: false,
            })
        };
        let range = |text| Step::Range(Range::parse(text).unwrap());
        assert_eq!(Mark::parse("step=0..3"), mark(range("0..3")));
        assert_eq!(Mark::parse("step = ..3"), mark(range("..3")));
        assert_eq!(Mark::parse("step"), mark(Step::Next));
        assert_eq!(Mark::parse("also"), mark(Step::Also));
        let invalid = [
            "",
            "step=",
            "step=x",
            "steps=1",
            "also=1",
            "step 1",
            "fragment 1",
            "*step",
            "step**",
        ];
        for text in invalid {
            assert_eq!(Mark::parse(text), None, "{text}");
        }
    }

    #[test]
    fn a_starred_step_takes_no_space() {
        let mark = |step| {
            Some(Mark {
                step,
                collapse: true,
            })
        };
        let range = Step::Range(Range::parse("1..3").unwrap());
        assert_eq!(Mark::parse("step*=1..3"), mark(range));
        assert_eq!(Mark::parse("step* = 1..3"), mark(range));
        assert_eq!(Mark::parse("step*"), mark(Step::Next));
        assert_eq!(Mark::parse("also*"), mark(Step::Also));
    }

    #[test]
    fn a_range_is_written_back_the_same() {
        for text in ["3..5", "..3", "3", ".."] {
            assert_eq!(Range::parse(text).unwrap().to_string(), text);
        }
        assert_eq!(Range::parse("3..").unwrap().to_string(), "3");
    }
}
