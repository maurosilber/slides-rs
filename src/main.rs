use std::fs;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html};

const TEMPLATE: &str = r#"<html>

<head>
    <link rel="stylesheet" href="slides.css">
</head>

<body>
{body}
</body>

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
    let events = Parser::new_ext(markdown, Options::ENABLE_YAML_STYLE_METADATA_BLOCKS).filter_map(
        move |event| match event {
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
            // Every other rule delimits two slides.
            Event::Rule => Some(Event::Html("</section>\n<section>\n".into())),
            event => Some(event),
        },
    );

    let mut rendered = String::from("<section>\n");
    html::push_html(&mut rendered, events);
    rendered.push_str("</section>");
    rendered
}
