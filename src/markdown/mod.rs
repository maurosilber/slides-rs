//! Rendering one markdown file into slides, apart from the files it imports
//! and the outputs of its code cells, which are filled in later.

mod math;

use std::ops::Range;
use std::path::{Path, PathBuf};

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd, html};

use crate::aspect::{self, AspectRatio};
use crate::paths::{self, Importing, canonical, parent};
use crate::step;
use crate::store;

/// Marks an import in the rendered html, followed by its path and a newline.
const IMPORT: char = '\u{0}';

/// Marks a code cell in the rendered html, followed by a newline. The cells
/// are marked in order, so the n-th marker stands for the n-th cell.
const CELL: char = '\u{1}';

/// Opens every slide in the rendered html.
pub const SECTION: &str = "<section>";

/// Opens a slide whose steps all show at once.
pub const NO_STEPS: &str = "<section data-steps=\"false\">";

/// A file's slides, in the order they are rendered: its own html, the
/// outputs of its code cells, and the files it imports, which bring their own.
pub enum Part {
    Html(String),
    /// A code cell, by the hash its outputs are saved under.
    Cell(String),
    Import(PathBuf),
}

/// A rendered file. Its code cells make up a notebook, run on a kernel of its own.
pub struct File {
    /// Its markdown, as rendered.
    pub markdown: String,
    pub parts: Vec<Part>,
    pub cells: Vec<String>,
    /// The hash each cell's outputs are saved under.
    pub hashes: Vec<String>,
    /// The lock file of the environment its cells run in, which their hashes cover.
    pub lock: Option<PathBuf>,
    /// What its frontmatter asks of the page. Only the input's is used, so
    /// an imported file can keep the theme it was written with.
    pub page: PageSettings,
    /// Whether its frontmatter steps through its slides' steps. Without
    /// a say, it does as the file importing it does.
    pub steps: Option<bool>,
}

impl File {
    /// The files it imports, in order.
    pub fn imports(&self) -> impl Iterator<Item = &PathBuf> {
        self.parts.iter().filter_map(|part| match part {
            Part::Import(path) => Some(path),
            Part::Html(_) | Part::Cell(_) => None,
        })
    }
}

/// Renders the markdown of the file at `path` into `<section>`s, taking the
/// YAML frontmatter out and splitting on horizontal rules, with a part
/// boundary at every import and at every code cell. The code of the cells
/// comes along, in order.
pub fn render(markdown: &str, path: &Path) -> File {
    let (rendered, frontmatter, cells) = html_of(markdown, parent(path), false);

    // Split at the markers, so every file is cached apart from the ones it
    // imports, and apart from the outputs of its cells, which may come later.
    let frontmatter = Frontmatter::parse(&frontmatter, path);
    let dir = parent(path);
    let lock = store::lock_file(dir);
    let hashes = store::hashes(dir, &store::environment(lock.as_deref()), &cells);
    let mut next_hash = hashes.iter().cloned();
    let mut parts = Vec::new();
    let mut rest = rendered.as_str();
    while let Some(start) = rest.find([IMPORT, CELL]) {
        let (html, marked) = rest.split_at(start);
        let (marker, after) = marked.split_once('\n').unwrap();
        parts.push(Part::Html(html.to_string()));
        parts.push(match marker.strip_prefix(IMPORT) {
            Some(import) => Part::Import(PathBuf::from(import)),
            None => Part::Cell(next_hash.next().unwrap()),
        });
        rest = after;
    }
    parts.push(Part::Html(rest.to_string()));
    File {
        markdown: markdown.to_string(),
        parts,
        cells,
        hashes,
        lock,
        page: PageSettings {
            aspect_ratio: frontmatter.aspect_ratio(path),
            figures: frontmatter.figures(path),
            theme: frontmatter.theme,
        },
        steps: frontmatter.steps,
    }
}

/// Opens a comment that `html_of` marks where in the markdown what follows
/// it is with, as `<!--@120-->`, by its byte offset, and a newline, for the
/// html to go on as it would without it, rather than add one before a block.
const AT: &str = "<!--@";

