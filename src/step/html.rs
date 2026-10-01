//! The steps of the slides, numbered in their html, for slides.css to show
//! and hide as the page says which step it is at.
//!
//! A slide is revealed in steps, numbered from 1, the step it opens with.
//! Each h2 and h3 starts a column, a step of its own. After each column, and
//! before the first one, come the steps of the elements it marks: every list
//! item, the elements with a `step`, an `also` or a `data-step`, as raw html
//! and an SVG's are, and the steps of its math, as `\step{...}` writes them.
//! Every number the column's ranges start or end at is a step, in order, and
//! an element shows from the step its range starts at, or its column's, up
//! to the one it ends at. The first step is what the slide opens with: the
//! first column, unless a marked element comes before it.
//!
//! A heading with `steps="false"`, as `{ steps=false }` writes, shows
//! everything under it at once, up to the next heading of its level or above:
//! the elements it marks, and the columns it holds, which join the step before
//! them. `steps="true"` steps through them again. Outside every such heading,
//! the slide's `data-steps` decides, from its file's frontmatter.
//!
//! Each element that steps is given the class `step`, and `step-collapse` if
//! it takes no space while hidden, with the steps it shows in as
//! `--from` and `--to`, excluded, in its style; each slide is given how many
//! steps it has, as `data-count`. A step of math is written as
//! `\htmlStyle{--from:..}{\htmlClass{step}{...}}`, for KaTeX to write the same.

use std::collections::HashMap;
use std::ops::Range as Span;

use super::{Mark, Range, Step};

/// The html with the steps of every `<section>` numbered.
pub fn number(html: &str) -> String {
    let elements = elements(html);
    let mut edits = Vec::new();
    for (index, element) in elements.iter().enumerate() {
        if element.parent.is_some() || element.name != "section" {
            continue;
        }
        let children: Vec<usize> = (index + 1..element.end)
            .filter(|&child| elements[child].parent == Some(index))
            .collect();
        let steps = element.attribute(html, "data-steps") != Some("false");
        let count = number_slide(html, &elements, &children, steps, &mut edits);
        edits.push((
            element.close..element.close,
            format!(" data-count=\"{count}\""),
        ));
    }
    apply(html, edits)
}

/// A figure with its steps numbered, as a slide that holds it alone numbers
/// them, as a notebook shows it.
pub fn number_figure(svg: &str) -> String {
    let elements = elements(svg);
    let children: Vec<usize> = (0..elements.len())
        .filter(|&child| elements[child].parent.is_none())
        .collect();
    let mut edits = Vec::new();
    number_slide(svg, &elements, &children, true, &mut edits);
    apply(svg, edits)
}

/// A figure as it was before `number_figure` numbered it, as it is saved.
pub fn unnumber_figure(svg: &str) -> String {
    let elements = elements(svg);
    let mut edits = Vec::new();
    for element in &elements {
        if element_mark(svg, element).is_none() {
            continue;
        }
        let classes = ["step step-collapse", "step"];
        if let Some(class) = element.find("class") {
            unappend(
                svg,
                class,
                &classes.map(|class| format!(" {class}")),
                &classes,
                &mut edits,
            );
        }
        if let Some(style) = element.find("style")
            && let Some(value) = style.value.clone()
        {
            let value = &svg[value];
            let vars = value.rfind("--from:").map(|start| &value[start..]);
            if let Some(vars) = vars.filter(|vars| is_vars(vars)) {
                unappend(svg, style, &[format!(";{vars}")], &[vars], &mut edits);
            }
        }
    }
    apply(svg, edits)
}

