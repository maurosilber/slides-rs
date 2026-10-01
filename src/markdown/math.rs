//! Steps of a slide, written inside math.
//!
//! `\step{...}` appears one step after the latest one, `\also{...}` along
//! with it, and `\step[3..5]{...}` in the steps its range says. Each becomes
//! `\htmlData{step=...}{...}` with its range, which src/step/html.rs numbers
//! among the other steps of the slide, as KaTeX writes them to the page.
//! Starred, as `\step*` or `\also*`, they take no space while hidden, and are
//! written with a `collapse` too. Their ranges are numbered here, in the order
//! they are written, rather than as KaTeX lays them out: an aligned
//! environment column by column, the left side of every row before any right
//! side.

use crate::step::Range;

/// Counts the steps of one column of a slide, as its numbers start over at
/// every heading, the way the slides count them.
#[derive(Default)]
pub struct Steps {
    /// The highest step a range starts at so far.
    latest: u32,
}

impl Steps {
    /// Starts counting a new column, or a new slide.
    pub fn reset(&mut self) {
        self.latest = 0;
    }

    /// The TeX, with every `\step` and `\also` written with its range.
    pub fn number(&mut self, tex: &str) -> String {
        let mut numbered = String::with_capacity(tex.len());
        let mut rest = tex;
        while let Some(start) = rest.find('\\') {
            numbered.push_str(&rest[..start]);
            let command = &rest[start + 1..];
            let name_len = command
                .find(|c: char| !c.is_ascii_alphabetic())
                .unwrap_or(command.len());
            if name_len == 0 {
                // A control symbol, like `\\` or `\{`, which names no command.
                let symbol_len = command.chars().next().map_or(0, char::len_utf8);
                numbered.push_str(&rest[start..start + 1 + symbol_len]);
                rest = &command[symbol_len..];
                continue;
            }
            let (name, mut after) = command.split_at(name_len);
            let starred = matches!(name, "step" | "also") && after.starts_with('*');
            if starred {
                after = &after[1..];
            }
            let range = match name {
                "step" => match range_argument(after) {
                    Some((range, argument_len)) => {
                        after = &after[argument_len..];
                        Some(range)
                    }
                    None => Some(Range {
                        start: Some(self.latest + 1),
                        end: None,
                    }),
                },
                "also" => Some(Range {
                    start: Some(self.latest.max(1)),
                    end: None,
                }),
                _ => None,
            };
            match range {
                Some(range) => {
                    if let Some(start) = range.start {
                        self.latest = self.latest.max(start);
                    }
                    let collapse = if starred { ",collapse=true" } else { "" };
                    numbered.push_str(&format!("\\htmlData{{step={range}{collapse}}}"));
                }
                None => {
                    numbered.push('\\');
                    numbered.push_str(name);
                }
            }
            rest = after;
        }
        numbered.push_str(rest);
        numbered
    }
}

/// The range in a `[3..5]` that opens `tex`, and how long it is written. One
/// that is not a range is left for KaTeX to show, which points it out.
fn range_argument(tex: &str) -> Option<(Range, usize)> {
    let argument = tex.strip_prefix('[')?;
    let (range, _) = argument.split_once(']')?;
    Some((Range::parse(range)?, range.len() + 2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_are_numbered_in_the_order_they_are_written() {
        let mut steps = Steps::default();
        assert_eq!(
            steps.number(r"a \step{b} \step{c}"),
            r"a \htmlData{step=1}{b} \htmlData{step=2}{c}"
        );
        // The count goes on in the next expression of the same column.
        assert_eq!(steps.number(r"\step{d}"), r"\htmlData{step=3}{d}");
    }

    #[test]
    fn also_joins_the_latest_step() {
        let mut steps = Steps::default();
        let tex = r"a &= b \\ \step{c} &\also{= d} \\ \step{e} &\also{= f}";
        assert_eq!(
            steps.number(tex),
            r"a &= b \\ \htmlData{step=1}{c} &\htmlData{step=1}{= d} \\ \htmlData{step=2}{e} &\htmlData{step=2}{= f}"
        );
    }

    #[test]
    fn also_before_any_step_is_the_first() {
        let mut steps = Steps::default();
        assert_eq!(
            steps.number(r"\also{a} \step{b}"),
            r"\htmlData{step=1}{a} \htmlData{step=2}{b}"
        );
    }

    #[test]
    fn a_step_can_say_its_range() {
        let mut steps = Steps::default();
        assert_eq!(
            steps.number(r"\step[2..4]{a} \step{b} \also{c} \step[..9]{d} \step{e}"),
            r"\htmlData{step=2..4}{a} \htmlData{step=3}{b} \htmlData{step=3}{c} \htmlData{step=..9}{d} \htmlData{step=4}{e}"
        );
    }

    #[test]
    fn a_starred_step_collapses() {
        let mut steps = Steps::default();
        assert_eq!(
            steps.number(r"\step*[..2]{a}\step*[2..]{b} \also*{c} \step*{d} \step{e}"),
            concat!(
                r"\htmlData{step=..2,collapse=true}{a}\htmlData{step=2,collapse=true}{b} ",
                r"\htmlData{step=2,collapse=true}{c} \htmlData{step=3,collapse=true}{d} ",
                r"\htmlData{step=4}{e}",
            )
        );
    }

    #[test]
    fn a_range_that_is_not_one_is_left_to_show() {
        let mut steps = Steps::default();
        assert_eq!(steps.number(r"\step[x]{a}"), r"\htmlData{step=1}[x]{a}");
    }

    #[test]
    fn a_heading_starts_the_count_over() {
        let mut steps = Steps::default();
        steps.number(r"\step{a} \step{b}");
        steps.reset();
        assert_eq!(steps.number(r"\step{c}"), r"\htmlData{step=1}{c}");
    }

    #[test]
    fn other_commands_are_left_alone() {
        let mut steps = Steps::default();
        let tex = r"\stepcounter \\step \{also\} \frac{1}{2} \ \\";
        assert_eq!(steps.number(tex), tex);
    }
}
