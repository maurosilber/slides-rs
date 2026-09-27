//! Rendering one markdown file into slides, apart from the files it imports
//! and the outputs of its code cells, which are filled in later.

mod math;

use std::ops::Range;
use std::path::{Path, PathBuf};

use pulldown_cmark::{
    CodeBlockKind, Event, HeadingLevel, OffsetIter, Options, Parser, Tag, TagEnd, html,
};

use crate::aspect::{self, AspectRatio};
use crate::paths::{canonical, parent};
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
    let dir = parent(path);
    let mut metadata = false;
    let mut frontmatter = String::new();
    let mut code: Option<String> = None;
    let mut steps = math::Steps::default();
    let mut cells = Vec::new();
    // The events are consumed by the time the cells are needed again.
    let cell_codes = &mut cells;
    let yaml = &mut frontmatter;
    let events = parse(markdown).filter_map(move |(event, range)| match event {
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
                        steps.reset();
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
        Event::Rule => {
            steps.reset();
            Some(Event::Html("</section>\n<section>\n".into()))
        }
        // Each h2 and h3 starts a column, which counts its steps from one.
        event @ Event::Start(Tag::Heading {
            level: HeadingLevel::H2 | HeadingLevel::H3,
            ..
        }) => {
            steps.reset();
            Some(event)
        }
        Event::InlineMath(tex) => Some(Event::InlineMath(steps.number(&tex).into())),
        Event::DisplayMath(tex) => Some(Event::DisplayMath(steps.number(&tex).into())),
        event => Some(event),
    });

    let mut rendered = String::from("<section>\n");
    html::push_html(&mut rendered, events);
    rendered.push_str("</section>\n");

    // Split at the markers, so every file is cached apart from the ones it
    // imports, and apart from the outputs of its cells, which may come later.
    let frontmatter = Frontmatter::parse(&frontmatter, path);
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

/// What the frontmatter's `figures` sets.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct FigureSettings {
    /// Whether figures are shown as the theme filters them, as a dark theme
    /// inverts them, rather than as they were drawn. Unset, they are.
    pub invert: Option<bool>,
    /// Whether the steps inside figures fade, as the others do, rather than
    /// show at once. Unset, they do.
    pub fade: Option<bool>,
    /// How many seconds an animation still playing takes, sped up, to get
    /// where it is going when the next one begins. Unset, the deck's own.
    pub rush: Option<f64>,
}

/// What a file's frontmatter sets. Keys the deck does not read are ignored,
/// so the frontmatter can hold a title, an author, or notes of its own.
#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct Frontmatter {
    theme: Option<String>,
    /// The shape of the slides.
    #[serde(rename = "aspect-ratio")]
    aspect_ratio: Option<aspect::Written>,
    figures: FigureSettings,
    /// Whether to step through the file's slides, and those of the files it
    /// imports that do not say. Unset, they are stepped through.
    steps: Option<bool>,
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
