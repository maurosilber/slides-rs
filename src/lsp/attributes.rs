//! Completes and describes the attributes the deck reads of a heading's
//! `{ … }` and of an HTML element's tag: `steps`, how what a heading holds
//! steps, `step`, the steps an element shows in, and `collapse`, that it takes
//! no space while hidden. A `step`'s value is completed with the names given
//! to steps on the slide, counted from.

use lsp_types::{
    Command, CompletionItem, CompletionItemKind, CompletionTextEdit, Documentation, Hover,
    HoverContents, InsertTextFormat, MarkupContent, MarkupKind, Position, Range, TextEdit,
};

use super::{byte, utf16};

/// A value an attribute takes, and what it does.
struct Value {
    value: String,
    description: String,
}

fn value(value: &str, description: &str) -> Value {
    Value {
        value: value.to_string(),
        description: description.to_string(),
    }
}

/// An attribute the deck reads, and where.
struct Attribute {
    name: &'static str,
    description: &'static str,
    /// Whether it is read of a heading alone.
    heading: bool,
    /// Whether it is written with no value, as `collapse` is.
    bare: bool,
    /// Whether it can be written in a heading's `{ … }`.
    block: bool,
    values: &'static [(&'static str, &'static str)],
}

const ATTRIBUTES: &[Attribute] = &[
    Attribute {
        name: "steps",
        description: "How what the heading holds steps, up to the next heading of its level or above.",
        heading: true,
        bare: false,
        block: true,
        values: &[
            ("false", "It shows all at once, with the heading."),
            (
                "true",
                "It steps, though the file's frontmatter, or a heading above, says `steps: false`.",
            ),
            (
                "parallel",
                "The `###` columns under it step together: the first step of each at once, then the second of each. Inside a column, a range counts that column's steps, its heading 0.",
            ),
            (
                "interleave",
                "The `###` columns under it take turns: the first step of each column, one after another, then the second of each. Inside a column, a range counts that column's steps, its heading 0.",
            ),
        ],
    },
    Attribute {
        name: "step",
        description: "The steps the element shows in: with no value, one after the latest; `3` or `3..` from the third on; `2..4` from the second up to the fourth; `..3` from its own up to the third; `+1` one after the step of the element before it that steps, or of the one it is inside; a name, as `a`, a step of its own named so; and `a+1` one after the step named `a`.",
        heading: false,
        bare: false,
        block: true,
        values: &[],
    },
    Attribute {
        name: "collapse",
        description: "Hidden, the element takes no space, so that two can show in turns in the same place.",
        heading: false,
        bare: true,
        block: false,
        values: &[],
    },
];

impl Attribute {
    fn values(&self) -> Vec<Value> {
        self.values
            .iter()
            .map(|(each, description)| value(each, description))
            .collect()
    }
}

/// Where an attribute is being written: in a heading's `{ … }` or in an HTML
/// tag, of which `tag` is the name, and what of it is written around the
/// cursor.
struct Context<'a> {
    block: bool,
    tag: Option<String>,
    /// What of the block or the tag is written, from its start.
    text: &'a str,
    /// What of the block or the tag is written after the cursor, on its line.
    rest: &'a str,
}

/// How many lines above the cursor a tag may start.
const TAG_LINES: usize = 10;

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// Whether the line is a heading, as up to three spaces, one to six `#` and a
/// space begin it.
fn is_heading(line: &str) -> bool {
    let rest = line.trim_start_matches(' ');
    if line.len() - rest.len() > 3 {
        return false;
    }
    let hashes = rest.len() - rest.trim_start_matches('#').len();
    (1..=6).contains(&hashes) && rest[hashes..].starts_with(char::is_whitespace)
}

/// Where the cursor is, `before` being the text up to it from the start of
/// the line `TAG_LINES` above, and `after` the rest of its line.
fn context_at<'a>(line: &'a str, before: &'a str, after: &'a str) -> Option<Context<'a>> {
    if is_heading(line)
        && let Some(open) = line[..line.len() - after.len()].rfind('{')
    {
        let text = &line[open + 1..line.len() - after.len()];
        if !text.contains('}') {
            let rest = after.split('}').next().unwrap_or("");
            return Some(Context {
                block: true,
                tag: None,
                text,
                rest,
            });
        }
    }
    // A tag, which may span lines, as `<p step="..3"\n  collapse>` does.
    let open = before.rfind('<')?;
    let tag = &before[open + 1..];
    if tag.contains('>') || !tag.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let name = tag.len() - tag.trim_start_matches(is_word).len();
    let text = &tag[name..];
    if !text.starts_with(char::is_whitespace) {
        return None;
    }
    Some(Context {
        block: false,
        tag: Some(tag[..name].to_ascii_lowercase()),
        text,
        rest: after.split('>').next().unwrap_or(""),
    })
}

