//! The html page of the deck: its slides inside the template, and the saved
//! outputs of its cells as html.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use crate::markdown::{self, PageSettings};
use crate::paths::{href, parent};
use crate::{step, store};

const TEMPLATE: &str = include_str!("template.html");

/// The stylesheets and the script a page can link, written into the outputs
/// directory next to it as it needs them: the slides' own, and the themes a
/// deck can name.
const STATIC: &[(&str, &str)] = &[
    ("slides.css", include_str!("../static/slides.css")),
    // The steps are a file of their own, which the extension's notebook
    // renderer shares.
    (
        "slides.js",
        concat!(
            include_str!("../static/steps.js"),
            "\n",
            include_str!("../static/slides.js")
        ),
    ),
    ("theme-base.css", include_str!("../static/theme-base.css")),
    ("theme-dark.css", include_str!("../static/theme-dark.css")),
    ("theme-light.css", include_str!("../static/theme-light.css")),
    ("theme-paper.css", include_str!("../static/theme-paper.css")),
    (
        "theme-projector.css",
        include_str!("../static/theme-projector.css"),
    ),
];

/// The name and contents of the bundled file named `name`, if there is one.
fn bundled(name: &str) -> Option<(&'static str, &'static str)> {
    STATIC
        .iter()
        .find(|&&(bundled, _)| bundled == name)
        .copied()
}

/// The bundled files a page in `theme` links: the slides' stylesheet and
/// script, and the theme's stylesheet with those it imports, if it is one of
/// the bundled themes.
pub fn bundled_files(theme: Option<&str>) -> Vec<&'static str> {
    let mut files = vec!["slides.css", "slides.js"];
    if let Some((name, _)) = theme.and_then(|theme| bundled(&theme_href(theme))) {
        with_imports(name, &mut files);
    }
    files
}

/// Adds a bundled stylesheet to `files`, along with the ones it imports.
fn with_imports(name: &'static str, files: &mut Vec<&'static str>) {
    if files.contains(&name) {
        return;
    }
    files.push(name);
    let (_, css) = bundled(name).unwrap();
    for import in css.lines().filter_map(import_href) {
        match bundled(import) {
            Some((import, _)) => with_imports(import, files),
            None => eprintln!("{name} imports {import}, which is not bundled"),
        }
    }
}

/// Writes the bundled `files` into the outputs directory `root`, except
/// those that are there already as they are, so that a render that changes
/// nothing does not reload the slides.
pub fn write_static(root: &Path, files: &[&str]) {
    if let Err(error) = store::create(root) {
        eprintln!("{}: {error}", root.display());
        return;
    }
    for (name, contents) in files.iter().filter_map(|name| bundled(name)) {
        let path = root.join(name);
        if fs::read(&path).is_ok_and(|old| old == contents.as_bytes()) {
            continue;
        }
        if let Err(error) = fs::write(&path, contents) {
            eprintln!("{}: {error}", path.display());
        }
    }
}