/// The html of a file's markdown in `dir`, in `<section>`s, with its imports
/// and its code cells marked with `IMPORT` and `CELL`, its frontmatter, and
/// the code of its cells, in order. With `at`, each block, piece of raw html,
/// math and rule comes after a comment with where it is in the markdown.
fn html_of(markdown: &str, dir: &Path, at: bool) -> (String, String, Vec<String>) {
    let mut metadata = false;
    let mut frontmatter = String::new();
    let mut code: Option<String> = None;
    let mut cells = Vec::new();
    // The events are consumed by the time the cells are needed again.
    let cell_codes = &mut cells;
    let yaml = &mut frontmatter;
    let events = parse(markdown).flat_map(|(event, range, top)| {
        let marked = at
            && matches!(
                event,
                Event::Start(
                    Tag::Paragraph
                        | Tag::Heading { .. }
                        | Tag::BlockQuote(_)
                        | Tag::CodeBlock(_)
                        | Tag::HtmlBlock
                        | Tag::List(_)
                        | Tag::Item
                        | Tag::Table(_)
                ) | Event::InlineHtml(_)
                    | Event::InlineMath(_)
                    | Event::DisplayMath(_)
                    | Event::Rule
            );
        let marker = marked.then(|| {
            (
                Event::Html(format!("{AT}{}-->\n", range.start).into()),
                range.clone(),
                top,
            )
        });
        marker.into_iter().chain(std::iter::once((event, range, top)))
    });
    let events = events.filter_map(move |(event, range, top)| match event {
        // The frontmatter is metadata, not content.
        Event::Start(Tag::MetadataBlock(_)) => {
            metadata = true;
            None
        }
        Event::End(TagEnd::MetadataBlock(_)) => {
            metadata = false;
            None
        }
        Event::Text(text) if metadata => {
            yaml.push_str(&text);
            None
        }
        _ if metadata => None,
        // A tilde-fenced block is a code cell, which stands in for its outputs.
        Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_)))
            if is_tilde_fenced(markdown, range.start) =>
        {
            code = Some(String::new());
            None
        }
        Event::Text(text) if code.is_some() => {
            code.as_mut().unwrap().push_str(&text);
            None
        }
        Event::End(TagEnd::CodeBlock) if code.is_some() => {
            cell_codes.push(code.take().unwrap());
            Some(Event::Html(format!("{CELL}\n").into()))
        }
        // An imported file brings its own slides, so it breaks out of this one.
        Event::Html(raw) if top && raw.contains("<import-slide") => {
            let mut html = String::new();
            for line in raw.lines() {
                match import_src(line) {
                    Some(src) => {
                        let path = canonical(&dir.join(src));
                        html.push_str("</section>\n");
                        html.push(IMPORT);
                        html.push_str(path.to_str().unwrap());
                        html.push_str("\n<section>\n");
                    }
                    None => {
                        html.push_str(line);
                        html.push('\n');
                    }
                }
            }
            Some(Event::Html(html.into()))
        }
        // Every other rule delimits two slides, but within a list or a quote,
        // where it is a rule.
        Event::Rule if top => Some(Event::Html("</section>\n<section>\n".into())),
        Event::InlineMath(tex) => Some(Event::InlineMath(math::number(&tex).into())),
        Event::DisplayMath(tex) => Some(Event::DisplayMath(math::number(&tex).into())),
        event => Some(event),
    });

    let mut rendered = String::from("<section>\n");
    html::push_html(&mut rendered, events);
    rendered.push_str("</section>\n");
    (rendered, frontmatter, cells)
}

/// A slide of a file, as the deck numbers its steps, by line, counted from
/// zero.
#[derive(Debug, PartialEq)]
pub struct SlideSteps {
    /// The line its `<section>` opens at: the first of the file, or the one
    /// after the rule or the import before it.
    pub line: usize,
    /// How many steps it has.
    pub count: u32,
    /// What steps in it, in order, but what shows from the start all along.
    pub steps: Vec<LineStep>,
    /// What is wrong with how it steps, by line, in order.
    pub warnings: Vec<LineWarning>,
    /// Whether it holds nothing, as a break at either end of a file, or two
    /// in a row, leave, which the deck leaves out.
    pub empty: bool,
    /// Its columns, as the deck boxes them, in order.
    pub columns: Vec<LineColumn>,
}

/// A column of a slide, from the line of the heading that starts it to the
/// last line of what it holds, and which of the columns side by side it is,
/// from 0.
#[derive(Debug, PartialEq)]
pub struct LineColumn {
    pub line: usize,
    pub last: usize,
    pub index: usize,
}

/// What is wrong with how a slide steps, on the line it is about.
#[derive(Debug, PartialEq)]
pub struct LineWarning {
    pub line: usize,
    pub message: String,
}

/// What steps in a slide, by the line it begins at, and the steps it shows
/// in, from `from` up to `to`, excluded.
#[derive(Debug, PartialEq)]
pub struct LineStep {
    pub line: usize,
    pub from: u32,
    pub to: Option<u32>,
    pub collapse: bool,
}

/// The steps of a file's slides, numbered as the deck numbers them, for an
/// editor to show where they are, and what is wrong with how they step. A
/// code cell steps as its outputs do, which `outputs` gives, from the code of
/// every cell, in order, as the html the deck shows each cell's as. The
/// frontmatter's `steps` is the file's own, as it is when the file is the
/// deck rather than imported.
pub fn steps(markdown: &str, outputs: impl FnOnce(&[String]) -> Vec<String>) -> Vec<SlideSteps> {
    let (html, frontmatter, cells) = html_of(markdown, Path::new(""), true);
    let steps = Frontmatter::parse(&frontmatter, Path::new("slides.md")).steps;
    steps_of(markdown, html, steps, &outputs(&cells))
}