/// The attributes the deck reads where `context` is.
fn attributes_of(context: &Context) -> impl Iterator<Item = &'static Attribute> {
    let heading = context.tag.as_deref().is_some_and(|tag| {
        tag.len() == 2 && tag.starts_with('h') && matches!(tag.as_bytes()[1], b'1'..=b'6')
    });
    let block = context.block;
    ATTRIBUTES.iter().filter(move |attribute| {
        if block {
            attribute.block
        } else {
            !attribute.heading || heading
        }
    })
}

fn documentation(name: &str, description: &str, value: Option<&Value>) -> MarkupContent {
    let mut text = format!("**{name}**\n\n{description}");
    if let Some(value) = value {
        text.push_str(&format!(
            "\n\n`{name}={}`: {}",
            value.value, value.description
        ));
    }
    MarkupContent {
        kind: MarkupKind::Markdown,
        value: text,
    }
}

/// Whether the line is a rule, which breaks two slides.
fn is_rule(line: &str) -> bool {
    let rest = line.trim_start_matches(' ');
    if line.len() - rest.len() > 3 {
        return false;
    }
    let rest = rest.trim_end();
    rest.len() >= 3
        && ["-", "*", "_"]
            .iter()
            .any(|c| rest.chars().all(|each| each.to_string() == *c))
}

/// The names given to steps on the slide the line is in, between the rules
/// around it, as `step="a"`, `{ step=a }`, `\step[a]` or `gid="step=a"` give
/// them.
fn names_on(lines: &[&str], line: usize) -> Vec<String> {
    let mut first = line;
    while first > 0 && !is_rule(lines[first - 1]) {
        first -= 1;
    }
    let mut last = line;
    while last + 1 < lines.len() && !is_rule(lines[last + 1]) {
        last += 1;
    }
    let text = lines[first..=last].join("\n");
    let mut names = Vec::new();
    for (at, _) in text.match_indices("step") {
        let before = text[..at].chars().next_back();
        let rest = text[at + 4..].strip_prefix('*').unwrap_or(&text[at + 4..]);
        let name = if before == Some('\\') {
            rest.strip_prefix('[').and_then(|rest| {
                let name = name_at(rest)?;
                rest[name.len()..].starts_with(']').then_some(name)
            })
        } else if !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            rest.strip_prefix('=')
                .map(|rest| rest.strip_prefix(['"', '\'']).unwrap_or(rest))
                .and_then(name_at)
        } else {
            None
        };
        if let Some(name) = name
            && !names.iter().any(|each| each == name)
        {
            names.push(name.to_string());
        }
    }
    names
}

/// The name a step is given at the start of `text`, unless it goes on as a
/// count from one, as `a+1` does.
fn name_at(text: &str) -> Option<&str> {
    if !text.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        return None;
    }
    let end = text.len()
        - text
            .trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_')
            .len();
    let next = text[end..].chars().next();
    (!next.is_some_and(|c| c == '+' || c == '-')).then_some(&text[..end])
}

/// The values a `step` is completed with: one after the step before, with it,
/// and with or after each step named on the slide.
fn step_values(lines: &[&str], line: usize) -> Vec<Value> {
    let mut values = vec![
        value(
            "+1",
            "One step after the element before it that steps, or the one it is inside.",
        ),
        value(
            "+0",
            "With the element before it that steps, or the one it is inside.",
        ),
    ];
    for name in names_on(lines, line) {
        values.push(value(
            &format!("{name}+0"),
            &format!("With the step named `{name}`."),
        ));
        values.push(value(
            &format!("{name}+1"),
            &format!("One step after the step named `{name}`."),
        ));
    }
    values
}

