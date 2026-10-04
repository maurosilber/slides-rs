//! The steps of the slides, numbered in their html, for slides.css to show
//! and hide as the page says which step it is at.
//!
//! A slide is revealed in steps, numbered from 1, the step it opens with,
//! which shows its first element, usually its title. Each element after it,
//! a heading, a paragraph, a code block, is a step of its own, in order, but
//! one marked as a step itself, which joins the element before it. After
//! each element, and with the first one, come the steps of the elements it
//! marks: every list item, the elements with a `step` or a `data-step`, as
//! raw html and an SVG's are, and the steps of its math, as
//! `\step{...}` writes them. A list shows with its first item, rather than
//! a step before it. The numbers of a range are the slide's steps, wherever
//! the element is, and an element shows from the step its range starts at,
//! or its own, up to the one it ends at, but from the earliest step of what
//! it holds, if earlier, as an SVG does with its elements. A `<style>` or a
//! `<script>` shows nothing, and is no step.
//!
//! A heading with `steps="false"`, as `{ steps=false }` writes, shows
//! everything under it along with it, up to the next heading of its level or
//! above: the elements it marks, and those after it, which join its step.
//! `steps="true"` steps through them again. Outside every such heading, the
//! slide's `data-steps` decides, from its file's frontmatter.
//!
//! With `steps="parallel"` or `steps="interleave"`, a heading's columns, each
//! started by a heading under it of the level of the first one, step each on
//! its own, and are merged: in parallel, the first step of each at the same
//! step, then the second of each, and so on; interleaved, taking turns, the
//! first step of each column, one after another, then the second of each.
//! Their headings show along with the step they join. In a column, the
//! numbers of a range are the column's steps, its heading's 0, for them to
//! line up with the other columns', and the steps after it go on as if it
//! were not there.
//!
//! A run of `<h3>`s, up to the next heading above them, are columns too,
//! which step one after another, as anything else does. Every run of
//! columns is boxed in a `<div class="columns">`, for slides.css to lay them
//! out side by side, and the editor to mark.
//!
//! A bound written with its sign, as `+1`, `+0` or `-1`, is a number of
//! steps from the step of the latest element that steps before it, among
//! those it is beside, or else of the element it is in, or that holds it,
//! as `<li step="+0">` shows along with the item before it, and a step of
//! math, along with the step of math before it, or that holds it, as
//! `\step[+0]{...}` shows along with the step of math before it. Before
//! anything that steps, it is from the latest step. Unlike the numbers of a
//! range, it counts as the latest step, in a column as well.
//!
//! A step can be named by `step="a"`, which makes a step of its own, as an
//! element that is not marked is, rather than join the element before it,
//! as `\step[a]{...}` does in math, and shown from or hidden at a number of
//! steps from it, wherever in the slide it is, with `a+0`, `a+1` or `a-1` as
//! a bound of a range. These steps do not count as the latest, nor as the
//! steps a range is numbered among: they show where the name says, once the
//! whole slide is numbered. A name that names no step shows what is marked
//! from it all along, and is warned of. A name given again names the first step given it, and
//! is warned of, for the step to show along with it as `a+0` does.
//!
//! Each element that steps is given the class `step`, and `step-collapse` if
//! it takes no space while hidden, with the steps it shows in as
//! `--from` and `--to`, excluded, in its style; each slide is given how many
//! steps it has, as `data-count`. A step of math is written as
//! `\htmlStyle{--from:..}{\htmlClass{step}{...}}`, for KaTeX to write the same.

use std::collections::HashMap;
use std::ops::Range as Span;

use super::{Bound, Mark, Range, Step};

/// The html with the steps of every `<section>` numbered, and its columns
/// boxed, as slides.css lays them out: each run of them in a
/// `<div class="columns">`, with how many it has as its `--cols`, and the
/// heading that starts each marked `data-column`.
pub fn number(html: &str) -> String {
    let elements = elements(html);
    let mut edits = Vec::new();
    for (index, children, steps) in sections(html, &elements) {
        let count = number_slide(html, &elements, &children, steps, &mut edits);
        let close = elements[index].close;
        edits.push((close..close, format!(" {COUNT}=\"{count}\"")));
        let items = items(html, &elements, &children, steps);
        for run in runs(&items) {
            let first = &elements[run[0][0]];
            let last = &elements[*run.last().unwrap().last().unwrap()];
            let open = format!("<div class=\"columns\" style=\"--cols:{}\">", run.len());
            edits.push((first.start..first.start, open));
            let end = last.outer_end(html);
            edits.push((end..end, "</div>".to_string()));
            for column in &run {
                let close = elements[column[0]].close;
                edits.push((close..close, " data-column".to_string()));
            }
        }
    }
    apply(html, edits)
}

/// A column of a slide, by where the heading that starts it begins and where
/// what it holds ends, and which of the columns side by side it is, from 0.
#[derive(Debug, PartialEq)]
pub struct Column {
    pub at: usize,
    pub end: usize,
    pub index: usize,
}

/// The runs of columns of a slide made of `items`, each with the elements
/// of each of its columns, in order, the heading that starts it first, and
/// the runs within a column after the run it is in.
fn runs(items: &[Item]) -> Vec<Vec<Vec<usize>>> {
    let mut found = Vec::new();
    for item in items {
        if let Item::Columns { columns, .. } = item {
            found.push(columns.iter().map(|column| elements_of(column)).collect());
            for column in columns {
                found.extend(runs(column));
            }
        }
    }
    found
}

/// The elements of `items`, those in their columns too, in order.
fn elements_of(items: &[Item]) -> Vec<usize> {
    items
        .iter()
        .flat_map(|item| match item {
            &Item::Element { index, .. } => vec![index],
            Item::Columns { columns, .. } => columns
                .iter()
                .flat_map(|column| elements_of(column))
                .collect(),
        })
        .collect()
}

/// A slide's steps, as `number` numbers them, by where they are in the html.
#[derive(Debug, PartialEq)]
pub struct Slide {
    /// Where its `<section>` begins.
    pub at: usize,
    /// How many steps it has.
    pub count: u32,
    /// What steps in it, in order, but what shows from the start all along.
    pub steps: Vec<Stepped>,
    /// What is wrong with how it steps, in order.
    pub warnings: Vec<Warning>,
    /// Its columns, in order.
    pub columns: Vec<Column>,
}

/// What is wrong with how a slide steps, by where in the html.
#[derive(Debug, PartialEq)]
pub struct Warning {
    pub at: usize,
    pub message: String,
}

/// What steps, by where it begins in the html: an element's start tag, or a
/// step of math, and the steps it shows in, from `from` up to `to`, excluded.
#[derive(Debug, PartialEq)]
pub struct Stepped {
    pub at: usize,
    pub from: u32,
    pub to: Option<u32>,
    pub collapse: bool,
}

