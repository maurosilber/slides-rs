//! Steps of a slide, written inside math.
//!
//! `\step{...}` appears one step after the latest one, and `\step[3..5]{...}`
//! or `\step[+0]{...}` in the steps its range says. Each becomes
//! `\htmlData{step=...}{...}`, with its range or its name, or nothing for
//! the next step, as a bare `step` attribute is, which src/step/html.rs
//! numbers among the other steps of the
//! slide, in the order they are written, rather than as KaTeX lays them out:
//! an aligned environment column by column, the left side of every row
//! before any right side. Starred, as `\step*`, they take no space
//! while hidden, and are written with a `collapse` too.

use crate::step::Step;

/// The TeX, with every `\step` written as `\htmlData`.
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
        let starred = name == "step" && after.starts_with('*');
        if starred {
            after = &after[1..];
        }
        let step = match name {
            "step" => match range_argument(after) {
                Some((step, argument_len)) => {
                    after = &after[argument_len..];
                    Some(step.value().unwrap_or_default())
                }
                None => Some(String::new()),
            },
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

/// The range in a `[3..5]`, or the name in an `[a]`, that opens `tex`, and
/// how long it is written. One that is neither is left for KaTeX to show,
/// which points it out.
fn range_argument(tex: &str) -> Option<(Step, usize)> {
    let argument = tex.strip_prefix('[')?;
    let (range, _) = argument.split_once(']')?;
    Some((Step::parse(range)?, range.len() + 2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_are_written_in_the_order_they_are_written() {
        assert_eq!(
            number(r"a \step{b} &\step[+0]{= c} \step[2..4]{d} \also{e}"),
            r"a \htmlData{step=}{b} &\htmlData{step=+0}{= c} \htmlData{step=2..4}{d} \also{e}"
        );
    }

    #[test]
    fn a_step_can_be_named_or_from_a_name() {
        assert_eq!(
            number(r"\step[a]{x} \step[a+1..]{y}"),
            r"\htmlData{step=a}{x} \htmlData{step=a+1}{y}"
        );
    }

    #[test]
    fn any_name_can_be_given() {
        assert_eq!(number(r"\step[next]{x}"), r"\htmlData{step=next}{x}");
    }

    #[test]
    fn a_step_can_be_from_the_one_before_it() {
        assert_eq!(
            number(r"\step[+0]{x} \step*[-1..+1]{y}"),
            r"\htmlData{step=+0}{x} \htmlData{step=-1..+1,collapse=true}{y}"
        );
    }

    #[test]
    fn a_starred_step_collapses() {
        assert_eq!(
            number(r"\step*[..2]{a} \step*{c}"),
            r"\htmlData{step=..2,collapse=true}{a} \htmlData{step=,collapse=true}{c}"
        );
    }

    #[test]
    fn a_range_that_is_not_one_is_left_to_show() {
        assert_eq!(number(r"\step[1..x]{a}"), r"\htmlData{step=}[1..x]{a}");
    }

    #[test]
    fn other_commands_are_left_alone() {
        let tex = r"\stepcounter \\step \{also\} \frac{1}{2} \ \\";
        assert_eq!(number(tex), tex);
    }
}
