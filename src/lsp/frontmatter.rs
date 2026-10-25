//! Completes and describes the keys of a deck's YAML frontmatter, and their
//! values, as the deck's own schema, src/frontmatter.schema.json, has them.
//! VS Code applies a JSON schema to YAML files, but not to the frontmatter of
//! a markdown file.

use std::sync::LazyLock;

use lsp_types::{
    Command, CompletionItem, CompletionItemKind, CompletionTextEdit, Hover, HoverContents,
    InsertTextFormat, MarkupContent, MarkupKind, Position, Range, TextEdit,
};
use serde_json::Value;

use super::{byte, utf16};

/// The schema, which schemars writes from the deck's Rust.
static SCHEMA: LazyLock<Value> =
    LazyLock::new(|| serde_json::from_str(include_str!("../frontmatter.schema.json")).unwrap());

/// The lines of the frontmatter, between the `---` that opens the document
/// and the one that closes it, if the document has one.
fn frontmatter(lines: &[&str]) -> Option<(usize, usize)> {
    if lines.len() < 2 || lines[0].trim_end() != "---" {
        return None;
    }
    (1..lines.len())
        .find(|&line| matches!(lines[line].trim_end(), "---" | "..."))
        .map(|end| (1, end))
}

/// A line that sets a key: its indentation, the key, and what follows the
/// `:`.
fn key(text: &str) -> Option<(usize, &str, &str)> {
    let indent = text.len() - text.trim_start().len();
    let rest = &text[indent..];
    let name = rest.len() - rest.trim_start_matches(is_word).len();
    let after = rest[name..].strip_prefix(':')?;
    (name > 0).then(|| (indent, &rest[..name], after))
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// The keys that hold a line indented `indent`, outermost first, as the lines
/// above it that are indented less say.
fn parents<'a>(lines: &[&'a str], start: usize, line: usize, mut indent: usize) -> Vec<&'a str> {
    let mut keys = Vec::new();
    for above in (start..line).rev() {
        if indent == 0 {
            break;
        }
        if let Some((outer, name, rest)) = key(lines[above])
            && outer < indent
            && rest.trim().is_empty()
        {
            keys.insert(0, name);
            indent = outer;
        }
    }
    keys
}

/// What the schema has for the keys, in turn.
fn at(keys: &[&str]) -> Option<&'static Value> {
    keys.iter()
        .try_fold(&*SCHEMA, |node, key| node.get("properties")?.get(key))
}

fn is_object(node: &Value) -> bool {
    node.get("type").and_then(Value::as_str) == Some("object") || node.get("properties").is_some()
}

/// The types a key can hold, as its own or those of any of its alternatives,
/// but for null, which leaving it unset is.
fn types_of(node: &Value) -> Vec<String> {
    let mut types: Vec<String> = match node.get("type") {
        Some(Value::String(name)) => vec![name.clone()],
        Some(Value::Array(names)) => names
            .iter()
            .filter_map(|name| Some(name.as_str()?.to_string()))
            .collect(),
        _ => Vec::new(),
    };
    for alternative in node
        .get("anyOf")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        types.extend(types_of(alternative));
    }
    let mut seen = Vec::new();
    for name in types {
        if name != "null" && !seen.contains(&name) {
            seen.push(name);
        }
    }
    seen
}

/// A value of the schema as the frontmatter writes it.
fn written(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        value => value.to_string(),
    }
}

fn documentation(key: &str, node: &Value) -> MarkupContent {
    let types = types_of(node).join(" or ");
    let types = if types.is_empty() {
        String::new()
    } else {
        format!(" ({types})")
    };
    let description = node
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut value = format!("**{key}**{types}\n\n{description}");
    if let Some(examples) = node.get("examples").and_then(Value::as_array)
        && !examples.is_empty()
    {
        let examples: Vec<String> = examples
            .iter()
            .map(|example| format!("`{}`", written(example)))
            .collect();
        value.push_str(&format!("\n\nFor example: {}", examples.join(", ")));
    }
    MarkupContent {
        kind: MarkupKind::Markdown,
        value,
    }
}

/// The suggestions after a key has been written, to write the next.
fn suggest() -> Command {
    Command {
        title: "Suggest".to_string(),
        command: "editor.action.triggerSuggest".to_string(),
        arguments: None,
    }
}