/// The steps of every `<section>`, as `number` numbers them, rather than
/// written into the html.
pub fn slides(html: &str) -> Vec<Slide> {
    let elements = elements(html);
    sections(html, &elements)
        .into_iter()
        .map(|(index, children, steps_on)| {
            let (resolved, count, warnings) = resolve_slide(html, &elements, &children, steps_on);
            let mut steps: Vec<Stepped> = resolved
                .into_iter()
                .map(|(target, from, to, collapse)| Stepped {
                    at: match target {
                        Target::Element(index) => elements[index].start,
                        Target::Math { span, .. } => span.start,
                    },
                    from,
                    to,
                    collapse,
                })
                .collect();
            steps.sort_by_key(|stepped| stepped.at);
            let items = items(html, &elements, &children, steps_on);
            let columns = runs(&items)
                .into_iter()
                .flat_map(|run| {
                    run.into_iter().enumerate().map(|(index, column)| Column {
                        at: elements[column[0]].start,
                        end: elements[*column.last().unwrap()].content_end(html),
                        index,
                    })
                })
                .collect();
            Slide {
                at: elements[index].start,
                count,
                steps,
                warnings,
                columns,
            }
        })
        .collect()
}

/// The `<section>`s of the html, each with its children and whether it steps
/// through them.
fn sections(html: &str, elements: &[Element]) -> Vec<(usize, Vec<usize>, bool)> {
    let mut sections = Vec::new();
    for (index, element) in elements.iter().enumerate() {
        if element.parent.is_some() || element.name != "section" {
            continue;
        }
        let children: Vec<usize> = (index + 1..element.end)
            .filter(|&child| elements[child].parent == Some(index))
            .collect();
        let steps = element.attribute(html, "data-steps") != Some("false");
        sections.push((index, children, steps));
    }
    sections
}

/// A figure with its steps numbered, as a slide that holds it alone numbers
/// them, as a notebook shows it, with how many steps it has as its
/// `data-count`, as a slide has.
pub fn number_figure(svg: &str) -> String {
    let elements = elements(svg);
    let children: Vec<usize> = (0..elements.len())
        .filter(|&child| elements[child].parent.is_none())
        .collect();
    let mut edits = Vec::new();
    let count = number_slide(svg, &elements, &children, true, &mut edits);
    if let Some(&first) = children.first() {
        let close = elements[first].close;
        edits.push((close..close, format!(" {COUNT}=\"{count}\"")));
    }
    apply(svg, edits)
}

/// The attribute that says how many steps a slide, or a figure, has.
const COUNT: &str = "data-count";

/// A figure as it was before `number_figure` numbered it, as it is saved.
pub fn unnumber_figure(svg: &str) -> String {
    let elements = elements(svg);
    let mut edits = Vec::new();
    for element in &elements {
        if element.parent.is_none()
            && let Some(count) = element.find(COUNT)
            && let Some(value) = count.value.clone()
        {
            // Added with a space before it, and quoted.
            edits.push((count.name.start - 1..value.end + 1, String::new()));
        }
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
    // Where one ends where another begins, the one that ends there goes
    // first, as a step of math within another ends before it.
    edits.sort_by_key(|(span, _)| (span.start, span.end));
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
#[derive(Clone, Debug, PartialEq)]
struct Shown {
    from: At,
    to: Option<At>,
    collapse: bool,
}

/// A step an element shows from or hides at: a number, or a number of steps
/// from a named one, which is found once the whole slide is numbered.
#[derive(Clone, Debug, PartialEq)]
enum At {
    Step(u32),
    Name(String, i32),
}

impl At {
    /// The step numbered otherwise, as `f` says, if it is a number.
    fn map(self, f: impl Fn(u32) -> u32) -> At {
        match self {
            At::Step(step) => At::Step(f(step)),
            name => name,
        }
    }

    /// The step `offset` steps after it, or before it, if negative.
    fn offset(self, offset: i32) -> At {
        match self {
            At::Step(step) => At::Step(step.saturating_add_signed(offset)),
            At::Name(name, from) => At::Name(name, from + offset),
        }
    }

    /// The bound, a number of steps from `reference`, if it is signed.
    fn of(bound: &Bound, reference: &At) -> At {
        match bound {
            &Bound::Step(step) => At::Step(step),
            &Bound::Relative(offset) => reference.clone().offset(offset),
            Bound::Name { name, offset } => At::Name(name.clone(), *offset),
        }
    }
}

/// The steps of what steps in a part of the slide so far, as numbered there,
/// for a step to be numbered from those before it.
type Known = HashMap<Target, At>;

/// What steps: an element, by its index, or a step of math, by where it is
/// and the element it is in.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Target {
    Element(usize),
    Math {
        span: Span<usize>,
        argument: Span<usize>,
        element: usize,
    },
}

/// How the columns under a heading step: together, each column's first step
/// at the same step, or taking turns, first one step of each, in order, then
/// the next of each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Parallel,
    Interleave,
}

/// The level of the headings that start columns, unless a heading says
/// otherwise by stepping them as a mode says, when they are the first under
/// it.
const COLUMN: u8 = 3;

/// What a part of the slide is made of, in order.
enum Item {
    /// An element, with whether it may be a step of its own, and whether the
    /// steps it marks step.
    Element {
        index: usize,
        step: bool,
        marks: bool,
    },
    /// The columns under a heading, which step as its mode says, or one
    /// after another, as a run of `<h3>`s does that says nothing.
    Columns {
        mode: Option<Mode>,
        columns: Vec<Vec<Item>>,
    },
}

/// A heading whose columns step as its mode says, as they are read.
struct Frame {
    level: u8,
    mode: Option<Mode>,
    /// The level of the headings that start its columns, the level of the
    /// first one under it.
    column: Option<u8>,
    columns: Vec<Vec<Item>>,
}

