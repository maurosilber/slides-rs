//! The html page of the deck: its slides inside the template, and the saved
//! outputs of its cells as html.

use std::fs;
use std::path::Path;

use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd, html};

use crate::markdown::{NO_FRAGMENTS, SECTION};
use crate::paths::parent;
use crate::store;

const TEMPLATE: &str = include_str!("template.html");

/// The page for the slides in `body`, in the theme the input asks for, to be
/// written at `output`.
pub fn page(body: &str, theme: Option<&str>, output: &Path) -> String {
    // A slide break at either end of a file, or two in a row, leaves an empty slide.
    let body = body
        .replace(&format!("{SECTION}\n</section>\n"), "")
        .replace(&format!("{NO_FRAGMENTS}\n</section>\n"), "");
    let body = indent(&body);
    let theme = match theme {
        Some(theme) => {
            let href = theme_href(theme);
            if !parent(output).join(&href).is_file() {
                eprintln!("{}: no theme {href} next to it", output.display());
            }
            format!("    <link rel=\"stylesheet\" href=\"{}\">\n", escape(&href))
        }
        None => String::new(),
    };
    TEMPLATE
        .replace("    {theme}\n", &theme)
        .replace("{body}", &body)
}

/// The stylesheet a theme names, relative to the html: one of the
/// `theme-<name>.css` next to `slides.css`, or a stylesheet of its own.
fn theme_href(theme: &str) -> String {
    if theme.ends_with(".css") {
        theme.to_string()
    } else {
        format!("theme-{theme}.css")
    }
}

/// Escapes text for an html attribute.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

/// The outputs a cell saved under `root`, in the order the kernel produced
/// them. Raster images are linked, relative to the html, and the rest is
/// inlined: an SVG too, so that the slides can reach into it for fragments.
pub fn cell_html(root: &Path, hash: &str) -> String {
    let Ok(files) = store::saved(root, hash) else {
        // The cell has not run, or its notebook failed; its error was reported then.
        return format!("<!-- no outputs for cell {hash} -->\n");
    };
    let mut html = String::new();
    for file in files {
        let name = file.file_name().unwrap().to_str().unwrap();
        let extension = file.extension().and_then(|extension| extension.to_str());
        if let Some("png" | "jpeg" | "gif") = extension {
            html.push_str(&format!("<img src=\"{}/{name}\">\n", store::DIR));
            continue;
        }
        let Ok(mut text) = fs::read_to_string(&file) else {
            eprintln!("{}: could not read", file.display());
            continue;
        };
        if !text.ends_with('\n') {
            text.push('\n');
        }
        match extension {
            // SVGs were prepared for inlining when they were saved.
            Some("html" | "svg") => html.push_str(&text),
            Some("md") => html::push_html(&mut html, Parser::new(&text)),
            // Plain text keeps its layout, and is escaped on the way in.
            _ => html::push_html(
                &mut html,
                [
                    Event::Start(Tag::CodeBlock(CodeBlockKind::Indented)),
                    Event::Text(text.into()),
                    Event::End(TagEnd::CodeBlock),
                ]
                .into_iter(),
            ),
        }
    }
    html
}

/// Indents the html to sit inside the template. Indentation is cosmetic
/// everywhere but inside `<pre>`, where it is content.
fn indent(html: &str) -> String {
    let mut indented = String::new();
    let mut preformatted = false;
    for line in html.lines() {
        if !preformatted {
            indented.push_str(match line {
                SECTION | NO_FRAGMENTS | "</section>" => "    ",
                _ => "        ",
            });
        }
        indented.push_str(line);
        indented.push('\n');
        preformatted = (preformatted || line.contains("<pre")) && !line.contains("</pre>");
    }
    indented
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_theme_names_a_bundled_stylesheet_or_its_own() {
        assert_eq!(theme_href("dark"), "theme-dark.css");
        assert_eq!(theme_href("custom/talk.css"), "custom/talk.css");
    }
}