/// The steps of a file's slides, as `steps` finds them in `html`, the
/// file's as `html_of` marks it, with `steps` its frontmatter's, and each
/// cell's outputs as `outputs` has them.
fn steps_of(
    markdown: &str,
    html: String,
    steps: Option<bool>,
    outputs: &[String],
) -> Vec<SlideSteps> {
    // What steps in a cell's outputs is on the line of the cell, which the
    // outputs are kept on.
    let mut outputs = outputs.iter().map(|html| html.replace('\n', " "));
    let mut html = html
        .split(&format!("{CELL}\n"))
        .enumerate()
        .fold(String::new(), |mut html, (i, part)| {
            if i > 0 {
                html.push_str(&outputs.next().unwrap_or_default());
                html.push('\n');
            }
            html.push_str(part);
            html
        });
    while let Some(start) = html.find(IMPORT) {
        let end = html[start..]
            .find('\n')
            .map_or(html.len(), |end| start + end + 1);
        html.replace_range(start..end, "");
    }
    if steps == Some(false) {
        html = html.replace(SECTION, NO_STEPS);
    }

    let starts: Vec<usize> = std::iter::once(0)
        .chain(markdown.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let line_of = |offset: usize| starts.partition_point(|&start| start <= offset) - 1;
    // Where each comment ends in the html, and the line it marks.
    let marks: Vec<(usize, usize)> = html
        .match_indices(AT)
        .filter_map(|(start, _)| {
            let rest = &html[start + AT.len()..];
            let digits = rest.find("-->")?;
            let offset: usize = rest[..digits].parse().ok()?;
            Some((start + AT.len() + digits + 4, line_of(offset)))
        })
        .collect();
    // The line of what is at `at` in the html: that of the comment before it,
    // and as many more as the lines between them, which the html keeps from
    // the markdown, as soft breaks and raw html do.
    let line = |at: usize| {
        let mark = marks.partition_point(|&(end, _)| end <= at);
        let (from, line) = mark.checked_sub(1).map_or((0, 0), |mark| marks[mark]);
        line + html[from..at].matches('\n').count()
    };
    step::slides(&html)
        .into_iter()
        .map(|slide| SlideSteps {
            line: line(slide.at),
            empty: sections(&html)
                .into_iter()
                .any(|(at, inner, _)| at == slide.at && is_empty_slide(&html[inner])),
            count: slide.count,
            steps: slide
                .steps
                .into_iter()
                .map(|stepped| LineStep {
                    line: line(stepped.at),
                    from: stepped.from,
                    to: stepped.to,
                    collapse: stepped.collapse,
                })
                // A list shows along with its first item, on its line.
                .fold(Vec::new(), |mut steps: Vec<LineStep>, step| {
                    if steps.last() != Some(&step) {
                        steps.push(step);
                    }
                    steps
                }),
            warnings: slide
                .warnings
                .into_iter()
                .map(|warning| LineWarning {
                    line: line(warning.at),
                    message: warning.message,
                })
                .collect(),
            columns: slide
                .columns
                .into_iter()
                .map(|column| LineColumn {
                    line: line(column.at),
                    last: line(column.end),
                    index: column.index,
                })
                .collect(),
        })
        .collect()
}

/// The `<section>`s of html as rendered, each with where it begins, where
/// what it holds is, and where it ends, after the newline that follows it.
fn sections(html: &str) -> Vec<(usize, Range<usize>, usize)> {
    let mut sections = Vec::new();
    let mut from = 0;
    while let Some(found) = html[from..].find("<section") {
        let at = from + found;
        let Some(open) = html[at..].find('>').map(|open| at + open + 1) else {
            break;
        };
        let close = html[open..]
            .find("</section>")
            .map_or(html.len(), |close| open + close);
        let mut end = (close + "</section>".len()).min(html.len());
        if html[end..].starts_with('\n') {
            end += 1;
        }
        sections.push((at, open..close, end));
        from = end;
    }
    sections
}

/// Whether a slide holds nothing, as what its `<section>` holds says, but
/// for where `html_of` marks what is in it.
fn is_empty_slide(inner: &str) -> bool {
    let mut rest = inner;
    while let Some(start) = rest.find(AT) {
        if !rest[..start].trim().is_empty() {
            return false;
        }
        let Some(end) = rest[start..].find("-->") else {
            return false;
        };
        rest = &rest[start + end + "-->".len()..];
    }
    rest.trim().is_empty()
}

/// The html of the slides with the empty ones left out, as a break at either
/// end of a file, or two in a row, leave one.
pub fn without_empty_slides(html: &str) -> String {
    let mut kept = String::with_capacity(html.len());
    let mut from = 0;
    for (at, inner, end) in sections(html) {
        kept.push_str(&html[from..at]);
        if !is_empty_slide(&html[inner]) {
            kept.push_str(&html[at..end]);
        }
        from = end;
    }
    kept.push_str(&html[from..]);
    kept
}

/// Where a line of a file is on its page, as slides.js numbers it.
#[derive(Debug, PartialEq)]
pub struct Position {
    /// Its slide, from 1, among those of the file and those its imports
    /// bring.
    pub slide: usize,
    /// The step it shows at, from 1.
    pub step: u32,
}

/// What reads the files of a deck, for the slides they bring: `load` reads
/// a file's markdown, if there is one, and `outputs` the html of the saved
/// outputs of its cells, from their code, as `steps` takes them.
pub struct Files<L, O> {
    pub load: L,
    pub outputs: O,
}

impl<L, O> Files<L, O>
where
    L: FnMut(&Path) -> Option<String>,
    O: FnMut(&Path, &[String]) -> Vec<String>,
{
    /// Where `line` of the file at `path`, whose markdown is `markdown`, is
    /// on its page: the slide it is in, after those its imports before it
    /// bring, at the step that what steps at or before it in the slide shows
    /// at. On the line of an import, it is at the first slide it brings.
    pub fn position(&mut self, path: &Path, markdown: &str, line: usize) -> Position {
        let slides = steps(markdown, |codes| (self.outputs)(path, codes));
        let mut importing = Importing::default();
        importing.enter(path);
        let mut before = 0;
        for import in breaks(markdown).imports {
            if import.line < line {
                before += self.brought(path, &import.src, &mut importing);
            } else if import.line == line {
                // The slides before the import, and its own.
                let own = slides.iter().filter(|slide| slide.line <= line);
                let slide = before + own.filter(|slide| !slide.empty).count() + 1;
                return Position { slide, step: 1 };
            }
        }
        let own = slides.iter().rposition(|slide| slide.line <= line).unwrap_or(0);
        let slide = before + slides[..own].iter().filter(|slide| !slide.empty).count() + 1;
        // What steps at or before the line, in its slide, shows it.
        let steps = slides.get(own).map_or(&[][..], |slide| &slide.steps[..]);
        let before: Vec<&LineStep> = steps.iter().filter(|step| step.line <= line).collect();
        let last = before.last().map(|step| step.line);
        let step = before
            .iter()
            .filter(|step| Some(step.line) == last)
            .map(|step| step.from)
            .fold(1, u32::max);
        Position { slide, step }
    }

    /// How many slides the file at `path` brings, along with those its
    /// imports bring, but the empty ones, which the deck leaves out.
    pub fn slide_count(&mut self, path: &Path) -> usize {
        self.count(path, &mut Importing::default())
    }

    fn count(&mut self, path: &Path, importing: &mut Importing) -> usize {
        if !importing.enter(path) {
            return 0;
        }
        let Some(markdown) = (self.load)(path) else {
            importing.leave();
            return 0;
        };
        let slides = steps(&markdown, |codes| (self.outputs)(path, codes));
        let mut count = slides.iter().filter(|slide| !slide.empty).count();
        for import in breaks(&markdown).imports {
            count += self.brought(path, &import.src, importing);
        }
        importing.leave();
        count
    }

    /// How many slides an import of `src` in the file at `path` brings.
    fn brought(&mut self, path: &Path, src: &str, importing: &mut Importing) -> usize {
        self.count(&paths::joined(parent(path), src), importing)
    }
}

/// A code cell as written in the markdown.
pub struct CodeCell {
    /// Where it is, from its opening fence to its closing one.
    pub range: Range<usize>,
    /// What follows the opening fence, such as the language.
    pub info: String,
    /// Its code, which its address is the hash of.
    pub code: String,
}

/// The source of a code cell in a notebook, which is its code but for the
/// newline that ends the last line.
pub fn source(code: &str) -> &str {
    code.strip_suffix('\n').unwrap_or(code)
}

/// The code of a notebook's code cell, as the deck reads it from the file,
/// so that it has the same address.
pub fn code(source: &str) -> String {
    if source.is_empty() {
        String::new()
    } else {
        format!("{source}\n")
    }
}

/// The code cells of a file, in order, found as `render` finds them.
pub fn code_cells(markdown: &str) -> Vec<CodeCell> {
    let mut cells: Vec<CodeCell> = Vec::new();
    let mut open = false;
    for (event, range, _) in parse(markdown) {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info)))
                if is_tilde_fenced(markdown, range.start) =>
            {
                open = true;
                cells.push(CodeCell {
                    range,
                    info: info.to_string(),
                    code: String::new(),
                });
            }
            Event::Text(text) if open => cells.last_mut().unwrap().code.push_str(&text),
            Event::End(TagEnd::CodeBlock) => open = false,
            _ => {}
        }
    }
    cells
}