/// The items of a slide made of `children`, stepping through them as `steps`
/// says outside every heading that says otherwise.
fn items(html: &str, elements: &[Element], children: &[usize], steps: bool) -> Vec<Item> {
    // Where the next item goes: the column it is in, or the slide.
    fn target<'a>(slide: &'a mut Vec<Item>, frames: &'a mut [Frame]) -> &'a mut Vec<Item> {
        match frames
            .iter_mut()
            .rev()
            .find(|frame| !frame.columns.is_empty())
        {
            Some(frame) => frame.columns.last_mut().unwrap(),
            None => slide,
        }
    }
    fn close(slide: &mut Vec<Item>, frames: &mut Vec<Frame>) {
        let frame = frames.pop().unwrap();
        if !frame.columns.is_empty() {
            target(slide, frames).push(Item::Columns {
                mode: frame.mode,
                columns: frame.columns,
            });
        }
    }

    let mut slide = Vec::new();
    let mut frames: Vec<Frame> = Vec::new();
    // The headings whose part of the slide the current child is in, by level,
    // each with whether steps are on there.
    let mut scopes: Vec<(u8, bool)> = Vec::new();
    let on = |scopes: &[(u8, bool)]| scopes.last().map_or(steps, |&(_, on)| on);
    for &child in children {
        let element = &elements[child];
        if UNSHOWN.contains(&element.name.as_str()) {
            continue;
        }
        let level = heading_level(&element.name);
        let mut mode = None;
        if let Some(level) = level {
            while scopes.last().is_some_and(|&(scope, _)| scope >= level) {
                scopes.pop();
            }
            while frames.last().is_some_and(|frame| frame.level >= level) {
                close(&mut slide, &mut frames);
            }
            if let Some(frame) = frames.last_mut()
                && frame.column.is_none_or(|column| level <= column)
            {
                frame.column.get_or_insert(level);
                frame.columns.push(Vec::new());
            } else if level == COLUMN {
                // A run of them, up to the next heading above them, are
                // columns, which step one after another.
                frames.push(Frame {
                    level: COLUMN - 1,
                    mode: None,
                    column: Some(COLUMN),
                    columns: vec![Vec::new()],
                });
            }
            mode = match element.attribute(html, "steps") {
                Some("parallel") => Some(Mode::Parallel),
                Some("interleave") => Some(Mode::Interleave),
                _ => None,
            };
        }
        // Whether an element is a step of its own is up to the part of the
        // slide it is in, and what a heading holds is up to the heading.
        let step = on(&scopes);
        if let Some(level) = level {
            let here = match element.attribute(html, "steps") {
                Some(value) => value != "false",
                None => on(&scopes),
            };
            scopes.push((level, here));
        }
        target(&mut slide, &mut frames).push(Item::Element {
            index: child,
            step,
            marks: on(&scopes),
        });
        if let (Some(level), Some(mode)) = (level, mode) {
            frames.push(Frame {
                level,
                mode: Some(mode),
                column: None,
                columns: Vec::new(),
            });
        }
    }
    while !frames.is_empty() {
        close(&mut slide, &mut frames);
    }
    slide
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
    let (resolved, count, _) = resolve_slide(html, elements, children, steps);
    for (target, from, to, collapse) in resolved {
        let classes = if collapse {
            "step step-collapse"
        } else {
            "step"
        };
        let mut vars = format!("--from:{from}");
        if let Some(to) = to {
            vars.push_str(&format!(";--to:{to}"));
        }
        match target {
            Target::Element(index) => {
                let element = &elements[index];
                append(html, element, "class", " ", classes, edits);
                append(html, element, "style", ";", &vars, edits);
            }
            // Written around its argument, rather than over it, for the
            // steps within it to be written too.
            Target::Math { span, argument, .. } => {
                let open = format!("\\htmlStyle{{{vars}}}{{\\htmlClass{{{classes}}}{{");
                edits.push((span.start..argument.start, open));
                edits.push((argument.end..span.end, "}}".to_string()));
            }
        }
    }
    count
}

/// What steps, the steps it shows from and hides at, and whether it collapses.
type Resolved = (Target, u32, Option<u32>, bool);

/// The steps that what steps in a slide made of `children` shows from and
/// hides at, and whether it collapses, but for what shows from the start all
/// along, which needs no step, how many steps the slide has, and what is
/// wrong with how it steps.
fn resolve_slide(
    html: &str,
    elements: &[Element],
    children: &[usize],
    steps: bool,
) -> (Vec<Resolved>, u32, Vec<Warning>) {
    let items = items(html, elements, children, steps);
    let mut names = Vec::new();
    let (shown, count) = number_items(html, elements, &items, false, &mut names);
    let mut targets: HashMap<Target, Shown> = HashMap::new();
    for (target, shown) in shown {
        // The slide counts from the step it opens with, 1.
        let shown = Shown {
            from: shown.from.map(|from| from + 1),
            to: shown.to.map(|to| to.map(|to| to + 1)),
            ..shown
        };
        targets.insert(target, shown);
    }

    // The first step to take a name wins it, and those after it are warned
    // of.
    let at = |target: &Target| match target {
        Target::Element(index) => elements[*index].start,
        Target::Math { span, .. } => span.start,
    };
    names.sort_by_key(|(_, target)| at(target));
    let mut named: HashMap<String, Target> = HashMap::new();
    let mut warnings = Vec::new();
    for (name, target) in names {
        if named.contains_key(&name) {
            warnings.push(Warning {
                at: at(&target),
                message: format!(
                    "the step `{name}` is named again, and the name stays with the first: \
                     to show along with it, write `{name}+0`"
                ),
            });
            continue;
        }
        named.insert(name, target);
    }
    // A step from a name, as the step it names says, or none if it names
    // none, or one named from itself.
    fn resolve(
        at: &At,
        named: &HashMap<String, Target>,
        targets: &HashMap<Target, Shown>,
        depth: usize,
    ) -> Option<u32> {
        match at {
            &At::Step(step) => Some(step),
            At::Name(name, offset) => {
                let target = named.get(name)?;
                let step = match targets.get(target) {
                    Some(shown) if depth < named.len() => {
                        resolve(&shown.from, named, targets, depth + 1)?
                    }
                    Some(_) => return None,
                    // Unmarked, it shows from the start.
                    None => 1,
                };
                Some(step.saturating_add_signed(*offset).max(1))
            }
        }
    }
    // A name that names no step is warned of, as a step that shows from it
    // shows from the start, or is never hidden.
    let mut unnamed: Vec<(usize, &str)> = Vec::new();
    for (target, shown) in &targets {
        for step in [Some(&shown.from), shown.to.as_ref()].into_iter().flatten() {
            if let At::Name(name, _) = step
                && !named.contains_key(name)
            {
                unnamed.push((at(target), name));
            }
        }
    }
    unnamed.sort();
    unnamed.dedup();
    warnings.extend(unnamed.into_iter().map(|(at, name)| Warning {
        at,
        message: format!("no step of the slide is named `{name}`: name one with `step=\"{name}\"`"),
    }));
    let resolved: Vec<Resolved> = targets
        .iter()
        .map(|(target, shown)| {
            let from = resolve(&shown.from, &named, &targets, 0).unwrap_or(1);
            let to = shown
                .to
                .as_ref()
                .and_then(|to| resolve(to, &named, &targets, 0));
            (target.clone(), from, to, shown.collapse)
        })
        .collect();
    // An element shows from the earliest step of what it holds, if earlier
    // than its own, as a hidden one hides all it holds: an SVG, from the
    // first of its elements that step, and a step of math, from the first of
    // those within it.
    let starts: Vec<(usize, u32)> = resolved
        .iter()
        .map(|(target, from, _, _)| (at(target), *from))
        .collect();
    let resolved: Vec<Resolved> = resolved
        .into_iter()
        .map(|(target, from, to, collapse)| {
            let inside = match &target {
                Target::Element(index) => elements[*index].text.clone().unwrap_or_default(),
                Target::Math { argument, .. } => argument.clone(),
            };
            let from = starts
                .iter()
                .filter(|(at, _)| inside.contains(at))
                .map(|&(_, start)| start)
                .fold(from, u32::min);
            (target, from, to, collapse)
        })
        .collect();
    // One hidden again before it shows is never shown.
    let mut empty: Vec<(usize, u32, u32)> = resolved
        .iter()
        .filter_map(|(target, from, to, _)| {
            to.filter(|to| to <= from).map(|to| (at(target), *from, to))
        })
        .collect();
    empty.sort();
    warnings.extend(empty.into_iter().map(|(at, from, to)| Warning {
        at,
        message: format!(
            "this shows on no step: it shows from step {from}, and is hidden again at step {to}"
        ),
    }));
    warnings.sort_by_key(|warning| warning.at);
    // As many steps as the latest any shows from, or is hidden again at.
    let count = resolved
        .iter()
        .flat_map(|&(_, from, to, _)| [Some(from), to])
        .flatten()
        .fold(count + 1, u32::max);
    // What shows from the start, all along, needs no step.
    let resolved = resolved
        .into_iter()
        .filter(|&(_, from, to, _)| !(from == 1 && to.is_none()))
        .collect();
    (resolved, count, warnings)
}