/// The text up to the cursor from the line `TAG_LINES` above it, and the rest
/// of its line.
fn around<'a>(lines: &[&'a str], line: usize, cursor: usize) -> (String, &'a str) {
    let first = line.saturating_sub(TAG_LINES);
    let mut before = lines[first..line]
        .iter()
        .map(|line| format!("{line}\n"))
        .collect::<String>();
    before.push_str(&lines[line][..cursor]);
    (before, &lines[line][cursor..])
}

/// The value of an attribute being written at the end of `text`: the
/// attribute, the quote it opens, and what of the value is written.
fn value_at(text: &str) -> Option<(&str, &str, &str)> {
    let separates = |c: char| c.is_whitespace() || c == '"' || c == '\'';
    for (equals, _) in text.rmatch_indices('=') {
        let after = &text[equals + 1..];
        let (quote, typed) = match after.strip_prefix(['"', '\'']) {
            Some(typed) => (&after[..1], typed),
            None => ("", after),
        };
        if typed.contains(separates) {
            return None;
        }
        let before = &text[..equals];
        let name = before.len() - before.trim_end_matches(is_word).len();
        let preceded = before[..before.len() - name].chars().next_back();
        if name > 0 && !preceded.is_some_and(|c| !c.is_whitespace()) {
            return Some((&before[before.len() - name..], quote, typed));
        }
    }
    None
}

/// The attribute name being written at the end of `text`, where one begins.
fn name_being_written(text: &str) -> Option<&str> {
    let start = text.trim_end_matches(is_word).len();
    let preceded = text[..start].chars().next_back();
    (!preceded.is_some_and(|c| !c.is_whitespace())).then_some(&text[start..])
}

/// The attributes written in `text`, each where one begins and followed by
/// its `=`, a space or the end.
fn written(text: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let mut rest = text;
    let mut preceded = None;
    while !rest.is_empty() {
        let name = rest.len() - rest.trim_start_matches(is_word).len();
        if name > 0 {
            let next = rest[name..].chars().next();
            let begins = !preceded.is_some_and(|c: char| !c.is_whitespace());
            if begins && next.is_none_or(|c| c == '=' || c.is_whitespace()) {
                names.push(&rest[..name]);
            }
            preceded = rest[..name].chars().next_back();
            rest = &rest[name..];
        } else {
            let c = rest.chars().next().unwrap();
            preceded = Some(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    names
}

pub fn completion(lines: &[&str], position: Position) -> Option<Vec<CompletionItem>> {
    let line = position.line as usize;
    let cursor = byte(lines[line], position.character);
    let (before, after) = around(lines, line, cursor);
    let context = context_at(lines[line], &before, after)?;
    let back = |typed: &str| Position::new(position.line, position.character - utf16(typed));

    // A value, after its attribute's `=`, and its quote in a tag.
    if let Some((name, quote, typed)) = value_at(context.text) {
        let attribute = attributes_of(&context).find(|attribute| attribute.name == name)?;
        let values = if name == "step" {
            step_values(lines, line)
        } else {
            attribute.values()
        };
        let range = Range::new(back(typed), position);
        // In a tag, a value is quoted, which is closed if it is not yet.
        let mark = if context.block || !quote.is_empty() {
            ""
        } else {
            "\""
        };
        return Some(
            values
                .iter()
                .enumerate()
                .map(|(i, each)| CompletionItem {
                    label: each.value.clone(),
                    kind: Some(CompletionItemKind::ENUM_MEMBER),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(
                        range,
                        format!("{mark}{}{mark}", each.value),
                    ))),
                    filter_text: Some(format!("{mark}{}", each.value)),
                    documentation: Some(Documentation::MarkupContent(documentation(
                        name,
                        attribute.description,
                        Some(each),
                    ))),
                    sort_text: Some(format!("{i:03}")),
                    ..Default::default()
                })
                .collect(),
        );
    }

    // An attribute, where one begins, of those not written yet.
    let typed = name_being_written(context.text)?;
    let all = format!("{} {}", context.text, context.rest);
    let mut written = written(&all);
    if let Some(at) = written.iter().position(|name| *name == typed) {
        written.remove(at);
    }
    let range = Range::new(back(typed), position);
    Some(
        attributes_of(&context)
            .filter(|attribute| !written.contains(&attribute.name))
            .map(|attribute| {
                let (insert, format, command) = if attribute.bare {
                    (
                        attribute.name.to_string(),
                        InsertTextFormat::PLAIN_TEXT,
                        None,
                    )
                } else {
                    let insert = if context.block {
                        format!("{}=$0", attribute.name)
                    } else {
                        format!("{}=\"$0\"", attribute.name)
                    };
                    let suggest = Command {
                        title: "Suggest".to_string(),
                        command: "editor.action.triggerSuggest".to_string(),
                        arguments: None,
                    };
                    (insert, InsertTextFormat::SNIPPET, Some(suggest))
                };
                CompletionItem {
                    label: attribute.name.to_string(),
                    kind: Some(CompletionItemKind::PROPERTY),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(range, insert))),
                    insert_text_format: Some(format),
                    documentation: Some(Documentation::MarkupContent(documentation(
                        attribute.name,
                        attribute.description,
                        None,
                    ))),
                    command,
                    ..Default::default()
                }
            })
            .collect(),
    )
}