/// Takes out of an attribute's value one of the `suffixes` it was appended
/// with, or the whole attribute, if it is one of the `values` it was added as.
fn unappend(
    html: &str,
    attribute: &Attribute,
    suffixes: &[String],
    values: &[&str],
    edits: &mut Vec<Edit>,
) {
    let Some(span) = attribute.value.clone() else {
        return;
    };
    let value = &html[span.clone()];
    if values.contains(&value) {
        // Added with a space before it, and quoted.
        edits.push((attribute.name.start - 1..span.end + 1, String::new()));
    } else if let Some(suffix) = suffixes
        .iter()
        .find(|suffix| value.ends_with(suffix.as_str()))
    {
        edits.push((span.end - suffix.len()..span.end, String::new()));
    }
}

/// Whether `text` is the steps of an element, as `--from:2;--to:5`.
fn is_vars(text: &str) -> bool {
    let number = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    let Some(rest) = text.strip_prefix("--from:") else {
        return false;
    };
    match rest.split_once(";--to:") {
        Some((from, to)) => number(from) && number(to),
        None => number(rest),
    }
}

/// What to write in place of a span of the html.
type Edit = (Span<usize>, String);

/// The html with each edit made, none of which overlap.
fn apply(html: &str, mut edits: Vec<Edit>) -> String {
    edits.sort_by_key(|(span, _)| span.start);
    let mut edited = String::with_capacity(html.len() + edits.len() * 24);
    let mut at = 0;
    for (span, text) in edits {
        edited.push_str(&html[at..span.start]);
        edited.push_str(&text);
        at = span.end;
    }
    edited.push_str(&html[at..]);
    edited
}

/// What an element or a step of math is shown in, numbered.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Shown {
    from: u32,
    to: Option<u32>,
    collapse: bool,
}

/// What steps: an element, by its index, or a step of math, by where it is.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Target {
    Element(usize),
    Math {
        span: Span<usize>,
        argument: Span<usize>,
    },
}

/// Numbers the steps of a slide made of `children`, stepping through them as
/// `steps` says outside every heading that says otherwise. Returns how many
/// steps it has.
fn number_slide(
    html: &str,
    elements: &[Element],
    children: &[usize],
    steps: bool,
    edits: &mut Vec<Edit>,
) -> u32 {
    struct Group {
        step: bool,
        children: Vec<(usize, bool)>,
    }
    // The headings whose part of the slide the current child is in, by level,
    // each with whether steps are on there.
    let mut scopes: Vec<(u8, bool)> = Vec::new();
    let on = |scopes: &[(u8, bool)]| scopes.last().map_or(steps, |&(_, on)| on);
    let mut groups = vec![Group {
        step: true,
        children: Vec::new(),
    }];
    for &child in children {
        let element = &elements[child];
        if let Some(level) = heading_level(&element.name) {
            while scopes.last().is_some_and(|&(scope, _)| scope >= level) {
                scopes.pop();
            }
            // Whether a column is a step of its own is up to the part of
            // the slide it is in, and what it holds is up to its heading.
            if level == 2 || level == 3 {
                groups.push(Group {
                    step: on(&scopes),
                    children: Vec::new(),
                });
            }
            let here = match element.attribute(html, "steps") {
                Some(value) => value != "false",
                None => on(&scopes),
            };
            scopes.push((level, here));
        }
        groups
            .last_mut()
            .unwrap()
            .children
            .push((child, on(&scopes)));
    }

    // Later entries win, as an element's own range wins over its column's.
    let mut shown: Vec<(Target, Shown)> = Vec::new();
    let mut count = 1;
    for (i, group) in groups.iter().enumerate() {
        // What comes before the first heading is always shown.
        let mut column = 1;
        if i > 0 {
            if !((i == 1 && count == 1) || !group.step) {
                count += 1;
            }
            column = count;
            for &(child, _) in &group.children {
                let column = Shown {
                    from: column,
                    to: None,
                    collapse: false,
                };
                shown.push((Target::Element(child), column));
            }
        }
        let mut parts = Vec::new();
        let mut latest = 0;
        for &(child, on) in &group.children {
            // Unmarked, the element's parts show along with it.
            if !on {
                continue;
            }
            for (target, mark) in parts_of(html, elements, child) {
                let range = match mark.step {
                    Step::Range(range) => range,
                    Step::Next => Range {
                        start: Some(latest + 1),
                        end: None,
                    },
                    Step::Also => Range {
                        start: Some(latest.max(1)),
                        end: None,
                    },
                };
                if let Some(start) = range.start {
                    latest = latest.max(start);
                }
                parts.push((target, range, mark.collapse));
            }
        }
        let mut bounds: Vec<u32> = parts
            .iter()
            .flat_map(|(_, range, _)| [range.start, range.end])
            .flatten()
            .collect();
        bounds.sort_unstable();
        bounds.dedup();
        let step = |bound: u32| count + 1 + bounds.binary_search(&bound).unwrap() as u32;
        for (target, range, collapse) in parts {
            let from = range.start.map_or(column, step);
            let to = range.end.map(step);
            shown.push((target, Shown { from, to, collapse }));
        }
        count += bounds.len() as u32;
    }

    let mut targets: HashMap<Target, Shown> = HashMap::new();
    for (target, steps) in shown {
        targets.insert(target, steps);
    }
    for (target, shown) in targets {
        let classes = if shown.collapse {
            "step step-collapse"
        } else {
            "step"
        };
        let mut vars = format!("--from:{}", shown.from);
        if let Some(to) = shown.to {
            vars.push_str(&format!(";--to:{to}"));
        }
        match target {
            Target::Element(index) => {
                let element = &elements[index];
                append(html, element, "class", " ", classes, edits);
                append(html, element, "style", ";", &vars, edits);
            }
            Target::Math { span, argument } => {
                let argument = &html[argument];
                let tex =
                    format!("\\htmlStyle{{{vars}}}{{\\htmlClass{{{classes}}}{{{argument}}}}}");
                edits.push((span, tex));
            }
        }
    }
    count
}

