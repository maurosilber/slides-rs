//! Steps of a slide, written inside math.
//!
//! `\step{...}` appears one step after the latest one, `\also{...}` along
//! with it, and `\step[3..5]{...}` in the steps its range says. Each becomes
//! `\htmlData{step=...}{...}`, with `next`, `also` or its range, which
//! src/step/html.rs numbers among the other steps of the slide, in the order
//! they are written, rather than as KaTeX lays them out: an aligned
//! environment column by column, the left side of every row before any right
//! side. Starred, as `\step*` or `\also*`, they take no space while hidden,
//! and are written with a `collapse` too.

use crate::step::Range;

/// The TeX, with every `\step` and `\also` written as `\htmlData`.
pub fn number(tex: &str) -> String {
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
        let step = match name {
            "step" => match range_argument(after) {
                Some((range, argument_len)) => {
                    after = &after[argument_len..];
                    Some(range.to_string())
                }
                None => Some("next".to_string()),
            },
            "also" => Some("also".to_string()),
            _ => None,
        };
        match step {
            Some(step) => {
                let collapse = if starred { ",collapse=true" } else { "" };
                numbered.push_str(&format!("\\htmlData{{step={step}{collapse}}}"));
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
    fn steps_are_written_in_the_order_they_are_written() {
        assert_eq!(
            number(r"a \step{b} &\also{= c} \step[2..4]{d}"),
            r"a \htmlData{step=next}{b} &\htmlData{step=also}{= c} \htmlData{step=2..4}{d}"
        );
    }

    #[test]
    fn a_starred_step_collapses() {
        assert_eq!(
            number(r"\step*[..2]{a} \also*{b} \step*{c}"),
            r"\htmlData{step=..2,collapse=true}{a} \htmlData{step=also,collapse=true}{b} \htmlData{step=next,collapse=true}{c}"
        );
    }

    #[test]
    fn a_range_that_is_not_one_is_left_to_show() {
        assert_eq!(number(r"\step[x]{a}"), r"\htmlData{step=next}[x]{a}");
    }

    #[test]
    fn other_commands_are_left_alone() {
        let tex = r"\stepcounter \\step \{also\} \frac{1}{2} \ \\";
        assert_eq!(number(tex), tex);
    }
}
