use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd, html};

const TEMPLATE: &str = r#"<!DOCTYPE html>
<html>

<head>
    <meta charset="UTF-8">
    <link rel="stylesheet" href="slides.css">
    <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/katex@0.18.7/dist/katex.min.css"
        integrity="sha384-JctiRyLzXCrSoOOzFlSoWLdyzQl7OrrRnhyeBmzB6ZWtcjccUyc8lCQJqIbs3uQX" crossorigin="anonymous">
    <script type="module">
        import renderMathInElement from "https://cdn.jsdelivr.net/npm/katex@0.18.7/dist/contrib/auto-render.mjs";
        renderMathInElement(document.body, {
            "delimiters": [
                { left: "$$", right: "$$", display: true },
                { left: "$", right: "$", display: false },
            ]
        });
    </script>
</head>

<body>
{body}</body>

<script src="slides.js"></script>

</html>
"#;

fn main() {
    let markdown = fs::read_to_string("index.md").unwrap();
    let body = render(&markdown);
    fs::write("index.html", TEMPLATE.replace("{body}", &body)).unwrap();
}

/// Renders the markdown as one `<section>` per slide,
/// dropping the YAML frontmatter and splitting on horizontal rules.
fn render(markdown: &str) -> String {
    let mut metadata = false;
    let mut code: Option<String> = None;
    let events = Parser::new_ext(
        markdown,
        Options::ENABLE_YAML_STYLE_METADATA_BLOCKS,
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
        // Every other rule delimits two slides.
        Event::Rule => Some(Event::Html("</section>\n<section>\n".into())),
        event => Some(event),
    });

    let mut rendered = String::from("<section>\n");
    html::push_html(&mut rendered, events);
    rendered.push_str("</section>\n");
    // A rule at either end of the document, or two in a row, leaves an empty slide.
    let rendered = rendered.replace("<section>\n</section>\n", "");

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