/// Appends `value` to an attribute of `element`, after `separator`, or adds
/// the attribute, if it has none.
fn append(
    html: &str,
    element: &Element,
    name: &str,
    separator: &str,
    value: &str,
    edits: &mut Vec<Edit>,
) {
    match element
        .find(name)
        .and_then(|attribute| attribute.value.clone())
    {
        Some(span) if element.quoted(html, name) => {
            edits.push((span.end..span.end, format!("{separator}{value}")));
        }
        // Unquoted, it is quoted, to hold the separator.
        Some(span) => {
            let old = &html[span.clone()];
            edits.push((span, format!("\"{old}{separator}{value}\"")));
        }
        None => {
            let at = element.close;
            edits.push((at..at, format!(" {name}=\"{value}\"")));
        }
    }
}

/// The level of a heading, from its name.
fn heading_level(name: &str) -> Option<u8> {
    match name.as_bytes() {
        [b'h', level @ b'1'..=b'6'] => Some(level - b'0'),
        _ => None,
    }
}

/// The steps marked within `index`, in order: the element itself, if it is
/// marked, and its elements and its math.
fn parts_of(html: &str, elements: &[Element], index: usize) -> Vec<(Target, Mark)> {
    let mut parts = Vec::new();
    for (index, element) in elements
        .iter()
        .enumerate()
        .take(elements[index].end)
        .skip(index)
    {
        if let Some(mark) = element_mark(html, element) {
            parts.push((Target::Element(index), mark));
        }
        if element.has_class(html, "math")
            && let Some(text) = element.text.clone()
        {
            parts.extend(math_marks(html, text));
        }
    }
    parts
}

/// How an element marks itself as a step, if it does: by `also`, by the range
/// its `step` or `data-step` says, or as the next step, as a list item does.
fn element_mark(html: &str, element: &Element) -> Option<Mark> {
    let collapse = element.find("collapse").is_some() || element.find("data-collapse").is_some();
    let range = element
        .attribute(html, "step")
        .or_else(|| element.attribute(html, "data-step"));
    let step = if element.find("also").is_some() {
        Step::Also
    } else if let Some(range) = range.and_then(Range::parse) {
        Step::Range(range)
    } else if element.name == "li"
        || element.find("step").is_some()
        || element.find("data-step").is_some()
    {
        Step::Next
    } else {
        return None;
    };
    Some(Mark { step, collapse })
}