/// The page for the slides in `body`, as the input's frontmatter asks for,
/// to be written at `output`. Its links are written as from `dir`, the
/// deck's own directory, which links the outputs directory as `outputs`, and
/// are moved to where the page is. A self-contained page holds every file it
/// would link, so that it can be opened on its own.
pub fn page(
    body: &str,
    settings: &PageSettings,
    dir: &Path,
    outputs: &str,
    output: &Path,
    self_contained: bool,
) -> String {
    let theme = settings.theme.as_deref();
    let body = indent(&step::number(&markdown::without_empty_slides(body)));
    // The deck's own theme is linked where it is, relative to the deck.
    let own = theme.map(theme_href).filter(|href| bundled(href).is_none());
    if let Some(href) = &own
        && !dir.join(href).is_file()
    {
        eprintln!("{}: no theme {href} in it", dir.display());
    }
    // The theme's stylesheet goes after the slides', which it overrides.
    let bundled_css = bundled_files(theme)
        .into_iter()
        .filter(|name| name.ends_with(".css") && !is_imported(name, theme));
    let (styles, script, body) = if self_contained {
        let mut seen = HashSet::new();
        let mut styles: Vec<String> = bundled_css
            .map(|name| inline_css(bundled(name).unwrap().1, None, &mut seen))
            .collect();
        styles.extend(own.map(|href| inline_file(&dir.join(href), &mut seen)));
        let styles = styles
            .iter()
            .map(|css| format!("<style>\n{}</style>", css.replace("</style", "<\\/style")))
            .collect();
        let (_, script) = bundled("slides.js").unwrap();
        let script = format!(
            "<script>\n{}</script>",
            script.replace("</script", "<\\/script")
        );
        (styles, script, embed(&body, dir))
    } else {
        let page_dir = parent(output);
        let link = |href: &str| {
            let href = escape(href);
            let href = rebase(&href, dir, page_dir).unwrap_or(href);
            format!("<link rel=\"stylesheet\" href=\"{href}\">")
        };
        let mut styles: Vec<String> = bundled_css
            .map(|name| link(&store::in_outputs(outputs, name)))
            .collect();
        styles.extend(own.as_deref().map(link));
        let script = store::in_outputs(outputs, "slides.js");
        let script = rebase(&script, dir, page_dir).unwrap_or(script);
        let script = format!("<script src=\"{script}\"></script>");
        let body = links(&body, &[" src=\"", " href=\""], |src| {
            rebase(src, dir, page_dir)
        });
        (styles, script, body)
    };
    fill(
        TEMPLATE,
        &[
            ("root", &root_attributes(settings)),
            ("styles", &styles.join("\n    ")),
            ("script", &script),
            ("body", &body),
        ],
    )
}

/// Whether the bundled stylesheet `name` comes in through the theme's
/// imports, rather than being linked by the page itself.
fn is_imported(name: &str, theme: Option<&str>) -> bool {
    let Some((theme, _)) = theme.and_then(|theme| bundled(&theme_href(theme))) else {
        return false;
    };
    name != theme && name != "slides.css"
}

/// The attributes of the page's root, which slides.css reads, so that the
/// frontmatter comes before the theme: the shape of the slides, and whether
/// figures are shown as drawn or step without fading, and, for steps.js, how
/// long an animation takes to rush where it is going.
fn root_attributes(settings: &PageSettings) -> String {
    let mut attributes = String::new();
    if let Some(ratio) = settings.aspect_ratio {
        attributes.push_str(&format!(" style=\"{}\"", escape(&ratio.css())));
    }
    let figures = &settings.figures;
    if figures.invert == Some(false) {
        attributes.push_str(" data-invert-figures=\"false\"");
    }
    if figures.fade == Some(false) {
        attributes.push_str(" data-fade-figures=\"false\"");
    }
    if let Some(rush) = figures.rush {
        attributes.push_str(&format!(" data-rush=\"{rush}\""));
    }
    attributes
}

/// The template with each `{name}` in place of its value, all at once, so
/// that a value holding a placeholder is not filled in too.
fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut filled = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        filled.push_str(&rest[..start]);
        rest = &rest[start..];
        let placeholder = values.iter().find(|(name, _)| {
            rest[1..].starts_with(name) && rest[1 + name.len()..].starts_with('}')
        });
        match placeholder {
            Some((name, value)) => {
                filled.push_str(value);
                rest = &rest[name.len() + 2..];
            }
            None => {
                filled.push('{');
                rest = &rest[1..];
            }
        }
    }
    filled.push_str(rest);
    filled
}

/// A stylesheet with the ones it imports in place of its `@import` rules,
/// so that it needs no other file, each only the first time. A bundled
/// stylesheet imports bundled ones, and a file those next to it, in `base`.
fn inline_css(css: &str, base: Option<&Path>, seen: &mut HashSet<PathBuf>) -> String {
    let mut inlined = String::new();
    for line in css.lines() {
        match (import_href(line), base) {
            (Some(import), Some(base)) => inlined.push_str(&inline_file(&base.join(import), seen)),
            (Some(import), None) => match bundled(import) {
                Some((name, css)) if seen.insert(PathBuf::from(name)) => {
                    inlined.push_str(&inline_css(css, None, seen));
                }
                Some(_) => {}
                None => eprintln!("{import} is not bundled"),
            },
            (None, _) => {
                inlined.push_str(line);
                inlined.push('\n');
            }
        }
    }
    inlined
}