/// Numbers the steps of `items`, from 0, the step they open with, and returns
/// how many come after it. In a column, the numbers of a range are the
/// column's steps, for them to line up with the other columns', and the
/// steps after it go on as if it were not there; elsewhere, they are the
/// steps the element they are in marks, in order, after its own.
fn number_items(
    html: &str,
    elements: &[Element],
    items: &[Item],
    column: bool,
    names: &mut Vec<(String, Target)>,
) -> (Vec<(Target, Shown)>, u32) {
    // The elements of each step, and the columns that open along with it,
    // each after as many of its elements as come before it. The first is the
    // first element, and what joins it.
    struct Group<'a> {
        elements: Vec<(usize, bool)>,
        columns: Vec<(Mode, &'a [Vec<Item>], usize)>,
    }
    let new = || Group {
        elements: Vec::new(),
        columns: Vec::new(),
    };
    let mut groups = vec![new()];
    let mut first = true;
    for item in in_order(items) {
        match item {
            &Item::Element { index, step, marks } => {
                let element = &elements[index];
                // A named step is a step of its own, but a marked element
                // joins the step before it.
                let mark = element_mark(html, element);
                let named = matches!(
                    &mark,
                    Some(Mark {
                        step: Step::Named(_),
                        ..
                    })
                );
                let step = step
                    && !first
                    && !matches!(element.name.as_str(), "br" | "wbr")
                    && (mark.is_none() || named);
                if step {
                    groups.push(new());
                }
                groups.last_mut().unwrap().elements.push((index, marks));
            }
            Item::Columns {
                mode: Some(mode),
                columns,
            } => {
                let group = groups.last_mut().unwrap();
                let before = group.elements.len();
                group.columns.push((*mode, columns, before));
            }
            Item::Columns { mode: None, .. } => unreachable!("taken in order"),
        }
        first = false;
    }

    // Later entries win, as an element's own range wins over its step's.
    let mut shown: Vec<(Target, Shown)> = Vec::new();
    let mut known = Known::new();
    let mut count = 0;
    // The latest step, but in a column, not a range's.
    let mut cursor = 0;
    for (i, group) in groups.iter().enumerate() {
        // The steps marked by some of its elements, as a range numbers them.
        let marks_of = |elements_of: &[(usize, bool)]| -> Vec<(Target, Mark)> {
            let marks = elements_of
                .iter()
                // Unmarked, the element's parts show along with it.
                .filter(|&&(_, marks)| marks)
                .flat_map(|&(child, _)| parts_of(html, elements, child));
            // On a slide, the numbers of a range are its steps, from 1, the
            // step it opens with, which is 0 here; in a column, they are the
            // column's.
            if column {
                marks.collect()
            } else {
                marks
                    .map(|(target, mark)| (target, from_slide(mark)))
                    .collect()
            }
        };
        // A list is shown along with its first item, at the step that item
        // is, rather than a step before it.
        let list = i > 0
            && group
                .elements
                .first()
                .is_some_and(|&(child, _)| matches!(elements[child].name.as_str(), "ul" | "ol"));
        let own = if i == 0 { 0 } else { cursor + 1 };
        // Before its marks, which win over it.
        for &(child, _) in &group.elements {
            if let Some(Mark {
                step: Step::Named(name),
                ..
            }) = element_mark(html, &elements[child])
            {
                names.push((name, Target::Element(child)));
            }
            known.insert(Target::Element(child), At::Step(own));
            let own = Shown {
                from: At::Step(own),
                to: None,
                collapse: false,
            };
            shown.push((Target::Element(child), own));
        }
        let mut latest = if list { own - 1 } else { own };
        let mut numbered = 0;
        // In order: the marks of the elements before each of its columns, then
        // the columns, and those of the elements after them.
        let ends = group.columns.iter().map(|&(_, _, before)| before);
        for (j, end) in ends.chain([group.elements.len()]).enumerate() {
            // Its own elements that are named show at its step, as named.
            let own_element = |target: &Target| {
                matches!(target, &Target::Element(index)
                    if group.elements.iter().any(|&(child, _)| child == index))
            };
            let marks: Vec<(Target, Mark)> = marks_of(&group.elements[numbered..end])
                .into_iter()
                .map(|(target, mark)| match mark.step {
                    Step::Named(_) if own_element(&target) => {
                        let step = Step::Range(Range {
                            start: None,
                            end: None,
                        });
                        (target, Mark { step, ..mark })
                    }
                    _ => (target, mark),
                })
                .collect();
            numbered = end;
            let parts;
            (parts, latest) = ranges(elements, &marks, own, latest, !column, &mut known, names);
            cursor = latest.max(own);
            count = steps_of(&parts).fold(count.max(cursor), u32::max);
            shown.extend(parts);
            let Some(&(mode, columns, _)) = group.columns.get(j) else {
                break;
            };
            let (merged, length) = merge(html, elements, mode, columns, names);
            // The columns open with the step they join, and step after it.
            let at = |step: u32| if step == 0 { own } else { count + step };
            for (target, merged) in merged {
                let from = merged.from.map(at);
                let to = merged.to.map(|to| to.map(at));
                known.insert(target.clone(), from.clone());
                shown.push((target, Shown { from, to, ..merged }));
            }
            count += length;
            if !column {
                cursor = count;
            }
            latest = cursor;
        }
    }
    (shown, count)
}