/// The steps written in the TeX of math, as `\htmlData{step=2..4}{...}`, as
/// src/markdown/math.rs writes them, each with where it is and its argument.
fn math_marks(html: &str, text: Span<usize>) -> Vec<(Target, Mark)> {
    const MARK: &str = "\\htmlData{step=";
    let mut marks = Vec::new();
    let mut at = text.start;
    while let Some(found) = html[at..text.end].find(MARK) {
        let start = at + found;
        at = start + MARK.len();
        let Some(close) = html[at..text.end].find('}') else {
            break;
        };
        let written = &html[at..at + close];
        let (range, collapse) = match written.strip_suffix(",collapse=true") {
            Some(range) => (range, true),
            None => (written, false),
        };
        let Some(range) = Range::parse(range) else {
            continue;
        };
        at += close + 1;
        let (argument, end) = tex_argument(html, at..text.end);
        marks.push((
            Target::Math {
                span: start..end,
                argument,
            },
            Mark {
                step: Step::Range(range),
                collapse,
            },
        ));
        at = end;
    }
    marks
}

/// The argument of a TeX command that begins in `span`: a group, without its
/// braces, or the one token there, and where it ends.
fn tex_argument(html: &str, span: Span<usize>) -> (Span<usize>, usize) {
    let text = &html[span.clone()];
    let start = span.start + (text.len() - text.trim_start().len());
    let mut chars = html[start..span.end].char_indices();
    match chars.next() {
        Some((_, '{')) => {
            let mut depth = 1;
            while let Some((i, char)) = chars.next() {
                match char {
                    '\\' => {
                        chars.next();
                    }
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            return (start + 1..start + i, start + i + 1);
                        }
                    }
                    _ => {}
                }
            }
            (start + 1..span.end, span.end)
        }
        Some((_, '\\')) => {
            let name = html[start + 1..span.end]
                .find(|char: char| !char.is_ascii_alphabetic())
                .unwrap_or(span.end - start - 1);
            // A control symbol, as `\,`, is one character long.
            let len = match name {
                0 => html[start + 1..span.end]
                    .chars()
                    .next()
                    .map_or(0, char::len_utf8),
                name => name,
            };
            (start..start + 1 + len, start + 1 + len)
        }
        Some((_, char)) => (start..start + char.len_utf8(), start + char.len_utf8()),
        None => (start..start, start),
    }
}

/// An attribute of a start tag: its name, in lower case, and where its
/// name and its value are.
struct Attribute {
    key: String,
    name: Span<usize>,
    value: Option<Span<usize>>,
}

/// An element of the html, in the order its start tags come in.
struct Element {
    /// Its name, in lower case.
    name: String,
    attributes: Vec<Attribute>,
    /// Where its start tag closes, with `>` or `/>`, where attributes go.
    close: usize,
    /// The element it is in.
    parent: Option<usize>,
    /// The index of the element after it and all it holds.
    end: usize,
    /// What it holds, from its start tag to its end tag.
    text: Option<Span<usize>>,
}

impl Element {
    fn find(&self, name: &str) -> Option<&Attribute> {
        self.attributes
            .iter()
            .find(|attribute| attribute.key == name)
    }

    /// The value of an attribute, or an empty one, if it has it.
    fn attribute<'a>(&self, html: &'a str, name: &str) -> Option<&'a str> {
        let attribute = self.find(name)?;
        Some(attribute.value.clone().map_or("", |value| &html[value]))
    }

    fn quoted(&self, html: &str, name: &str) -> bool {
        let value = self
            .find(name)
            .and_then(|attribute| attribute.value.clone());
        value.is_some_and(|value| matches!(html.as_bytes().get(value.end), Some(b'"' | b'\'')))
    }

    fn has_class(&self, html: &str, class: &str) -> bool {
        self.attribute(html, "class")
            .is_some_and(|classes| classes.split_ascii_whitespace().any(|name| name == class))
    }
}