/// The stylesheet at `path`, inlined as `inline_css` does.
fn inline_file(path: &Path, seen: &mut HashSet<PathBuf>) -> String {
    if !seen.insert(path.to_path_buf()) {
        return String::new();
    }
    match fs::read_to_string(path) {
        Ok(css) => inline_css(&css, Some(parent(path)), seen),
        Err(error) => {
            eprintln!("{}: {error}", path.display());
            String::new()
        }
    }
}

/// The stylesheet an `@import "..."` or `@import url("...")` rule imports.
fn import_href(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix("@import")?.trim_start();
    let rest = rest.strip_prefix("url(").unwrap_or(rest);
    let quote = rest.chars().next().filter(|&c| c == '"' || c == '\'')?;
    let (href, _) = rest[1..].split_once(quote)?;
    Some(href)
}

/// The slides with every file they link by a relative `src`, as a raster
/// output or an image of the markdown is, in a data URL, resolved as the
/// browser would, from the deck's directory `dir`.
fn embed(html: &str, dir: &Path) -> String {
    links(html, &[" src=\""], |src| data_url(dir, src))
}

/// The html with the value of each of the `attributes`, such as ` src="`,
/// in place of what `replace` makes of it, where it makes something.
fn links(html: &str, attributes: &[&str], replace: impl Fn(&str) -> Option<String>) -> String {
    let mut replaced = String::with_capacity(html.len());
    let mut rest = html;
    while let Some((start, attribute)) = attributes
        .iter()
        .filter_map(|attribute| Some((rest.find(attribute)?, attribute)))
        .min()
    {
        let (before, after) = rest.split_at(start + attribute.len());
        replaced.push_str(before);
        let Some(end) = after.find('"') else {
            rest = after;
            break;
        };
        let value = &after[..end];
        match replace(value) {
            Some(value) => replaced.push_str(&value),
            None => replaced.push_str(value),
        }
        rest = &after[end..];
    }
    replaced.push_str(rest);
    replaced
}

/// Whether `src` is a relative path, rather than a URL, an absolute path or
/// a place in the page.
fn is_relative(src: &str) -> bool {
    !src.is_empty()
        && !src.starts_with(['/', '#', '?'])
        && src
            .split(['/', '?', '#'])
            .next()
            .is_some_and(|first| !first.contains(':'))
}

/// A relative `src` written as from the directory `dir`, as from `page_dir`
/// instead, unless it is not relative or the two are the same.
fn rebase(src: &str, dir: &Path, page_dir: &Path) -> Option<String> {
    if dir == page_dir || !is_relative(src) {
        return None;
    }
    let end = src.find(['?', '#']).unwrap_or(src.len());
    let (path, rest) = src.split_at(end);
    let href = href(page_dir, &dir.join(path));
    Some(if href.is_empty() {
        format!(".{rest}")
    } else {
        href + rest
    })
}

/// The media types of the files a page can embed, by extension.
const MEDIA: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("svg", "image/svg+xml"),
    ("webp", "image/webp"),
    ("avif", "image/avif"),
    ("mp4", "video/mp4"),
    ("webm", "video/webm"),
    ("mp3", "audio/mpeg"),
    ("wav", "audio/wav"),
    ("ogg", "audio/ogg"),
];

/// The file at `src`, relative to `dir`, as a data URL, unless `src` is not
/// a relative path or its file cannot be embedded, which is reported.
fn data_url(dir: &Path, src: &str) -> Option<String> {
    if !is_relative(src) {
        return None;
    }
    let path = unescape(src.split(['?', '#']).next()?);
    let path = dir.join(path);
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    let Some(&(_, mime)) = MEDIA.iter().find(|&&(known, _)| known == extension) else {
        eprintln!(
            "{}: cannot embed a .{extension} file, so it stays linked",
            path.display()
        );
        return None;
    };
    match fs::read(&path) {
        Ok(bytes) => Some(format!("data:{mime};base64,{}", BASE64.encode(bytes))),
        Err(error) => {
            eprintln!("{}: {error}", path.display());
            None
        }
    }
}