/// The events of a file's markdown, each with where it is in the source,
/// and whether it is outside every list, quote and footnote, where a rule or
/// an import breaks the slide, and a heading is the slide's own.
fn parse(markdown: &str) -> impl Iterator<Item = (Event<'_>, Range<usize>, bool)> {
    let mut depth = 0usize;
    Parser::new_ext(
        markdown,
        Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
            | Options::ENABLE_HEADING_ATTRIBUTES
            | Options::ENABLE_MATH
            | Options::ENABLE_TABLES,
    )
    .into_offset_iter()
    .map(move |(event, range)| {
        match &event {
            Event::Start(Tag::List(_) | Tag::BlockQuote(_) | Tag::FootnoteDefinition(_)) => {
                depth += 1
            }
            Event::End(TagEnd::List(_) | TagEnd::BlockQuote(_) | TagEnd::FootnoteDefinition) => {
                depth -= 1
            }
            _ => {}
        }
        (event, range, depth == 0)
    })
}

/// What a file's frontmatter sets for the whole page, rather than for its own
/// slides.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageSettings {
    pub theme: Option<String>,
    /// The shape the slides keep.
    pub aspect_ratio: Option<AspectRatio>,
    /// How figures are shown, and step.
    pub figures: FigureSettings,
}

/// How figures are shown, and step.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(default)]
pub struct FigureSettings {
    /// Whether figures are shown as the theme filters them, as a dark theme
    /// inverts them, rather than as they were drawn. Unset, they are.
    pub invert: Option<bool>,
    /// Whether the steps inside figures fade, as the others do, rather than
    /// show at once. Unset, they do.
    pub fade: Option<bool>,
    /// How many seconds an animation still playing takes, sped up, to get
    /// where it is going when the next one begins. Unset, 0.2.
    #[cfg_attr(test, schemars(range(min = 0), extend("examples" = [0.2, 0.5, 0])))]
    pub rush: Option<f64>,
}