/// Elements that are never closed.
const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];

/// Elements whose text is not markup.
const RAW: &[&str] = &["script", "style", "textarea", "title"];

/// The elements of the html, in order. It reads html as written by hand, as
/// well as by the markdown: an end tag closes every element opened after the
/// one it names, and one that names none is left out.
fn elements(html: &str) -> Vec<Element> {
    let mut elements: Vec<Element> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut at = 0;
    while let Some(found) = html[at..].find('<') {
        let start = at + found;
        let rest = &html[start..];
        let skip = |end: &str| {
            rest.find(end)
                .map_or(html.len(), |found| start + found + end.len())
        };
        if rest.starts_with("<!--") {
            at = skip("-->");
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            at = skip("]]>");
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            at = skip(">");
            continue;
        }
        if let Some(name) = rest.strip_prefix("</") {
            at = skip(">");
            if !name.starts_with(|char: char| char.is_ascii_alphabetic()) {
                continue;
            }
            let name = name[..name_length(name)].to_ascii_lowercase();
            if let Some(depth) = open.iter().rposition(|&open| elements[open].name == name) {
                let count = elements.len();
                for &closed in &open[depth..] {
                    let element = &mut elements[closed];
                    element.end = count;
                    let from = element.text.as_ref().map_or(start, |text| text.start);
                    element.text = Some(from..start);
                }
                open.truncate(depth);
            }
            continue;
        }
        if !rest[1..].starts_with(|char: char| char.is_ascii_alphabetic()) {
            at = start + 1;
            continue;
        }
        let tag = start_tag(html, start);
        let index = elements.len();
        let name = tag.name.clone();
        elements.push(Element {
            name: tag.name,
            attributes: tag.attributes,
            close: tag.close,
            parent: open.last().copied(),
            end: index + 1,
            text: Some(tag.after..tag.after),
        });
        at = tag.after;
        if tag.self_closing || VOID.contains(&name.as_str()) {
            continue;
        }
        if RAW.contains(&name.as_str()) {
            let lower = html[at..].to_ascii_lowercase();
            at = lower
                .find(&format!("</{name}"))
                .map_or(html.len(), |found| at + found);
        }
        open.push(index);
    }
    let count = elements.len();
    for open in open {
        let element = &mut elements[open];
        element.end = count;
        let from = element.text.as_ref().map_or(html.len(), |text| text.start);
        element.text = Some(from..html.len());
    }
    elements
}

/// How long the name that `text` starts with is.
fn name_length(text: &str) -> usize {
    text.find(|char: char| char.is_ascii_whitespace() || char == '/' || char == '>')
        .unwrap_or(text.len())
}

/// A start tag, read.
struct StartTag {
    /// Its name, in lower case.
    name: String,
    attributes: Vec<Attribute>,
    close: usize,
    self_closing: bool,
    /// Where what it holds begins.
    after: usize,
}