/// A path as an attribute holds it: with its ampersands escaped, and
/// whatever else a URL cannot hold percent-encoded.
fn unescape(src: &str) -> String {
    let src = src.replace("&amp;", "&");
    let mut bytes = Vec::with_capacity(src.len());
    let mut rest = src.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        let hex = after
            .get(..2)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (byte, hex) {
            (b'%', Some(decoded)) => {
                bytes.push(decoded);
                rest = &after[2..];
            }
            _ => {
                bytes.push(byte);
                rest = after;
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
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

/// Indents the html to sit inside the template. Indentation is cosmetic
/// everywhere but inside `<pre>`, where it is content.
fn indent(html: &str) -> String {
    let mut indented = String::new();
    let mut preformatted = false;
    for line in html.lines() {
        if !preformatted {
            // A slide's opening tag says how many steps it has too.
            let slide = line == "</section>" || line.starts_with("<section") && line.ends_with('>');
            indented.push_str(if slide { "    " } else { "        " });
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
    use slides::aspect::AspectRatio;

    #[test]
    fn a_theme_names_a_bundled_stylesheet_or_its_own() {
        assert_eq!(theme_href("dark"), "theme-dark.css");
        assert_eq!(theme_href("custom/talk.css"), "custom/talk.css");
    }

    fn settings(theme: Option<&str>, aspect_ratio: Option<AspectRatio>) -> PageSettings {
        PageSettings {
            theme: theme.map(str::to_string),
            aspect_ratio,
            ..PageSettings::default()
        }
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("slides-page-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_theme_brings_the_stylesheets_it_imports() {
        assert_eq!(bundled_files(None), ["slides.css", "slides.js"]);
        assert_eq!(
            bundled_files(Some("dark")),
            [
                "slides.css",
                "slides.js",
                "theme-dark.css",
                "theme-base.css"
            ]
        );
        assert_eq!(bundled_files(Some("talk.css")), ["slides.css", "slides.js"]);
    }

    #[test]
    fn a_page_links_the_files_of_its_theme_in_the_outputs() {
        let dir = temp_dir();
        let output = dir.join("index.html");
        let html = page(
            "<section>\n</section>\n",
            &settings(Some("dark"), None),
            &dir,
            store::DIR,
            &output,
            false,
        );
        let links: Vec<&str> = html
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with("<link") && !line.contains("katex"))
            .collect();
        assert_eq!(
            links,
            [
                "<link rel=\"stylesheet\" href=\"_outputs/slides.css\">",
                "<link rel=\"stylesheet\" href=\"_outputs/theme-dark.css\">",
            ]
        );
        assert!(html.contains("<script src=\"_outputs/slides.js\"></script>"));

        let root = dir.join(store::DIR);
        write_static(&root, &bundled_files(Some("dark")));
        let mut written: Vec<String> = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        written.sort();
        let expected = [
            store::GITIGNORE,
            "slides.css",
            "slides.js",
            "theme-base.css",
            "theme-dark.css",
        ];
        assert_eq!(written, expected);
        assert_eq!(
            fs::read_to_string(root.join("theme-dark.css")).unwrap(),
            bundled("theme-dark.css").unwrap().1
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_page_in_the_outputs_links_as_from_there() {
        let dir = Path::new("/deck");
        let body = "<section>\n<p><img src=\"_outputs/figure.png\">\n<img src=\"photo.jpg?v=1\">\n<a href=\"#3\">back</a>\n<a href=\"https://example.com\">site</a></p>\n</section>\n";
        let settings = settings(Some("custom/talk.css"), None);
        let output = dir.join(store::DIR).join("index.html");
        let html = page(body, &settings, dir, store::DIR, &output, false);
        assert!(html.contains("<link rel=\"stylesheet\" href=\"slides.css\">"));
        assert!(html.contains("<link rel=\"stylesheet\" href=\"../custom/talk.css\">"));
        assert!(html.contains("<script src=\"slides.js\"></script>"));
        assert!(html.contains("<img src=\"figure.png\">"));
        assert!(html.contains("<img src=\"../photo.jpg?v=1\">"));
        assert!(html.contains("<a href=\"#3\">"));
        assert!(html.contains("<a href=\"https://example.com\">"));
    }

    #[test]
    fn an_own_theme_is_linked_where_it_is() {
        let dir = temp_dir();
        fs::write(
            dir.join("talk.css"),
            "@import url(\"base.css\");\nh1 { color: red; }\n",
        )
        .unwrap();
        fs::write(dir.join("base.css"), "h2 { color: blue; }\n").unwrap();
        let output = dir.join("index.html");
        let page = |self_contained| {
            let settings = settings(Some("talk.css"), None);
            page("", &settings, &dir, store::DIR, &output, self_contained)
        };
        let linked = page(false);
        assert!(linked.contains("<link rel=\"stylesheet\" href=\"talk.css\">"));
        let inlined = page(true);
        assert!(inlined.contains("h2 { color: blue; }\nh1 { color: red; }"));
        assert!(!inlined.contains("theme-base.css") && !inlined.contains("@import"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_self_contained_page_holds_every_file() {
        let dir = temp_dir();
        fs::create_dir_all(dir.join(store::DIR)).unwrap();
        fs::write(dir.join(store::DIR).join("figure.png"), b"png").unwrap();
        fs::write(dir.join("my photo.jpg"), b"jpg").unwrap();
        let body = format!(
            "<section>\n<img src=\"{}/figure.png\">\n<img src=\"my%20photo.jpg\" alt=\"\">\n<img src=\"https://example.com/a.png\">\n</section>\n",
            store::DIR
        );
        let html = page(
            &body,
            &settings(Some("dark"), None),
            &dir,
            store::DIR,
            &dir.join("index.html"),
            true,
        );
        assert!(!html.contains("<link rel=\"stylesheet\" href=\"_outputs"));
        assert!(!html.contains("src=\"_outputs/slides.js\""));
        assert!(!html.contains("@import"), "the theme's import is inlined");
        let base = bundled("theme-base.css").unwrap().1;
        assert!(html.contains(base.lines().nth(3).unwrap()));
        let light = bundled("theme-light.css").unwrap().1;
        assert!(
            !html.contains(light.lines().next().unwrap()),
            "only its own theme"
        );
        let png = BASE64.encode(b"png");
        assert!(html.contains(&format!("src=\"data:image/png;base64,{png}\"")));
        let jpg = BASE64.encode(b"jpg");
        assert!(html.contains(&format!("src=\"data:image/jpeg;base64,{jpg}\"")));
        assert!(html.contains("src=\"https://example.com/a.png\""));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_shape_of_the_slides_is_set_on_the_page() {
        let output = Path::new("index.html");
        let html = |ratio| {
            page(
                "",
                &settings(None, ratio),
                Path::new("."),
                store::DIR,
                output,
                false,
            )
        };
        assert!(html(None).starts_with("<!DOCTYPE html>\n<html>\n"));
        let four_three = AspectRatio::Fixed {
            width: 4.0,
            height: 3.0,
        };
        assert!(html(Some(four_three)).contains("<html style=\"--aspect-ratio: 4 / 3\">"));
        let fill = "<html style=\"--slide-width: 100vw; --slide-height: 100vh\">";
        assert!(html(Some(AspectRatio::Fill)).contains(fill));
    }

    #[test]
    fn figures_can_be_shown_as_drawn_without_fading_and_rush_as_long_as_said() {
        use crate::markdown::FigureSettings;
        let output = Path::new("index.html");
        let html = |invert, fade, rush| {
            let settings = PageSettings {
                figures: FigureSettings { invert, fade, rush },
                ..PageSettings::default()
            };
            page("", &settings, Path::new("."), store::DIR, output, false)
        };
        assert!(html(None, Some(true), None).starts_with("<!DOCTYPE html>\n<html>\n"));
        let all =
            "<html data-invert-figures=\"false\" data-fade-figures=\"false\" data-rush=\"0.5\">";
        assert!(html(Some(false), Some(false), Some(0.5)).contains(all));
    }

    #[test]
    fn a_value_is_not_filled_in_again() {
        let filled = fill("{a} { {b}", &[("a", "{b}"), ("b", "2")]);
        assert_eq!(filled, "{b} { 2");
    }
}
