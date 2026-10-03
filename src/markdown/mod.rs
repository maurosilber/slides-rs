//! Rendering one markdown file into slides, apart from the files it imports
//! and the outputs of its code cells, which are filled in later.

mod math;

use std::ops::Range;
use std::path::{Path, PathBuf};

use pulldown_cmark::{CodeBlockKind, Event, OffsetIter, Options, Parser, Tag, TagEnd, html};

use crate::aspect::{self, AspectRatio};
use crate::paths::{canonical, parent};
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
    let (marked, _, _) = html_of(markdown, parent(path), true);
    for slide in steps_of(markdown, marked, frontmatter.steps) {
        for warning in slide.warnings {
            let line = warning.line + 1;
            eprintln!("{}:{line}: {}", path.display(), warning.message);
        }
    }
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
    let events = parse(markdown).flat_map(|(event, range)| {
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
            )
        });
        marker.into_iter().chain(std::iter::once((event, range)))
    });
    let events = events.filter_map(move |(event, range)| match event {
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
        Event::Html(raw) if raw.contains("<import-slide") => {
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
        // Every other rule delimits two slides.
        Event::Rule => Some(Event::Html("</section>\n<section>\n".into())),
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
/// editor to show where they are. A code cell is a step, as its outputs are
/// when it has one, and the frontmatter's `steps` is the file's own, as it is
/// when the file is the deck rather than imported.
pub fn steps(markdown: &str) -> Vec<SlideSteps> {
    let (html, frontmatter, _) = html_of(markdown, Path::new(""), true);
    let steps = Frontmatter::parse(&frontmatter, Path::new("slides.md")).steps;
    steps_of(markdown, html, steps)
}

/// The steps of a file's slides, as `steps` finds them in `html`, the
/// file's as `html_of` marks it, with `steps` its frontmatter's.
fn steps_of(markdown: &str, html: String, steps: Option<bool>) -> Vec<SlideSteps> {
    let mut html = html.replace(&format!("{CELL}\n"), "<div></div>\n");
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
        })
        .collect()
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
    for (event, range) in parse(markdown) {
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

/// The events of a file's markdown, each with where it is in the source.
fn parse(markdown: &str) -> OffsetIter<'_> {
    Parser::new_ext(
        markdown,
        Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
            | Options::ENABLE_HEADING_ATTRIBUTES
            | Options::ENABLE_MATH
            | Options::ENABLE_TABLES,
    )
    .into_offset_iter()
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
    /// The lines of the imports.
    pub imports: Vec<usize>,
    /// The headings of the slides, which slides.js makes columns of, rather
    /// than those in a list or a quote.
    pub headings: Vec<Heading>,
}

/// A heading of a slide, by its first and last lines, two for one
/// underlined, and its level, from 1.
#[derive(Debug, PartialEq)]
pub struct Heading {
    pub line: usize,
    pub last: usize,
    pub level: u8,
}

/// Where the slides of a file's markdown break.
pub fn breaks(markdown: &str) -> Breaks {
    let starts: Vec<usize> = std::iter::once(0)
        .chain(markdown.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let line = |offset: usize| starts.partition_point(|&start| start <= offset) - 1;
    let mut breaks = Breaks::default();
    // How many lists and quotes the events are in.
    let mut depth = 0;
    for (event, range) in parse(markdown) {
        match event {
            Event::Start(Tag::List(_) | Tag::BlockQuote(_) | Tag::FootnoteDefinition(_)) => {
                depth += 1
            }
            Event::End(TagEnd::List(_) | TagEnd::BlockQuote(_) | TagEnd::FootnoteDefinition) => {
                depth -= 1
            }
            Event::Start(Tag::Heading { level, .. }) if depth == 0 => {
                breaks.headings.push(Heading {
                    line: line(range.start),
                    last: line(range.end.saturating_sub(1)),
                    level: level as u8,
                })
            }
            Event::End(TagEnd::MetadataBlock(_)) => {
                // The block ends with its closing fence, or just after it.
                breaks.start = line(range.end.saturating_sub(1)) + 1;
            }
            Event::Rule => breaks.rules.push(line(range.start)),
            Event::Html(raw) if raw.contains("<import-slide") => {
                let first = line(range.start);
                for (i, text) in raw.lines().enumerate() {
                    if import_src(text).is_some() {
                        breaks.imports.push(first + i);
                    }
                }
            }
            _ => {}
        }
    }
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
                imports: vec![15],
                headings: vec![heading(3, 3, 1), heading(7, 8, 2)],
            }
        );
        assert_eq!(breaks("# One\n").start, 0);
    }

    /// Each slide's line and count, and each step, as `line from..to`.
    fn lines(markdown: &str) -> Vec<String> {
        steps(markdown)
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
        let warnings: Vec<usize> = steps(markdown)
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