/// Reads the start tag at `start`.
fn start_tag(html: &str, start: usize) -> StartTag {
    let bytes = html.as_bytes();
    let source = start + 1;
    let mut at = source + name_length(&html[source..]);
    let name = html[source..at].to_ascii_lowercase();
    let mut attributes = Vec::new();
    let (close, self_closing, after) = loop {
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        match bytes.get(at) {
            None => break (html.len(), false, html.len()),
            Some(b'>') => break (at, false, at + 1),
            Some(b'/') if bytes.get(at + 1) == Some(&b'>') => break (at, true, at + 2),
            Some(b'/') => {
                at += 1;
                continue;
            }
            Some(_) => {}
        }
        let name_start = at;
        while bytes
            .get(at)
            .is_some_and(|&byte| !byte.is_ascii_whitespace() && !matches!(byte, b'/' | b'>' | b'='))
        {
            at += 1;
        }
        let name = name_start..at.max(name_start + 1);
        at = name.end;
        let mut equals = at;
        while bytes.get(equals).is_some_and(u8::is_ascii_whitespace) {
            equals += 1;
        }
        let mut value = None;
        if bytes.get(equals) == Some(&b'=') {
            at = equals + 1;
            while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
                at += 1;
            }
            match bytes.get(at) {
                Some(&quote @ (b'"' | b'\'')) => {
                    let end = html[at + 1..]
                        .find(quote as char)
                        .map_or(html.len(), |found| at + 1 + found);
                    value = Some(at + 1..end);
                    at = (end + 1).min(html.len());
                }
                _ => {
                    let end = html[at..]
                        .find(|char: char| char.is_ascii_whitespace() || char == '>')
                        .map_or(html.len(), |found| at + found);
                    value = Some(at..end);
                    at = end;
                }
            }
        }
        let key = html[name.clone()].to_ascii_lowercase();
        attributes.push(Attribute { key, name, value });
    };
    StartTag {
        name,
        attributes,
        close,
        self_closing,
        after,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The steps each element shows in, as `name from..to`, with a `*` if it
    /// collapses, in order, and how many steps each slide has.
    fn steps(html: &str) -> Vec<String> {
        let numbered = number(html);
        let elements = elements(&numbered);
        let mut steps = Vec::new();
        for element in &elements {
            if element.name == "section" {
                steps.push(format!(
                    "count {}",
                    element.attribute(&numbered, "data-count").unwrap()
                ));
            }
            if !element.has_class(&numbered, "step") {
                continue;
            }
            let style = element.attribute(&numbered, "style").unwrap();
            let vars = &style[style.find("--from:").unwrap()..];
            let collapse = if element.has_class(&numbered, "step-collapse") {
                "*"
            } else {
                ""
            };
            steps.push(format!("{}{collapse} {vars}", element.name));
        }
        steps
    }

    #[test]
    fn list_items_step_one_after_another() {
        let html = "<section>\n<h1>Title</h1>\n<ul>\n<li>a</li>\n<li>b</li>\n</ul>\n</section>\n";
        assert_eq!(steps(html), ["count 3", "li --from:2", "li --from:3"]);
        assert_eq!(
            number(html),
            "<section data-count=\"3\">\n<h1>Title</h1>\n<ul>\n<li class=\"step\" style=\"--from:2\">a</li>\n<li class=\"step\" style=\"--from:3\">b</li>\n</ul>\n</section>\n"
        );
    }

    #[test]
    fn each_column_is_a_step_followed_by_its_own() {
        let html = "<section><h1>T</h1><h2>A</h2><ul><li>a</li></ul><h2>B</h2><p>b</p></section>";
        assert_eq!(
            steps(html),
            [
                "count 3",
                "h2 --from:1",
                "ul --from:1",
                "li --from:2",
                "h2 --from:3",
                "p --from:3",
            ]
        );
    }

    #[test]
    fn a_marked_element_before_the_first_column_comes_first() {
        let html = "<section><p step>a</p><h3>A</h3><p>b</p></section>";
        assert_eq!(
            steps(html),
            ["count 3", "p --from:2", "h3 --from:3", "p --from:3"]
        );
    }

    #[test]
    fn ranges_are_the_steps_their_bounds_are() {
        let html = "<section><p step=\"2..4\">a</p><p also>b</p><p step=\"..2\" collapse>c</p><p step>d</p></section>";
        // The bounds 2, 4 and 3, the next after the latest a range starts at,
        // are steps 2, 4 and 3.
        assert_eq!(
            steps(html),
            [
                "count 4",
                "p --from:2;--to:4",
                "p --from:2",
                "p* --from:1;--to:2",
                "p --from:3",
            ]
        );
    }

    #[test]
    fn steps_can_be_turned_off_by_a_heading_or_the_slide() {
        let html = "<section><h1 steps=\"false\">T</h1><ul><li>a</li></ul><h2>A</h2><ul><li>b</li></ul></section>";
        assert_eq!(steps(html), ["count 1", "h2 --from:1", "ul --from:1"]);
        let html = "<section data-steps=\"false\"><ul><li>a</li></ul><h2 steps=\"true\">A</h2><ul><li>b</li></ul></section>";
        assert_eq!(
            steps(html),
            ["count 2", "h2 --from:1", "ul --from:1", "li --from:2"]
        );
    }

    #[test]
    fn math_steps_are_written_for_katex() {
        let html = "<section><p><span class=\"math math-inline\">a \\htmlData{step=1}{= b} \\htmlData{step=2,collapse=true} c</span></p></section>";
        assert_eq!(
            number(html),
            "<section data-count=\"3\"><p><span class=\"math math-inline\">a \\htmlStyle{--from:2}{\\htmlClass{step}{= b}} \\htmlStyle{--from:3}{\\htmlClass{step step-collapse}{c}}</span></p></section>"
        );
    }

    #[test]
    fn math_steps_count_as_steps_of_their_column() {
        let html = "<section><ul><li>a</li></ul><p><span class=\"math\">\\htmlData{step=3}{x}</span></p></section>";
        assert_eq!(steps(html), ["count 3", "li --from:2"]);
        assert!(number(html).contains("\\htmlStyle{--from:3}{\\htmlClass{step}{x}}"));
    }

    #[test]
    fn a_tex_argument_is_a_group_or_a_token() {
        fn argument(tex: &str) -> (&str, &str) {
            let (span, end) = tex_argument(tex, 0..tex.len());
            (&tex[span], &tex[end..])
        }
        assert_eq!(argument(" {a {b} \\} c} d"), ("a {b} \\} c", " d"));
        assert_eq!(argument("\\alpha b"), ("\\alpha", " b"));
        assert_eq!(argument("\\, b"), ("\\,", " b"));
        assert_eq!(argument("x y"), ("x", " y"));
        assert_eq!(argument(""), ("", ""));
    }

    #[test]
    fn existing_classes_and_styles_are_added_to() {
        let html = "<section><p step class=\"note\" style=\"color: red\">a</p><p step class=x>b</p></section>";
        assert_eq!(
            number(html),
            "<section data-count=\"3\"><p step class=\"note step\" style=\"color: red;--from:2\">a</p><p step class=\"x step\" style=\"--from:3\">b</p></section>"
        );
    }

    #[test]
    fn markup_that_is_not_an_element_is_passed_over() {
        let html = "<section><!-- <li> --><pre><code>&lt;li&gt;</code></pre><script>if (a <li) {}</script><br><img src=x><ul><li>a</ul></section>";
        assert_eq!(steps(html), ["count 2", "li --from:2"]);
    }

    #[test]
    fn a_figure_is_numbered_as_a_slide_of_its_own_and_saved_as_it_was() {
        let svg = "<svg viewBox=\"0 0 1 1\"><g step=\"\"><path d=\"M0\"/></g><g also=\"\" class=\"\"/><g step=\"2..3\" collapse=\"\" style=\"fill:red;\"></g><title>a <g step></title></svg>";
        let numbered = number_figure(svg);
        assert_eq!(
            numbered,
            "<svg viewBox=\"0 0 1 1\"><g step=\"\" class=\"step\" style=\"--from:2\"><path d=\"M0\"/></g><g also=\"\" class=\" step\" style=\"--from:2\"/><g step=\"2..3\" collapse=\"\" style=\"fill:red;;--from:3;--to:4\" class=\"step step-collapse\"></g><title>a <g step></title></svg>"
        );
        assert_eq!(unnumber_figure(&numbered), svg);
    }
}
