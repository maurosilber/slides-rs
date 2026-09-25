//! Steps of the fragment animation, written inside math.
//!
//! `\step{...}` appears one step after the ones before it, and `\also{...}`
//! along with the last of them. Both become `\fragment{n}{...}`, which the
//! page hands to KaTeX, numbered here in the order they are written: KaTeX
//! lays out an aligned environment column by column, so by the time the page
//! sees them, the left side of every row comes before any right side.

/// Counts the steps of one column of a slide, as its numbers start over at
/// every heading, the way the slides count them.
#[derive(Default)]
pub struct Steps {
    last: u32,
}

impl Steps {
    /// Starts counting a new column, or a new slide.
    pub fn reset(&mut self) {
        self.last = 0;
    }

    /// The TeX, with every `\step` and `\also` numbered as a `\fragment`.
    /// An explicit `\fragment{n}` is kept, and counts as step n.
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
            let (name, after) = command.split_at(name_len);
            match name {
                "step" => {
                    self.last += 1;
                    numbered.push_str(&format!("\\fragment{{{}}}", self.last));
                }
                "also" => {
                    self.last = self.last.max(1);
                    numbered.push_str(&format!("\\fragment{{{}}}", self.last));
                }
                "fragment" => {
                    if let Some(n) = number_argument(after) {
                        self.last = self.last.max(n);
                    }
                    numbered.push_str("\\fragment");
                }
                _ => {
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

/// The number in a `{n}` that opens `tex`.
fn number_argument(tex: &str) -> Option<u32> {
    let argument = tex.trim_start().strip_prefix('{')?;
    let (number, _) = argument.split_once('}')?;
    number.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_are_numbered_in_the_order_they_are_written() {
        let mut steps = Steps::default();
        assert_eq!(
            steps.number(r"a \step{b} \step{c}"),
            r"a \fragment{1}{b} \fragment{2}{c}"
        );
        // The count goes on in the next expression of the same column.
        assert_eq!(steps.number(r"\step{d}"), r"\fragment{3}{d}");
    }

    #[test]
    fn also_joins_the_last_step() {
        let mut steps = Steps::default();
        let tex = r"a &= b \\ \step{c} &\also{= d} \\ \step{e} &\also{= f}";
        assert_eq!(
            steps.number(tex),
            r"a &= b \\ \fragment{1}{c} &\fragment{1}{= d} \\ \fragment{2}{e} &\fragment{2}{= f}"
        );
    }

    #[test]
    fn an_explicit_fragment_counts_as_its_step() {
        let mut steps = Steps::default();
        assert_eq!(
            steps.number(r"\fragment{3}{a} \step{b}"),
            r"\fragment{3}{a} \fragment{4}{b}"
        );
    }

    #[test]
    fn a_heading_starts_the_count_over() {
        let mut steps = Steps::default();
        steps.number(r"\step{a} \step{b}");
        steps.reset();
        assert_eq!(steps.number(r"\step{c}"), r"\fragment{1}{c}");
    }

    #[test]
    fn other_commands_are_left_alone() {
        let mut steps = Steps::default();
        let tex = r"\stepcounter \\step \{also\} \frac{1}{2} \ \\";
        assert_eq!(steps.number(tex), tex);
    }
}
