use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use clap::Parser as _;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd, html};

const TEMPLATE: &str = include_str!("template.html");

/// Renders a markdown file into an HTML slide deck.
#[derive(clap::Parser)]
struct Cli {
    /// The markdown file to render.
    input: PathBuf,
    /// Where to write the HTML. Defaults to the input with an `.html` extension.
    output: Option<PathBuf>,
}

fn main() {
    let cli = Cli::parse();
    let markdown = fs::read_to_string(&cli.input).unwrap();
    // Imports in the input are relative to it.
    let body = render(&markdown, cli.input.parent().unwrap());
    let output = cli.output.unwrap_or_else(|| cli.input.with_extension("html"));
    fs::write(output, TEMPLATE.replace("{body}", &body)).unwrap();
}

/// Renders the markdown as one `<section>` per slide, indented to sit in the template.
fn render(markdown: &str, dir: &Path) -> String {
    // A slide break at either end of the document, or two in a row, leaves an empty slide.
    let rendered = sections(markdown, dir).replace("<section>\n</section>\n", "");

    // Indentation is cosmetic everywhere but inside `<pre>`, where it is content.
    let mut indented = String::new();
    let mut preformatted = false;
    for line in rendered.lines() {
        if !preformatted {
            indented.push_str(match line {
                "<section>" | "</section>" => "    ",
                _ => "        ",
            });
        }
        indented.push_str(line);
        indented.push('\n');
        preformatted = (preformatted || line.contains("<pre")) && !line.contains("</pre>");
    }
    indented
}

/// Renders the markdown into `<section>`s, dropping the YAML frontmatter
/// and splitting on horizontal rules. Imports are rendered in place.
fn sections(markdown: &str, dir: &Path) -> String {
    let mut metadata = false;
    let mut code: Option<String> = None;
    let events = Parser::new_ext(
        markdown,
        Options::ENABLE_YAML_STYLE_METADATA_BLOCKS | Options::ENABLE_HEADING_ATTRIBUTES,
    )
    .into_offset_iter()
    .filter_map(move |(event, range)| match event {
        // The frontmatter is metadata, not content.
        Event::Start(Tag::MetadataBlock(_)) => {
            metadata = true;
            None
        }
        Event::End(TagEnd::MetadataBlock(_)) => {
            metadata = false;
            None
        }
        _ if metadata => None,
        // A tilde-fenced block is collected, and stands in for its hash.
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
            let hash = hash(&code.take().unwrap());
            Some(Event::Html(format!("<p>{hash:016x}</p>\n").into()))
        }
        // An imported file brings its own slides, so it breaks out of this one.
        Event::Html(raw) if raw.contains("<import-slide") => {
            let mut html = String::new();
            for line in raw.lines() {
                match import_src(line) {
                    Some(src) => {
                        let path = dir.join(src);
                        let imported = fs::read_to_string(&path).unwrap();
                        html.push_str("</section>\n");
                        html.push_str(&sections(&imported, path.parent().unwrap()));
                        html.push_str("<section>\n");
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
        event => Some(event),
    });

    let mut rendered = String::from("<section>\n");
    html::push_html(&mut rendered, events);
    rendered.push_str("</section>\n");
    rendered
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

/// Identifies a code block by its content, so its output can be cached.
fn hash(code: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    code.hash(&mut hasher);
    hasher.finish()
}
