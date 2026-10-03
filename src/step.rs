//! When a step of a slide shows, written as a range of the steps of its
//! column, as in Rust: `3..5` shows from step 3 and hides again at step 5,
//! `..3` shows from the start and hides at step 3, `3..` shows from step 3 to
//! the end, and so does a bare `3`. The page reads them the same way.
//!
//! Without a range, a step comes one after the latest one so far, `latest +
//! 1..`, where the latest is the highest step any range so far starts at.
//!
//! A bound written unsigned, as `3`, is a step of the slide, wherever the
//! element is. Written signed, as `+1`, `+0` or `-1`, it is a number of
//! steps from the element's: the step of the latest element before it, among
//! those it is beside, that steps, or else of the element it is in, as
//! `+0..+2` shows from that step up to two after it.
//!
//! A step can be named, for others to show along with it, or a number of
//! steps from it: `step="a"` is a step of its own, named `a`, and a bound of
//! a range can be `a+0`, `a+1` or `a-1`, the step named `a`, or one after or
//! before it, wherever in the slide it is, as `a+0..b+0`. A name is always
//! written with its sign in a bound, for a bare one to name a step.
//!
//! Hidden, a step keeps its space, so that what is around it stays in place.
//! Starred, as `step*`, it takes none, so that another can show in
//! its place.

use std::fmt;

mod html;

pub use html::{Slide, Stepped, number, number_figure, slides, unnumber_figure};

/// How an element marks itself as a step: by the range it shows in, or as
/// the next step, or the next step with a name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Range(Range),
    Next,
    Named(String),
}

impl Step {
    /// What it is written as, after `step=`, if anything: its range, or its
    /// name.
    pub fn value(&self) -> Option<String> {
        match self {
            Step::Range(range) => Some(range.to_string()),
            Step::Named(name) => Some(name.clone()),
            Step::Next => None,
        }
    }

    /// The step written after `step=`: a range, or a name.
    pub fn parse(text: &str) -> Option<Step> {
        let text = text.trim();
        if is_name(text) {
            return Some(Step::Named(text.to_string()));
        }
        Range::parse(text).map(Step::Range)
    }
}

/// Whether `text` is a name a step can be given: letters, digits and `_`,
/// not starting with a digit.
pub fn is_name(text: &str) -> bool {
    text.starts_with(|char: char| char.is_ascii_alphabetic() || char == '_')
        && text
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || char == '_')
}

/// A step as an element is marked with, and whether, hidden, it takes no
/// space.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    pub step: Step,
    pub collapse: bool,
}

impl Mark {
    /// The step marked as `step=3..5` or `step`, or starred, as `step*=3..5`
    /// or `step*`, if it is one, as matplotlib's `gid`
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
            ("step", Some(range)) => Step::parse(range)?,
            _ => return None,
        };
        Some(Mark { step, collapse })
    }
}

/// A bound of a range: a step, a number of steps from the element's, or a
/// number of steps from a named one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bound {
    Step(u32),
    Relative(i32),
    Name { name: String, offset: i32 },
}

impl Bound {
    /// The bound written as `text`, as `3`, `+1`, `-1`, `a+0`, `a+1` or
    /// `a-1`.
    pub fn parse(text: &str) -> Option<Bound> {
        let number = |text: &str| -> Option<u32> {
            if !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()) {
                text.parse().ok()
            } else {
                None
            }
        };
        // A number of steps, written with its sign, as `+1` or `-1`.
        let signed = |text: &str| -> Option<i32> {
            let sign = match text.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            Some(sign * i32::try_from(number(&text[1..])?).ok()?)
        };
        if let Some(offset) = signed(text) {
            return Some(Bound::Relative(offset));
        }
        // A name, with the number of steps from it, as `a+0`.
        let Some(at) = text.find(['+', '-']) else {
            return number(text).map(Bound::Step);
        };
        let (name, offset) = (&text[..at], signed(&text[at..])?);
        is_name(name).then(|| Bound::Name {
            name: name.to_string(),
            offset,
        })
    }
}