pub fn completion(lines: &[&str], position: Position) -> Option<Vec<CompletionItem>> {
    let (start, end) = frontmatter(lines)?;
    let line = position.line as usize;
    if line < start || line >= end {
        return None;
    }
    let text = lines[line];
    let before = &text[..byte(text, position.character)];

    // A value, after its key.
    if let Some((indent, name, rest)) = key(before) {
        let mut keys = parents(lines, start, line, indent);
        keys.push(name);
        let node = at(&keys).filter(|node| !is_object(node))?;
        let mut values = Vec::new();
        if types_of(node).iter().any(|name| name == "boolean") {
            values.extend([Value::Bool(true), Value::Bool(false)]);
        }
        values.extend(
            node.get("examples")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .cloned(),
        );
        let leading = if rest.starts_with(' ') { "" } else { " " };
        let typed = rest.trim_start();
        let range = Range::new(
            Position::new(position.line, utf16(&before[..before.len() - typed.len()])),
            position,
        );
        return Some(
            values
                .iter()
                .enumerate()
                .map(|(i, value)| CompletionItem {
                    label: written(value),
                    kind: Some(CompletionItemKind::VALUE),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(
                        range,
                        format!("{leading}{}", written(value)),
                    ))),
                    documentation: Some(lsp_types::Documentation::MarkupContent(documentation(
                        name, node,
                    ))),
                    sort_text: Some(format!("{i:03}")),
                    ..Default::default()
                })
                .collect(),
        );
    }

    // A key, where one begins, of those its mapping has not set yet.
    let indent = before.len() - before.trim_start().len();
    if !before[indent..].chars().all(is_word) {
        return None;
    }
    let keys = parents(lines, start, line, indent);
    let properties = at(&keys)?.get("properties")?.as_object()?;
    let set: Vec<&str> = (start..end)
        .filter(|&other| other != line)
        .filter_map(|other| {
            let (outer, name, _) = key(lines[other])?;
            (outer == indent && parents(lines, start, other, indent) == keys).then_some(name)
        })
        .collect();
    Some(
        properties
            .iter()
            .filter(|(name, _)| !set.contains(&name.as_str()))
            .map(|(name, node)| {
                let (insert, format) = if is_object(node) {
                    // The keys it holds come next, indented under it, as the
                    // snippet indents its lines as the one it is on, and a tab
                    // as the editor does.
                    (format!("{name}:\n\t$0"), InsertTextFormat::SNIPPET)
                } else {
                    (format!("{name}: "), InsertTextFormat::PLAIN_TEXT)
                };
                CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::PROPERTY),
                    documentation: Some(lsp_types::Documentation::MarkupContent(documentation(
                        name, node,
                    ))),
                    insert_text: Some(insert),
                    insert_text_format: Some(format),
                    command: Some(suggest()),
                    ..Default::default()
                }
            })
            .collect(),
    )
}

pub fn hover(lines: &[&str], position: Position) -> Option<Hover> {
    let (start, end) = frontmatter(lines)?;
    let line = position.line as usize;
    if line < start || line >= end {
        return None;
    }
    let text = lines[line];
    let (indent, name, _) = key(text)?;
    let cursor = byte(text, position.character);
    if cursor < indent || cursor > indent + name.len() {
        return None;
    }
    let mut keys = parents(lines, start, line, indent);
    keys.push(name);
    let node = at(&keys)?;
    Some(Hover {
        contents: HoverContents::Markup(documentation(name, node)),
        range: Some(Range::new(
            Position::new(position.line, utf16(&text[..indent])),
            Position::new(position.line, utf16(&text[..indent + name.len()])),
        )),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRONTMATTER: &str = "---\ntheme: dark\n\nfigures:\n  fade: \n---\n\n# Slide\n";

    fn labels(line: u32, character: u32) -> Vec<String> {
        let lines: Vec<&str> = FRONTMATTER.lines().collect();
        let mut labels: Vec<String> = completion(&lines, Position::new(line, character))
            .unwrap_or_default()
            .into_iter()
            .map(|item| item.label)
            .collect();
        labels.sort();
        labels
    }

    #[test]
    fn keys_not_set_yet_are_completed() {
        assert_eq!(
            labels(2, 0),
            ["aspect-ratio", "deadline", "duration", "steps"]
        );
        assert_eq!(labels(4, 2), ["fade", "invert", "rush"]);
    }

    #[test]
    fn a_boolean_is_completed_with_true_and_false() {
        assert_eq!(labels(4, 8), ["false", "true"]);
    }

    #[test]
    fn past_the_frontmatter_nothing_is_completed() {
        assert!(labels(7, 0).is_empty());
    }

    #[test]
    fn a_key_is_described() {
        let lines: Vec<&str> = FRONTMATTER.lines().collect();
        let hover = hover(&lines, Position::new(4, 3)).unwrap();
        let HoverContents::Markup(markup) = hover.contents else {
            panic!("not markdown");
        };
        assert!(markup.value.starts_with("**fade** (boolean)"));
    }
}