pub fn hover(lines: &[&str], position: Position) -> Option<Hover> {
    let line = position.line as usize;
    let text = lines[line];
    let cursor = byte(text, position.character);
    // The word the cursor is in, or at either end of.
    let start = text[..cursor].trim_end_matches(is_word).len();
    let end = cursor + (text.len() - cursor - text[cursor..].trim_start_matches(is_word).len());
    if start == end {
        return None;
    }
    let name = &text[start..end];
    let (before, after) = around(lines, line, end);
    let context = context_at(text, &before, after)?;
    if name_being_written(context.text).is_none_or(str::is_empty) {
        return None;
    }
    let attribute = attributes_of(&context).find(|attribute| attribute.name == name)?;
    let written = after
        .strip_prefix('=')
        .map(|rest| rest.strip_prefix(['"', '\'']).unwrap_or(rest))
        .map(|rest| &rest[..rest.len() - rest.trim_start_matches(is_word).len()]);
    let values = attribute.values();
    let value = values
        .iter()
        .find(|each| Some(each.value.as_str()) == written);
    Some(Hover {
        contents: HoverContents::Markup(documentation(name, attribute.description, value)),
        range: Some(Range::new(
            Position::new(position.line, utf16(&text[..start])),
            Position::new(position.line, utf16(&text[..end])),
        )),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINES: &[&str] = &[
        "# Slide { steps= }",
        "",
        "### Left { step=a }",
        "",
        "<p step=\"\" >x</p>",
        "",
        "<h2 >y</h2>",
        "",
        "## Plain { #id }",
        "",
        "---",
        "",
        "<p step=\"\">z</p>",
    ];

    fn labels(line: usize, after: &str) -> Vec<String> {
        let character = (LINES[line].find(after).unwrap() + after.len()) as u32;
        let mut labels: Vec<String> = completion(LINES, Position::new(line as u32, character))
            .unwrap_or_default()
            .into_iter()
            .map(|item| item.label)
            .collect();
        labels.sort();
        labels
    }

    #[test]
    fn a_heading_s_steps_is_completed_with_its_values() {
        assert_eq!(
            labels(0, "steps="),
            ["false", "interleave", "parallel", "true"]
        );
    }

    #[test]
    fn a_block_completes_the_attributes_not_written_yet() {
        assert_eq!(labels(8, "{ "), ["step", "steps"]);
    }

    #[test]
    fn a_step_counts_from_the_names_on_its_slide() {
        assert_eq!(labels(4, "step=\""), ["+0", "+1", "a+0", "a+1"]);
        assert_eq!(labels(12, "step=\""), ["+0", "+1"]);
    }

    #[test]
    fn steps_is_completed_in_the_tag_of_a_heading_alone() {
        assert_eq!(labels(4, "\"\" "), ["collapse"]);
        assert_eq!(labels(6, "<h2 "), ["collapse", "step", "steps"]);
    }

    #[test]
    fn outside_a_block_or_a_tag_nothing_is_completed() {
        assert!(labels(4, ">x").is_empty());
    }

    #[test]
    fn an_attribute_is_described() {
        let character = LINES[0].find("step").unwrap() as u32 + 4;
        let hover = hover(LINES, Position::new(0, character)).unwrap();
        let HoverContents::Markup(markup) = hover.contents else {
            panic!("not markdown");
        };
        assert!(markup.value.starts_with("**steps**"));
    }
}