impl fmt::Display for Bound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Bound::Step(step) => write!(f, "{step}"),
            Bound::Relative(offset) => write!(f, "{offset:+}"),
            Bound::Name { name, offset } => write!(f, "{name}{offset:+}"),
        }
    }
}

/// The steps an element shows in, from `start`, or the start, up to `end`,
/// excluded, or the end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Range {
    pub start: Option<Bound>,
    pub end: Option<Bound>,
}

impl Range {
    /// The range written as `text`, if it is one.
    pub fn parse(text: &str) -> Option<Range> {
        let text = text.trim();
        let bound = |text: &str| -> Option<Option<Bound>> {
            if text.is_empty() {
                Some(None)
            } else {
                Bound::parse(text).map(Some)
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
        match (&self.start, &self.end) {
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
        Some(Range {
            start: start.map(Bound::Step),
            end: end.map(Bound::Step),
        })
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
        for text in [
            "", "x", "1..x", "1...3", "--1", "+", "1..=3", "1.5", "@a", "@a+1", "1a+1", "a+",
            "a*2", "a..b",
        ] {
            assert_eq!(Range::parse(text), None, "{text}");
        }
    }

    #[test]
    fn a_step_is_a_range_a_name_or_the_next_step() {
        let mark = |step| {
            Some(Mark {
                step,
                collapse: false,
            })
        };
        let range = |text| Step::Range(Range::parse(text).unwrap());
        assert_eq!(Mark::parse("step=0..3"), mark(range("0..3")));
        assert_eq!(Mark::parse("step=a"), mark(Step::Named("a".into())));
        assert_eq!(Mark::parse("step=a+0..b+1"), mark(range("a+0..b+1")));
        assert_eq!(Mark::parse("step = ..3"), mark(range("..3")));
        assert_eq!(Mark::parse("step"), mark(Step::Next));
        let invalid = [
            "",
            "step=",
            "step=1x",
            "steps=1",
            "also",
            "also=1",
            "step 1",
            "step=1a",
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
        assert_eq!(Mark::parse("step*=1..3"), mark(range.clone()));
        assert_eq!(Mark::parse("step* = 1..3"), mark(range));
        assert_eq!(Mark::parse("step*"), mark(Step::Next));
    }

    #[test]
    fn a_bound_can_be_a_number_of_steps_from_a_named_one() {
        let named = |name: &str, offset| {
            Some(Bound::Name {
                name: name.into(),
                offset,
            })
        };
        assert_eq!(Bound::parse("a+0"), named("a", 0));
        assert_eq!(Bound::parse("a_1+2"), named("a_1", 2));
        assert_eq!(Bound::parse("b-1"), named("b", -1));
        assert_eq!(Bound::parse("a"), None);
        assert_eq!(Bound::parse("4"), Some(Bound::Step(4)));
    }

    #[test]
    fn a_signed_bound_is_a_number_of_steps_from_the_elements() {
        assert_eq!(Bound::parse("+1"), Some(Bound::Relative(1)));
        assert_eq!(Bound::parse("+0"), Some(Bound::Relative(0)));
        assert_eq!(Bound::parse("-1"), Some(Bound::Relative(-1)));
        assert_eq!(
            Range::parse("+0..+2"),
            Some(Range {
                start: Some(Bound::Relative(0)),
                end: Some(Bound::Relative(2)),
            })
        );
        assert_eq!(Bound::parse("+-1"), None);
    }

    #[test]
    fn a_range_is_written_back_the_same() {
        for text in [
            "3..5", "..3", "3", "..", "a+0", "a+1..b-2", "+1", "-1..+0", "..+2",
        ] {
            assert_eq!(Range::parse(text).unwrap().to_string(), text);
        }
        assert_eq!(Range::parse("3..").unwrap().to_string(), "3");
    }
}