/// The items, with the columns that step one after another taken in the
/// order they come in, as if they were not columns.
fn in_order(items: &[Item]) -> Vec<&Item> {
    items
        .iter()
        .flat_map(|item| match item {
            Item::Columns {
                mode: None,
                columns,
            } => columns.iter().flat_map(|column| in_order(column)).collect(),
            item => vec![item],
        })
        .collect()
}

/// The mark with the numbers of its range counted from 0, the step a slide
/// opens with, rather than from 1.
fn from_slide(mark: Mark) -> Mark {
    let shift = |bound: Option<Bound>| match bound {
        Some(Bound::Step(step)) => Some(Bound::Step(step.saturating_sub(1))),
        bound => bound,
    };
    let step = match mark.step {
        Step::Range(Range { start, end }) => Step::Range(Range {
            start: shift(start),
            end: shift(end),
        }),
        step => step,
    };
    Mark { step, ..mark }
}

/// The numbers that the steps of `parts` are, rather than names.
fn steps_of(parts: &[(Target, Shown)]) -> impl Iterator<Item = u32> + '_ {
    parts
        .iter()
        .flat_map(|(_, shown)| [Some(&shown.from), shown.to.as_ref()])
        .filter_map(|at| match at {
            Some(&At::Step(step)) => Some(step),
            _ => None,
        })
}

/// The steps of `marks`, after the step `latest`, of an element at the step
/// `own`: one with no range comes one after the latest one so far, and a
/// signed bound is a number of steps from the step before it, as `reference` finds it in `known`. Where
/// a range's start is not one of the steps, as in a column, it does not
/// count as the latest, nor does one from a name. A step with a name is
/// added to `names`.
fn ranges(
    elements: &[Element],
    marks: &[(Target, Mark)],
    own: u32,
    mut latest: u32,
    ranges_count: bool,
    known: &mut Known,
    names: &mut Vec<(String, Target)>,
) -> (Vec<(Target, Shown)>, u32) {
    let mut steps = Vec::new();
    for (target, mark) in marks {
        let next = Range {
            start: Some(Bound::Step(latest + 1)),
            end: None,
        };
        let range = match &mark.step {
            Step::Range(range) => range.clone(),
            Step::Next => next,
            Step::Named(name) => {
                names.push((name.clone(), target.clone()));
                next
            }
        };
        let reference = reference(elements, known, target).unwrap_or(At::Step(latest));
        let from = range
            .start
            .as_ref()
            .map_or(At::Step(own), |start| At::of(start, &reference));
        let to = range.end.as_ref().map(|end| At::of(end, &reference));
        let counts = match &range.start {
            Some(Bound::Step(_)) => ranges_count || !matches!(mark.step, Step::Range(_)),
            Some(Bound::Relative(_)) => true,
            _ => false,
        };
        if counts && let At::Step(start) = from {
            latest = latest.max(start);
        }
        known.insert(target.clone(), from.clone());
        let collapse = mark.collapse;
        steps.push((target.clone(), Shown { from, to, collapse }));
    }
    (steps, latest)
}

/// The step that a signed bound of `target` is a number of steps from: the
/// step of the latest before it, beside it, that steps, or else of what it is
/// in, or holds it, as `known` says, if any. For a step of math, what is
/// beside it are the steps of math before it, in the same step, or math.
fn reference(elements: &[Element], known: &Known, target: &Target) -> Option<At> {
    // The step of an element, or of the nearest that holds it.
    let held = |mut index: Option<usize>| {
        while let Some(at) = index {
            if let Some(step) = known.get(&Target::Element(at)) {
                return Some(step.clone());
            }
            index = elements[at].parent;
        }
        None
    };
    match target {
        &Target::Element(index) => {
            let parent = elements[index].parent;
            let sibling = (0..index)
                .rev()
                .take_while(|&before| Some(before) != parent)
                .filter(|&before| elements[before].parent == parent)
                .find_map(|before| known.get(&Target::Element(before)));
            sibling.cloned().or_else(|| held(parent))
        }
        Target::Math { span, element, .. } => {
            let maths = known.iter().filter_map(|(other, step)| match other {
                Target::Math {
                    span,
                    argument,
                    element: other,
                } if other == element => Some((span, argument, step)),
                _ => None,
            });
            // The innermost step of math it is in.
            let parent = maths
                .clone()
                .filter(|(_, argument, _)| argument.contains(&span.start))
                .min_by_key(|(_, argument, _)| argument.len());
            // The one beside it that ends last before it, rather than one
            // within it, which ends earlier.
            let sibling = maths
                .filter(|(before, _, _)| before.end <= span.start)
                .filter(|(before, _, _)| {
                    parent.is_none_or(|(_, argument, _)| argument.contains(&before.start))
                })
                .max_by_key(|(before, _, _)| before.end);
            match sibling.or(parent) {
                Some((_, _, step)) => Some(step.clone()),
                None => held(Some(*element)),
            }
        }
    }
}

/// The steps of columns, numbered each on its own and merged as `mode` says,
/// from 0, the step they open with, and how many come after it.
fn merge(
    html: &str,
    elements: &[Element],
    mode: Mode,
    columns: &[Vec<Item>],
    names: &mut Vec<(String, Target)>,
) -> (Vec<(Target, Shown)>, u32) {
    let numbered: Vec<_> = columns
        .iter()
        .map(|column| number_items(html, elements, column, true, names))
        .collect();
    let lengths: Vec<u32> = numbered.iter().map(|&(_, length)| length).collect();
    // The step that the `step`th of a column is merged at.
    let at = |column: usize, step: u32| match mode {
        Mode::Parallel => step,
        Mode::Interleave if step == 0 => 0,
        // After every turn before this one, and those of the columns before
        // it in this one.
        Mode::Interleave => {
            let before: u32 = lengths.iter().map(|&length| length.min(step - 1)).sum();
            let beside = lengths[..column]
                .iter()
                .filter(|&&length| length >= step)
                .count() as u32;
            before + beside + 1
        }
    };
    let length = match mode {
        Mode::Parallel => lengths.iter().copied().max().unwrap_or(0),
        Mode::Interleave => lengths.iter().sum(),
    };
    let mut merged = Vec::new();
    for (column, (shown, _)) in numbered.into_iter().enumerate() {
        for (target, shown) in shown {
            let from = shown.from.map(|from| at(column, from));
            let to = shown.to.map(|to| to.map(|to| at(column, to)));
            merged.push((target, Shown { from, to, ..shown }));
        }
    }
    (merged, length)
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
            parts.extend(math_marks(html, index, text));
        }
    }
    parts
}