// It is also src/frontmatter.schema.json, which a test writes from it, and the
// VS Code extension completes the frontmatter from: its doc comments, and those
// of its fields, describe them there.

/// What the YAML frontmatter of a slides-rs deck's markdown sets. Keys the
/// deck does not read are left alone, as a title, an author or notes.
#[derive(Default, serde::Deserialize)]
#[cfg_attr(
    test,
    derive(schemars::JsonSchema),
    schemars(title = "slides-rs frontmatter")
)]
#[serde(default)]
struct Frontmatter {
    /// The theme of the page: one the deck comes with, or the path of a
    /// stylesheet of your own.
    #[cfg_attr(test, schemars(extend("examples" = ["light", "dark", "paper", "projector"])))]
    theme: Option<String>,
    /// The shape the slides keep, as 16:9, 4:3 or a number, or none, to fill
    /// the window.
    #[serde(rename = "aspect-ratio")]
    #[cfg_attr(test, schemars(extend("examples" = ["16:9", "4:3", "16/10", 1.6, "none"])))]
    aspect_ratio: Option<aspect::Written>,
    /// Whether to step through the slides of this file, and of the files it
    /// imports that do not say. Unset, it does.
    steps: Option<bool>,
    /// How figures are shown, and step.
    figures: FigureSettings,
}

impl Frontmatter {
    /// Frontmatter that is not valid YAML is reported and left out, rather
    /// than keeping the rest of the file from rendering.
    fn parse(yaml: &str, path: &Path) -> Frontmatter {
        if yaml.trim().is_empty() {
            return Frontmatter::default();
        }
        serde_saphyr::from_str(yaml).unwrap_or_else(|error| {
            eprintln!("{}: invalid frontmatter: {error}", path.display());
            Frontmatter::default()
        })
    }

    /// Whether it sets anything the deck reads.
    fn is_read(&self) -> bool {
        let Frontmatter {
            theme,
            aspect_ratio,
            steps,
            figures,
        } = self;
        theme.is_some()
            || aspect_ratio.is_some()
            || steps.is_some()
            || *figures != FigureSettings::default()
    }

    /// The shape the slides keep, if it says one, which is reported and left
    /// out if it is not a ratio.
    fn aspect_ratio(&self, path: &Path) -> Option<AspectRatio> {
        let written = self.aspect_ratio.as_ref()?;
        let ratio = written.aspect_ratio();
        if ratio.is_none() {
            eprintln!(
                "{}: aspect-ratio {written} is not one, as 16:9, 1.6 or none",
                path.display()
            );
        }
        ratio
    }

    /// How figures are shown and step, with a time to rush in that is not
    /// one, as a negative number of seconds, reported and left out.
    fn figures(&self, path: &Path) -> FigureSettings {
        let mut figures = self.figures.clone();
        if let Some(rush) = figures.rush
            && !(rush.is_finite() && rush >= 0.0)
        {
            eprintln!(
                "{}: figures.rush {rush} is not a number of seconds",
                path.display()
            );
            figures.rush = None;
        }
        figures
    }
}

/// Where a file's slides break, by line, counted from zero, as `render` breaks
/// them: at each rule, and at each import, which brings slides of its own.
#[derive(Debug, Default, PartialEq)]
pub struct Breaks {
    /// The first line after the frontmatter, where the first slide begins.
    pub start: usize,
    /// The lines of the rules between two slides.
    pub rules: Vec<usize>,
    /// The imports, by line.
    pub imports: Vec<Import>,
    /// The headings of the slides, rather than those in a list or a quote.
    pub headings: Vec<Heading>,
    /// Whether it has what only a deck, or a part of one, has: frontmatter
    /// the deck reads, an import or a code cell.
    pub deck: bool,
}

/// A heading of a slide, by its first and last lines, two for one
/// underlined, and its level, from 1.
#[derive(Debug, PartialEq)]
pub struct Heading {
    pub line: usize,
    pub last: usize,
    pub level: u8,
}

/// An import of a file's slides, by its line, and the file it imports, as
/// its `src` says, relative to the importing file.
#[derive(Debug, PartialEq)]
pub struct Import {
    pub line: usize,
    pub src: String,
}