/// How an element marks itself as a step, if it does: by the range its `step`
/// or `data-step` says, or as the next step, as a list item does.
fn element_mark(html: &str, element: &Element) -> Option<Mark> {
    let collapse = element.find("collapse").is_some() || element.find("data-collapse").is_some();
    let range = element
        .attribute(html, "step")
        .or_else(|| element.attribute(html, "data-step"));
    let step = if let Some(step) = range.and_then(Step::parse) {
        step
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
/// src/markdown/math.rs writes them, each with where it is and its argument,
/// in the math `element`, whose TeX is `text`.
fn math_marks(html: &str, element: usize, text: Span<usize>) -> Vec<(Target, Mark)> {
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
        // Nothing, as a bare `step` attribute, is the next step.
        let step = match range {
            "" => Step::Next,
            range => match Step::parse(range) {
                Some(step) => step,
                None => continue,
            },
        };
        at += close + 1;
        let (argument, end) = tex_argument(html, at..text.end);
        marks.push((
            Target::Math {
                span: start..end,
                argument,
                element,
            },
            Mark { step, collapse },
        ));
        // It goes on within its argument, which may hold steps of its own.
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
    /// Where its start tag begins.
    start: usize,
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

    /// Where it ends, after its end tag, if it has one.
    fn outer_end(&self, html: &str) -> usize {
        let end = self.text.as_ref().map_or(self.close, |text| text.end);
        let rest = &html[end..];
        let closes = rest.starts_with("</")
            && rest[2..]
                .get(..self.name.len())
                .is_some_and(|name| name.eq_ignore_ascii_case(&self.name));
        match rest.find('>') {
            Some(close) if closes => end + close + 1,
            _ => end,
        }
    }

    /// Where what it shows ends: its last character but whitespace, before
    /// its end tag, or its start tag's.
    fn content_end(&self, html: &str) -> usize {
        let end = self.text.as_ref().map_or(self.close, |text| text.end);
        html[..end].trim_end().len()
    }

    fn has_class(&self, html: &str, class: &str) -> bool {
        self.attribute(html, "class")
            .is_some_and(|classes| classes.split_ascii_whitespace().any(|name| name == class))
    }
}

/// Elements that show nothing on a slide.
const UNSHOWN: &[&str] = &["link", "meta", "script", "style", "template"];

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
            start,
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
        assert_eq!(
            steps(html),
            ["count 3", "ul --from:2", "li --from:2", "li --from:3"]
        );
        assert_eq!(
            number(html),
            "<section data-count=\"3\">\n<h1>Title</h1>\n<ul class=\"step\" style=\"--from:2\">\n<li class=\"step\" style=\"--from:2\">a</li>\n<li class=\"step\" style=\"--from:3\">b</li>\n</ul>\n</section>\n"
        );
    }

    #[test]
    fn each_element_after_the_first_is_a_step_followed_by_its_own() {
        let html = "<section><style>p {}</style><h1>T</h1><h3>A</h3><ul><li>a</li></ul><p>b <span step>c</span></p><h3>B</h3></section>";
        assert_eq!(
            steps(html),
            [
                "count 6",
                "h3 --from:2",
                "ul --from:3",
                "li --from:3",
                "p --from:4",
                "span --from:5",
                "h3 --from:6",
            ]
        );
    }

    #[test]
    fn a_marked_element_joins_the_step_before_it() {
        let html = "<section><h1>T</h1><p>a</p><p step=\"..3\" collapse>b</p><p step=\"3\" collapse>c</p></section>";
        assert_eq!(
            steps(html),
            ["count 3", "p --from:2", "p* --from:2;--to:3", "p* --from:3",]
        );
    }

    #[test]
    fn ranges_are_the_steps_their_bounds_are() {
        let html = "<section><p step=\"2..4\">a</p><p step=\"+0\">b</p><p step=\"..2\" collapse>c</p><p step>d</p></section>";
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
    fn ranges_are_steps_of_the_slide_wherever_they_are() {
        let html =
            "<section><h1>T</h1><p>a</p><p>b</p><p>c</p><svg><g step=\"3..\"></g></svg></section>";
        assert_eq!(
            steps(html),
            [
                "count 5",
                "p --from:2",
                "p --from:3",
                "p --from:4",
                "svg --from:3",
                "g --from:3"
            ]
        );
    }

    #[test]
    fn an_element_shows_from_the_earliest_step_it_holds() {
        // As matplotlib's figure, with `fig.set_gid("step=1")`, shows from
        // the start, and a line in it at a step of its own.
        let html =
            "<section><h1>T</h1><p>a</p><svg><g step=\"1\"><g step=\"3\"></g></g></svg></section>";
        assert_eq!(steps(html), ["count 3", "p --from:2", "g --from:3"]);
    }

    #[test]
    fn steps_can_be_turned_off_by_a_heading_or_the_slide() {
        let html = "<section><h1 steps=\"false\">T</h1><ul><li>a</li></ul><h2>A</h2><ul><li>b</li></ul></section>";
        assert_eq!(steps(html), ["count 1"]);
        let html = "<section><h1>T</h1><h3 steps=\"false\">A</h3><p>a</p><ul><li>b</li></ul><h3>B</h3></section>";
        assert_eq!(
            steps(html),
            [
                "count 3",
                "h3 --from:2",
                "p --from:2",
                "ul --from:2",
                "h3 --from:3"
            ]
        );
        let html = "<section data-steps=\"false\"><ul><li>a</li></ul><h2 steps=\"true\">A</h2><ul><li>b</li></ul><p>c</p></section>";
        assert_eq!(
            steps(html),
            ["count 3", "ul --from:2", "li --from:2", "p --from:3"]
        );
    }

    #[test]
    fn parallel_columns_step_together() {
        let html = "<section><h1 steps=\"parallel\">T</h1><h3>A</h3><ul><li>a</li><li>b</li></ul><h3>B</h3><p>c</p></section>";
        assert_eq!(
            steps(html),
            [
                "count 3",
                "ul --from:2",
                "li --from:2",
                "li --from:3",
                "p --from:2"
            ]
        );
    }

    #[test]
    fn interleaved_columns_take_turns() {
        let html = "<section><h1 steps=\"interleave\">T</h1><h3>A</h3><ul><li>a</li><li>b</li></ul><h3>B</h3><ul><li>c</li></ul><h3>C</h3><p>d</p><p>e</p></section>";
        assert_eq!(
            steps(html),
            [
                "count 6",
                "ul --from:2",
                "li --from:2",
                "li --from:5",
                "ul --from:3",
                "li --from:3",
                "p --from:4",
                "p --from:6",
            ]
        );
    }

    #[test]
    fn a_range_in_a_column_is_a_step_of_the_column() {
        // `3` is the column's third step, as `c` is the other's.
        let html = "<section><h1>T</h1><h2 steps=\"parallel\">S</h2><h3>A</h3><ul><li>a</li><li>b</li><li>c</li></ul><h3>B</h3><p step=\"3\">d</p><p><span class=\"math\">x \\htmlData{step=..2}{y}\\htmlData{step=}{z}</span></p><h2>U</h2></section>";
        assert_eq!(
            steps(html),
            [
                "count 6",
                "h2 --from:2",
                "h3 --from:2",
                "ul --from:3",
                "li --from:3",
                "li --from:4",
                "li --from:5",
                "h3 --from:2",
                "p --from:5",
                "p --from:3",
                "h2 --from:6",
            ]
        );
        assert!(number(html).contains("x \\htmlStyle{--from:3;--to:4}{\\htmlClass{step}{y}}\\htmlStyle{--from:4}{\\htmlClass{step}{z}}"));
    }

    #[test]
    fn a_step_can_show_from_a_named_one() {
        // `b` is the second paragraph's step, and `c` the third's, as their
        // names say.
        let html = "<section><h1>T</h1><p>a</p><p step=\"b\">b</p><p step=\"b+0..c+1\">x</p><p>c <span step=\"c\">y</span></p><p>d</p><p step=\"b-1\">z</p></section>";
        assert_eq!(
            steps(html),
            [
                "count 6",
                "p --from:2",
                "p --from:3",
                "p --from:3;--to:6",
                "p --from:4",
                "span --from:5",
                "p --from:6",
                "p --from:2",
            ]
        );
    }

    #[test]
    fn a_name_syncs_steps_across_columns() {
        let html = "<section><h1>T</h1><h2 steps=\"parallel\">S</h2><h3>A</h3><ul><li step=\"b\">a</li><li>b</li><li>c</li></ul><h3>B</h3><p step=\"b+1\">d</p><h2 step=\"n\">N</h2><p step=\"n+1\">e</p><ul><li>f</li></ul></section>";
        let numbered = number(html);
        // `d` shows one after `a`, in the other column, and `n+0` is the
        // h2's step, after the columns.
        assert!(numbered.contains("<p step=\"b+1\" class=\"step\" style=\"--from:4\">d</p>"));
        assert!(numbered.contains("<h2 step=\"n\" class=\"step\" style=\"--from:6\">"));
        assert!(numbered.contains("<p step=\"n+1\" class=\"step\" style=\"--from:7\">"));
        assert!(numbered.contains("<li class=\"step\" style=\"--from:7\">f</li>"));
        assert!(numbered.starts_with("<section data-count=\"7\">"));
    }

    #[test]
    fn a_signed_step_is_from_the_step_before_it() {
        // `+0` along with the item before it, `+2` two after it, and `-1`
        // one before the paragraph before it.
        let html = "<section><h1>T</h1><ul><li>a</li><li step=\"+0\">b</li><li step=\"+2\">c</li><li>d</li></ul><p>e</p><p step=\"-1\">f</p></section>";
        assert_eq!(
            steps(html),
            [
                "count 6",
                "ul --from:2",
                "li --from:2",
                "li --from:2",
                "li --from:4",
                "li --from:5",
                "p --from:6",
                "p --from:5",
            ]
        );
    }

    #[test]
    fn a_signed_step_with_none_before_it_is_from_what_holds_it() {
        let html = "<section><h1>T</h1><p>a</p><p>b <span step=\"+0..+2\">c</span> <b><span step=\"+1\">d</span></b></p><p>e</p></section>";
        assert_eq!(
            steps(html),
            [
                "count 5",
                "p --from:2",
                "p --from:3",
                "span --from:3;--to:5",
                "span --from:4",
                "p --from:5",
            ]
        );
    }

    #[test]
    fn a_signed_step_of_math_is_from_the_step_of_math_before_it() {
        let html = "<section><h1>T</h1><p><span class=\"math\">\\htmlData{step=+0}{a} \\htmlData{step=+2}{b \\htmlData{step=+1}{c}} \\htmlData{step=-1}{d}</span></p></section>";
        assert!(number(html).contains(
            "\\htmlStyle{--from:2}{\\htmlClass{step}{a}} \\htmlStyle{--from:4}{\\htmlClass{step}{b \\htmlStyle{--from:5}{\\htmlClass{step}{c}}}} \\htmlStyle{--from:3}{\\htmlClass{step}{d}}"
        ));
    }

    #[test]
    fn a_signed_step_in_a_column_is_from_the_columns_step() {
        let html = "<section><h1 steps=\"interleave\">T</h1><h3>A</h3><p>a</p><p step=\"+1\">b</p><h3>B</h3><p>c</p><p>d</p></section>";
        assert_eq!(
            steps(html),
            [
                "count 5",
                "p --from:2",
                "p --from:4",
                "p --from:3",
                "p --from:5",
            ]
        );
    }

    #[test]
    fn a_signed_step_from_a_name_is_from_the_named_one() {
        let html = "<section><h1>T</h1><p step=\"a\">a</p><p>b</p><p step=\"a+0\">c <span step=\"+1\">d</span></p></section>";
        assert_eq!(
            steps(html),
            [
                "count 3",
                "p --from:2",
                "p --from:3",
                "p --from:2",
                "span --from:3"
            ]
        );
    }

    #[test]
    fn a_named_step_is_a_step_of_its_own() {
        // As a list that is not named, it shows with its first item.
        let html = "<section><h1>T</h1><p>a</p><ul step=\"b\"><li>x</li><li>y</li></ul><p step=\"b+0\">c</p></section>";
        assert_eq!(
            steps(html),
            [
                "count 4",
                "p --from:2",
                "ul --from:3",
                "li --from:3",
                "li --from:4",
                "p --from:3",
            ]
        );
    }

    #[test]
    fn a_name_given_again_stays_with_the_first_and_is_warned_of() {
        let html = "<section><h1>T</h1><p step=\"a\">a</p><p step=\"a\">b</p><p step=\"a+0\">c</p></section>";
        assert_eq!(
            steps(html),
            ["count 3", "p --from:2", "p --from:3", "p --from:2"]
        );
        let slides = slides(html);
        let [warning] = &slides[0].warnings[..] else {
            panic!("{:?}", slides[0].warnings);
        };
        assert_eq!(warning.at, html.find("<p step=\"a\">b").unwrap());
        assert!(warning.message.contains("`a+0`"), "{}", warning.message);
    }

    #[test]
    fn a_step_from_a_name_that_names_none_shows_all_along() {
        let html = "<section><h1>T</h1><p step=\"x+0\">a</p><p step=\"..x+0\">b</p></section>";
        assert_eq!(steps(html), ["count 1"]);
        let warnings: Vec<_> = slides(html)
            .remove(0)
            .warnings
            .into_iter()
            .map(|warning| (warning.at, warning.message))
            .collect();
        let message = "no step of the slide is named `x`: name one with `step=\"x\"`";
        assert_eq!(
            warnings,
            [
                (html.find("<p step=\"x").unwrap(), message.to_string()),
                (html.find("<p step=\"..").unwrap(), message.to_string()),
            ]
        );
    }

    #[test]
    fn a_run_of_h3s_is_boxed_as_columns_up_to_the_next_heading_above_them() {
        let html = "<section><h1>T</h1><h3>A</h3><p>a</p><h3>B</h3><ul><li>b</li></ul><h2>S</h2><p>c</p></section>";
        assert_eq!(
            number(html),
            "<section data-count=\"7\"><h1>T</h1><div class=\"columns\" style=\"--cols:2\"><h3 class=\"step\" style=\"--from:2\" data-column>A</h3><p class=\"step\" style=\"--from:3\">a</p><h3 class=\"step\" style=\"--from:4\" data-column>B</h3><ul class=\"step\" style=\"--from:5\"><li class=\"step\" style=\"--from:5\">b</li></ul></div><h2 class=\"step\" style=\"--from:6\">S</h2><p class=\"step\" style=\"--from:7\">c</p></section>"
        );
    }

    #[test]
    fn the_columns_a_heading_steps_are_boxed_whatever_their_level() {
        let html = "<section><h1 steps=\"parallel\">T</h1><h2>A</h2><h3>x</h3><h3>y</h3><h2>B</h2><p>b</p></section>";
        let numbered = number(html);
        assert_eq!(
            numbered
                .matches("<div class=\"columns\" style=\"--cols:2\">")
                .count(),
            2,
            "{numbered}"
        );
        assert!(
            numbered.contains("<p class=\"step\" style=\"--from:2\">b</p></div></section>"),
            "{numbered}"
        );
        let columns: Vec<_> = slides(html)
            .remove(0)
            .columns
            .into_iter()
            .map(|column| (&html[column.at..column.end], column.index))
            .collect();
        assert_eq!(
            columns,
            [
                ("<h2>A</h2><h3>x</h3><h3>y", 0),
                ("<h2>B</h2><p>b", 1),
                ("<h3>x", 0),
                ("<h3>y", 1),
            ]
        );
    }

    #[test]
    fn a_slide_has_the_step_its_last_step_is_hidden_again_at() {
        let html = "<section><h1>T</h1><p step=\"a\">a</p><p step=\"..a+2\">b</p></section>";
        assert_eq!(steps(html), ["count 4", "p --from:2", "p --from:2;--to:4"]);
    }

    #[test]
    fn a_step_hidden_again_before_it_shows_is_warned_of() {
        let html = "<section><h1>T</h1><p>a</p><p>b</p><p step=\"..2\">c</p><p step=\"..4\">d</p></section>";
        let slide = slides(html).remove(0);
        let [warning] = &slide.warnings[..] else {
            panic!("{:?}", slide.warnings);
        };
        assert_eq!(warning.at, html.find("<p step=\"..2").unwrap());
        assert_eq!(
            warning.message,
            "this shows on no step: it shows from step 3, and is hidden again at step 2"
        );
    }

    #[test]
    fn a_math_step_can_be_named_next() {
        let html = "<section><h1>T</h1><p>a</p><p><span class=\"math\">\\htmlData{step=next}{b}</span></p><p step=\"next+0\">c</p></section>";
        assert_eq!(
            steps(html),
            ["count 4", "p --from:2", "p --from:3", "p --from:4"]
        );
    }

    #[test]
    fn math_steps_can_be_named() {
        let html = "<section><h1>T</h1><p><span class=\"math\">\\htmlData{step=s}{a} \\htmlData{step=}{b}</span></p><p step=\"s+0\">c</p></section>";
        assert_eq!(steps(html), ["count 4", "p --from:2", "p --from:3"]);
    }

    #[test]
    fn math_steps_are_written_for_katex() {
        let html = "<section><p><span class=\"math math-inline\">a \\htmlData{step=2}{= b} \\htmlData{step=3,collapse=true} c</span></p></section>";
        assert_eq!(
            number(html),
            "<section data-count=\"3\"><p><span class=\"math math-inline\">a \\htmlStyle{--from:2}{\\htmlClass{step}{= b}} \\htmlStyle{--from:3}{\\htmlClass{step step-collapse}{c}}</span></p></section>"
        );
    }

    #[test]
    fn math_steps_count_as_steps_of_their_element() {
        let html = "<section><ul><li>a</li></ul><p><span class=\"math\">\\htmlData{step=4}{x}</span></p></section>";
        assert_eq!(steps(html), ["count 4", "li --from:2", "p --from:3"]);
        assert!(number(html).contains("\\htmlStyle{--from:4}{\\htmlClass{step}{x}}"));
    }

    #[test]
    fn math_steps_within_math_steps_are_written_too() {
        let html = "<section><h1>T</h1><p><span class=\"math\">\\htmlData{step=3}{x \\htmlData{step=4} y}</span></p></section>";
        assert_eq!(
            number(html),
            "<section data-count=\"4\"><h1>T</h1><p class=\"step\" style=\"--from:2\"><span class=\"math\">\\htmlStyle{--from:3}{\\htmlClass{step}{x \\htmlStyle{--from:4}{\\htmlClass{step}{y}}}}</span></p></section>"
        );
    }

    #[test]
    fn a_math_step_shows_from_the_earliest_step_it_holds() {
        let html = "<section><h1>T</h1><p>a</p><p>b</p><p><span class=\"math\">\\htmlData{step=}{x \\htmlData{step=2}{y}}</span></p></section>";
        assert!(number(html).contains(
            "\\htmlStyle{--from:2}{\\htmlClass{step}{x \\htmlStyle{--from:2}{\\htmlClass{step}{y}}}}"
        ));
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
        assert_eq!(
            steps(html),
            ["count 3", "img --from:2", "ul --from:3", "li --from:3"]
        );
    }

    #[test]
    fn a_figure_is_numbered_as_a_slide_of_its_own_and_saved_as_it_was() {
        let svg = "<svg viewBox=\"0 0 1 1\"><g step=\"\"><path d=\"M0\"/></g><g step=\"+0\" class=\"\"/><g step=\"2..3\" collapse=\"\" style=\"fill:red;\"></g><title>a <g step></title></svg>";
        let numbered = number_figure(svg);
        assert_eq!(
            numbered,
            "<svg viewBox=\"0 0 1 1\" data-count=\"3\"><g step=\"\" class=\"step\" style=\"--from:2\"><path d=\"M0\"/></g><g step=\"+0\" class=\" step\" style=\"--from:2\"/><g step=\"2..3\" collapse=\"\" style=\"fill:red;;--from:2;--to:3\" class=\"step step-collapse\"></g><title>a <g step></title></svg>"
        );
        assert_eq!(unnumber_figure(&numbered), svg);
    }
}