/// Where the slides of a file's markdown break.
pub fn breaks(markdown: &str) -> Breaks {
    let starts: Vec<usize> = std::iter::once(0)
        .chain(markdown.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let line = |offset: usize| starts.partition_point(|&start| start <= offset) - 1;
    let mut breaks = Breaks::default();
    let mut yaml = String::new();
    let mut metadata = false;
    for (event, range, top) in parse(markdown) {
        match &event {
            Event::Start(Tag::MetadataBlock(_)) => metadata = true,
            Event::Text(text) if metadata => yaml.push_str(text),
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_)))
                if is_tilde_fenced(markdown, range.start) =>
            {
                breaks.deck = true
            }
            _ => {}
        }
        if !top {
            continue;
        }
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                breaks.headings.push(Heading {
                    line: line(range.start),
                    last: line(range.end.saturating_sub(1)),
                    level: level as u8,
                })
            }
            Event::End(TagEnd::MetadataBlock(_)) => {
                metadata = false;
                // The block ends with its closing fence, or just after it.
                breaks.start = line(range.end.saturating_sub(1)) + 1;
            }
            Event::Rule => breaks.rules.push(line(range.start)),
            Event::Html(raw) if raw.contains("<import-slide") => {
                let first = line(range.start);
                for (i, text) in raw.lines().enumerate() {
                    if let Some(src) = import_src(text) {
                        breaks.imports.push(Import {
                            line: first + i,
                            src: src.to_string(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    breaks.deck |= !breaks.imports.is_empty()
        || Frontmatter::parse(&yaml, Path::new("slides.md")).is_read();
    breaks
}

/// The `src` of an `<import-slide src="..." />`, relative to the importing file.
fn import_src(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix("<import-slide")?;
    let rest = rest.trim_start().strip_prefix("src=\"")?;
    let (src, _) = rest.split_once('"')?;
    Some(src)
}

/// The parser normalizes both fence styles, so the source says which one it was.
fn is_tilde_fenced(markdown: &str, start: usize) -> bool {
    markdown[start..].trim_start().starts_with('~')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slides_break_at_rules_and_imports_but_not_at_what_looks_like_one() {
        let markdown = "---\ntheme: dark\n---\n# One\n\n---\n\nA heading\n---\n\n```\n---\n```\n\n***\n<import-slide src=\"part.md\" />\n\n> # quoted\n";
        let heading = |line, last, level| Heading { line, last, level };
        assert_eq!(
            breaks(markdown),
            Breaks {
                start: 3,
                rules: vec![5, 14],
                imports: vec![Import {
                    line: 15,
                    src: "part.md".into()
                }],
                headings: vec![heading(3, 3, 1), heading(7, 8, 2)],
                deck: true,
            }
        );
        assert_eq!(breaks("# One\n").start, 0);
    }

    #[test]
    fn a_deck_has_frontmatter_it_reads_an_import_or_a_code_cell() {
        assert!(!breaks("---\ntitle: Notes\n---\n# One\n\n```python\nx\n```\n").deck);
        assert!(breaks("---\nsteps: false\n---\n# One\n").deck);
        assert!(breaks("# One\n\n- a\n\n  ~~~python\n  x\n  ~~~\n").deck);
        assert!(breaks("<import-slide src=\"part.md\" />\n").deck);
        assert!(!breaks("```\n<import-slide src=\"part.md\" />\n```\n").deck);
    }

    #[test]
    fn a_rule_or_an_import_in_a_list_or_a_quote_is_no_break() {
        let markdown = "# One\n\n> a\n>\n> ***\n\n- b\n\n  ---\n\n  <import-slide src=\"part.md\" />\n";
        let breaks = breaks(markdown);
        assert_eq!((breaks.rules, breaks.imports), (vec![], vec![]));
        let html = render(markdown, Path::new("/deck/slides.md"));
        let [Part::Html(html)] = html.parts.as_slice() else {
            panic!("{:?}", html.parts.len());
        };
        assert_eq!(html.matches("<section>").count(), 1, "{html}");
        assert!(html.contains("<hr />"), "{html}");
    }

    /// Each slide's line and count, and each step, as `line from..to`, with
    /// an output of each cell, as `<pre>` is.
    fn lines(markdown: &str) -> Vec<String> {
        lines_with(markdown, |cells| vec!["<pre>\n</pre>\n".into(); cells.len()])
    }

    /// The same, with the outputs of the cells as `outputs` gives them.
    fn lines_with(
        markdown: &str,
        outputs: impl FnOnce(&[String]) -> Vec<String>,
    ) -> Vec<String> {
        steps(markdown, outputs)
            .into_iter()
            .flat_map(|slide| {
                let steps = slide.steps.into_iter().map(|step| {
                    let to = step.to.map_or(String::new(), |to| to.to_string());
                    format!("{} {}..{to}", step.line, step.from)
                });
                std::iter::once(format!("slide {} count {}", slide.line, slide.count)).chain(steps)
            })
            .collect()
    }

    #[test]
    fn the_steps_are_found_by_the_line_they_are_written_on() {
        let markdown = "---\ntheme: dark\n---\n# One\n\ntext\n\n- a\n- b\n\n---\n\n# Two { steps=parallel }\n\n### L\n\n- x\n\n### R\n\n<p step=\"..3\"\n  collapse>y</p>\n\n$$\na \\step{b}\n$$\n";
        assert_eq!(
            lines(markdown),
            [
                "slide 0 count 4",
                "5 2..",
                "7 3..",
                "8 4..",
                // The columns' headings show with the title, and a range of
                // a column counts from its heading, 0.
                "slide 11 count 4",
                "16 2..",
                "20 1..4",
                "23 2..",
                "24 3..",
            ]
        );
    }

    #[test]
    fn a_name_given_again_is_warned_of_on_its_line() {
        let markdown = "# One\n\n### A { step=a }\n\n$$\n\\step[a]{x}\n$$\n";
        let warnings: Vec<usize> = steps(markdown, |_| Vec::new())
            .into_iter()
            .flat_map(|slide| slide.warnings)
            .map(|warning| warning.line)
            .collect();
        assert_eq!(warnings, [5]);
    }

    #[test]
    fn a_code_cell_is_a_step_and_an_import_ends_the_slide() {
        let markdown =
            "# One\n\n~~~python\nx = 1\n~~~\n\n<import-slide src=\"part.md\" />\n\ntext\n";
        assert_eq!(
            lines(markdown),
            ["slide 0 count 2", "2 2..", "slide 7 count 1"]
        );
    }

    #[test]
    fn an_empty_slide_is_left_out() {
        let html = "<section>\n</section>\n<section data-steps=\"false\">\n  \n</section>\n<section>\n<p>a</p>\n</section>\n<section>\n<!-- note -->\n</section>\n";
        assert_eq!(
            without_empty_slides(html),
            "<section>\n<p>a</p>\n</section>\n<section>\n<!-- note -->\n</section>\n"
        );
    }

    /// The files of a deck, by path, with each cell's outputs as `<p>`, but
    /// for a cell that prints nothing.
    fn files(
        files: &[(&str, &str)],
    ) -> Files<impl FnMut(&Path) -> Option<String>, impl FnMut(&Path, &[String]) -> Vec<String>>
    {
        let files: Vec<(PathBuf, String)> = files
            .iter()
            .map(|&(path, markdown)| (PathBuf::from(path), markdown.to_string()))
            .collect();
        Files {
            load: move |path: &Path| {
                let file = files.iter().find(|(file, _)| file == path);
                file.map(|(_, markdown)| markdown.clone())
            },
            outputs: |_: &Path, codes: &[String]| {
                let output = |code: &String| match code.as_str() {
                    "pass\n" => String::new(),
                    _ => "<p>out</p>\n".to_string(),
                };
                codes.iter().map(output).collect()
            },
        }
    }

    #[test]
    fn a_file_brings_its_slides_and_those_its_imports_do_but_the_empty_ones() {
        let mut files = files(&[
            ("/deck/index.md", "# A\n\n<import-slide src=\"part/b.md\" />\n\n---\n\n~~~\npass\n~~~\n"),
            ("/deck/part/b.md", "# B\n\n---\n\n# C\n\n<import-slide src=\"../index.md\" />\n"),
        ]);
        // A slide of a cell that shows nothing is left out, and a file
        // imported within itself brings nothing.
        assert_eq!(files.slide_count(Path::new("/deck/index.md")), 3);
        assert_eq!(files.slide_count(Path::new("/deck/part/b.md")), 3);
        assert_eq!(files.slide_count(Path::new("/deck/none.md")), 0);
    }

    #[test]
    fn a_line_is_on_its_slide_after_those_its_imports_bring() {
        let markdown = "# A\n\ntext\n\n<import-slide src=\"b.md\" />\n\n# D\n\n---\n\n---\n\n# E\n";
        let mut files = files(&[("/deck/b.md", "# B\n\n---\n\n# C\n")]);
        let mut at = |line| files.position(Path::new("/deck/a.md"), markdown, line);
        let position = |slide, step| Position { slide, step };
        assert_eq!(at(0), position(1, 1));
        assert_eq!(at(2), position(1, 2));
        assert_eq!(at(3), position(1, 2));
        assert_eq!(at(4), position(2, 1));
        assert_eq!(at(6), position(4, 1));
        // The empty slide between two rules is left out.
        assert_eq!(at(12), position(5, 1));
    }

    #[test]
    fn the_columns_are_found_by_their_lines() {
        let markdown = "# One\n\n### A\n\n- a\n- b\n\n### B\n\ntext\non two lines\n\n## Below\n";
        let columns: Vec<_> = steps(markdown, |_| Vec::new())
            .into_iter()
            .flat_map(|slide| slide.columns)
            .map(|column| (column.line, column.last, column.index))
            .collect();
        assert_eq!(columns, [(2, 5, 0), (7, 10, 1)]);
    }

    #[test]
    fn a_code_cell_steps_as_its_outputs_do() {
        let markdown = "# One\n\n~~~python\nx = 1\n~~~\n\n~~~python\nx\ny\n~~~\n\ntext\n";
        let outputs = |cells: &[String]| {
            assert_eq!(cells, ["x = 1\n", "x\ny\n"]);
            vec![
                String::new(),
                "<pre>\nx\n</pre>\n<svg data-count=\"2\">\n<g step=\"\"/>\n</svg>\n".into(),
            ]
        };
        // The first cell shows nothing, and is no step; the second shows
        // two outputs, the figure with a step of its own, on the cell's line.
        assert_eq!(
            lines_with(markdown, outputs),
            ["slide 0 count 5", "6 2..", "6 3..", "6 4..", "11 5.."]
        );
    }

    fn theme(yaml: &str) -> Option<String> {
        Frontmatter::parse(yaml, Path::new("slides.md")).theme
    }

    #[test]
    fn the_theme_comes_from_the_frontmatter() {
        let markdown = "---\ntitle: Talk\ntheme: dark\n---\n# Slide\n";
        let file = render(markdown, Path::new("slides.md"));
        assert_eq!(file.page.theme.as_deref(), Some("dark"));
    }

    #[test]
    fn a_theme_is_read_as_yaml() {
        assert_eq!(theme("theme: \"paper\"").as_deref(), Some("paper"));
        assert_eq!(
            theme("theme: 'my theme.css'").as_deref(),
            Some("my theme.css")
        );
        assert_eq!(
            theme("theme: light # for daytime").as_deref(),
            Some("light")
        );
    }

    #[test]
    fn without_a_theme_there_is_none() {
        assert_eq!(theme(""), None);
        assert_eq!(theme("title: Talk"), None);
        assert_eq!(theme("theme:"), None);
        // An indented key belongs to some other mapping.
        assert_eq!(theme("slides:\n  theme: dark"), None);
    }

    #[test]
    fn invalid_frontmatter_is_left_out() {
        assert_eq!(theme("theme: [dark"), None);
        assert_eq!(theme("theme: [dark]"), None);
    }

    #[test]
    fn the_aspect_ratio_comes_from_the_frontmatter() {
        let ratio = |yaml| {
            Frontmatter::parse(yaml, Path::new("slides.md")).aspect_ratio(Path::new("slides.md"))
        };
        let fixed = |width, height| Some(AspectRatio::Fixed { width, height });
        assert_eq!(ratio("aspect-ratio: 16:9"), fixed(16.0, 9.0));
        assert_eq!(ratio("aspect-ratio: \"4:3\""), fixed(4.0, 3.0));
        assert_eq!(ratio("aspect-ratio: 16/10"), fixed(16.0, 10.0));
        assert_eq!(ratio("aspect-ratio: 1.6"), fixed(1.6, 1.0));
        assert_eq!(ratio("aspect-ratio: 2"), fixed(2.0, 1.0));
        assert_eq!(ratio("aspect-ratio: none"), Some(AspectRatio::Fill));
        assert_eq!(ratio("aspect-ratio: wide"), None);
        assert_eq!(ratio("theme: dark"), None);
    }

    #[test]
    fn figures_can_be_shown_as_drawn_without_fading_and_rush_as_long_as_said() {
        let figures = |markdown| render(markdown, Path::new("slides.md")).page.figures;
        let set =
            figures("---\nfigures:\n  invert: false\n  fade: false\n  rush: 0.5\n---\n# Slide\n");
        assert_eq!(
            set,
            FigureSettings {
                invert: Some(false),
                fade: Some(false),
                rush: Some(0.5),
            }
        );
        assert_eq!(figures("# Slide\n"), FigureSettings::default());
        assert_eq!(figures("---\nfigures:\n  rush: 1\n---\n").rush, Some(1.0));
        assert_eq!(figures("---\nfigures:\n  rush: -1\n---\n").rush, None);
    }

    /// src/frontmatter.schema.json is written from the frontmatter as it is read,
    /// with what each key holds in place, for the VS Code extension to follow.
    /// Out of date, it is written anew, and the test fails, for it to be seen.
    #[test]
    fn the_schema_is_the_frontmatter_as_read() {
        let generator = schemars::generate::SchemaSettings::draft07()
            .with(|settings| settings.inline_subschemas = true)
            .into_generator();
        let schema = generator.into_root_schema_for::<Frontmatter>();
        let json = serde_json::to_string_pretty(&schema).unwrap() + "\n";
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/frontmatter.schema.json");
        if std::fs::read_to_string(&path).ok().as_deref() != Some(json.as_str()) {
            std::fs::write(&path, &json).unwrap();
            panic!("{} was out of date, and is written anew", path.display());
        }
    }

    #[test]
    fn steps_are_set_in_the_frontmatter() {
        let steps = |yaml| Frontmatter::parse(yaml, Path::new("slides.md")).steps;
        assert_eq!(steps("steps: false"), Some(false));
        assert_eq!(steps("steps: true"), Some(true));
        assert_eq!(steps("theme: dark"), None);
        assert_eq!(steps(""), None);
    }

    #[test]
    fn the_code_cells_are_the_ones_rendered() {
        let markdown = "---\ntitle: ~~~\n---\n~~~python\na = 1\n~~~\n\n```python\nnot a cell\n```\n\n~~~\n~~~\n";
        let cells = code_cells(markdown);
        let codes: Vec<&str> = cells.iter().map(|cell| cell.code.as_str()).collect();
        assert_eq!(codes, render(markdown, Path::new("slides.md")).cells);
        assert_eq!(codes, ["a = 1\n", ""]);
        assert_eq!(cells[0].info, "python");
        assert!(markdown[cells[0].range.clone()].starts_with("~~~python\n"));
        assert!(markdown[cells[1].range.clone()].trim_end().ends_with("~~~"));
    }

    #[test]
    fn a_heading_can_turn_steps_off_with_an_attribute() {
        let file = render("# Title { steps=false }\n", Path::new("slides.md"));
        let Part::Html(html) = &file.parts[0] else {
            panic!("the slide renders as html");
        };
        assert!(html.contains("<h1 steps=\"false\">Title</h1>"));
    }
}
