mod execute;
mod kernel;
mod math;
mod output;
mod svg;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use clap::{Parser as _, ValueEnum};
use notify::{RecursiveMode, Watcher};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd, html};

const TEMPLATE: &str = include_str!("template.html");

/// Where the outputs of every cell are stored, next to the rendered html.
const OUTPUT_DIR: &str = "_outputs";

/// Marks an import in the rendered html, followed by its path and a newline.
const IMPORT: char = '\u{0}';

/// Marks a code cell in the rendered html, followed by a newline. The cells
/// are marked in order, so the n-th marker stands for the n-th cell.
const CELL: char = '\u{1}';

/// Renders a markdown file into an HTML slide deck.
#[derive(clap::Parser)]
struct Cli {
    /// The markdown file to render.
    input: PathBuf,
    /// Where to write the HTML. Defaults to the input with an `.html` extension.
    output: Option<PathBuf>,
    /// Re-render on every change to the input or to a file it imports.
    #[arg(short, long)]
    watch: bool,
    /// Kernel to run the cells on. Without this, the first of xpython and
    /// python3 that is installed is used.
    #[arg(short, long, value_enum)]
    kernel: Option<KernelChoice>,
}

/// Which kernel to run the cells on.
#[derive(Clone, Copy, ValueEnum)]
enum KernelChoice {
    /// xeus-python's kernel.
    #[value(name = "xpython")]
    Xpython,
    /// ipykernel's kernel.
    #[value(name = "python3")]
    Python3,
}

impl KernelChoice {
    /// The names this kernel is installed under.
    fn kernelspecs(self) -> &'static [&'static str] {
        match self {
            KernelChoice::Xpython => &["xpython"],
            KernelChoice::Python3 => &["python3"],
        }
    }
}

fn main() {
    let Cli {
        input,
        output,
        watch: watching,
        kernel,
    } = Cli::parse();
    let output = output.unwrap_or_else(|| input.with_extension("html"));
    // The cache is keyed by canonical path, as watch events report those.
    let input = canonical(&input);
    // Naming a kernel narrows the search to that one, so asking for a kernel
    // that is not installed is an error rather than a quiet fallback.
    let kernels = kernel.map_or(execute::KERNELS, KernelChoice::kernelspecs);

    let mut cache = Cache::new(parent(&output).join(OUTPUT_DIR), kernels);
    cache.update(std::slice::from_ref(&input));
    cache.write(&input, &output);

    if watching {
        watch(&mut cache, &input, &output);
    }
}

/// A file's slides, in the order they are rendered: its own html, the
/// outputs of its code cells, and the files it imports, which bring their own.
enum Part {
    Html(String),
    /// A code cell, by the hash its outputs are saved under.
    Cell(String),
    Import(PathBuf),
}

/// A rendered file. Its code cells make up a notebook, run on a kernel of its own.
struct File {
    parts: Vec<Part>,
    cells: Vec<String>,
    /// The theme its frontmatter asks for. Only the input's is used, so an
    /// imported file can keep the theme it was written with.
    theme: Option<String>,
}

/// Each file is rendered on its own, so that a change re-renders only that file.
struct Cache {
    files: HashMap<PathBuf, File>,
    /// Where the outputs of the cells are saved.
    outputs: PathBuf,
    /// The kernels the cells can run on, most preferred first.
    kernels: &'static [&'static str],
    runtime: tokio::runtime::Runtime,
}

impl Cache {
    fn new(outputs: PathBuf, kernels: &'static [&'static str]) -> Cache {
        Cache {
            files: HashMap::new(),
            outputs,
            kernels,
            runtime: tokio::runtime::Runtime::new().unwrap(),
        }
    }

    /// Renders the files, along with every file they import that is not cached
    /// yet, and runs the cells of those whose outputs are not saved yet.
    fn update(&mut self, paths: &[PathBuf]) {
        let mut loaded = Vec::new();
        for path in paths {
            self.load(path, &mut loaded);
        }
        self.execute(&loaded);
    }

    /// Renders a file, along with every file it imports that is not cached yet,
    /// adding each one to `loaded`.
    fn load(&mut self, path: &Path, loaded: &mut Vec<PathBuf>) {
        let file = render(&read(path), path);
        let imports: Vec<PathBuf> = file
            .parts
            .iter()
            .filter_map(|part| match part {
                Part::Import(path) => Some(path.clone()),
                Part::Html(_) | Part::Cell(_) => None,
            })
            .collect();

        // Caching before recursing keeps a cycle of imports from looping forever.
        self.files.insert(path.to_path_buf(), file);
        loaded.push(path.to_path_buf());
        for import in imports {
            if !self.files.contains_key(&import) {
                self.load(&import, loaded);
            }
        }
    }

    /// Runs the notebook of every file, all at once, as each one has a kernel
    /// of its own. A notebook that fails leaves its missing outputs out of
    /// the deck, rather than the rest of the deck too.
    fn execute(&self, paths: &[PathBuf]) {
        let notebooks: Vec<(&PathBuf, Vec<String>)> = paths
            .iter()
            .map(|path| (path, self.files[path].cells.clone()))
            .filter(|(_, cells)| !cells.is_empty())
            .collect();
        if notebooks.is_empty() {
            return;
        }

        fs::create_dir_all(&self.outputs).unwrap();
        fs::write(self.outputs.join(".gitignore"), "*").unwrap();
        self.runtime.block_on(async {
            let tasks: Vec<_> = notebooks
                .into_iter()
                .map(|(path, cells)| {
                    let root = self.outputs.clone();
                    let task = execute::execute_cells(self.kernels, root, cells);
                    (path, tokio::spawn(task))
                })
                .collect();
            // Let every notebook finish, so a failure in one does not cut
            // another short while it is still writing.
            for (path, task) in tasks {
                match task.await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => eprintln!("{}: {error:#}", path.display()),
                    Err(error) => eprintln!("{}: {error}", path.display()),
                }
            }
        });
    }

    /// Forgets the files that `input` no longer imports, so that a change
    /// to one of them stops re-rendering the deck.
    fn prune(&mut self, input: &Path) {
        let mut imported = HashSet::new();
        self.imported(input, &mut imported);
        self.files.retain(|path, _| imported.contains(path));
    }

    /// The files the deck is made of, reached by following the imports.
    fn imported(&self, path: &Path, imported: &mut HashSet<PathBuf>) {
        if !imported.insert(path.to_path_buf()) {
            return;
        }
        for part in self.files.get(path).into_iter().flat_map(|file| &file.parts) {
            if let Part::Import(import) = part {
                self.imported(import, imported);
            }
        }
    }

    /// Concatenates the cached renders, following the imports from `path`.
    fn body(&self, path: &Path, body: &mut String) {
        for part in &self.files[path].parts {
            match part {
                Part::Html(html) => body.push_str(html),
                Part::Cell(hash) => body.push_str(&cell_html(&self.outputs, hash)),
                Part::Import(import) => self.body(import, body),
            }
        }
    }

    /// Writes the deck, unless the html on disk is already the same, so that
    /// a save that changes nothing does not reload the slides. Returns whether
    /// it wrote.
    fn write(&self, input: &Path, output: &Path) -> bool {
        let mut body = String::new();
        self.body(input, &mut body);
        // A slide break at either end of a file, or two in a row, leaves an empty slide.
        let body = indent(&body.replace("<section>\n</section>\n", ""));
        let theme = match &self.files[input].theme {
            Some(theme) => {
                let href = theme_href(theme);
                if !parent(output).join(&href).is_file() {
                    eprintln!("{}: no theme {href} next to it", output.display());
                }
                format!("    <link rel=\"stylesheet\" href=\"{}\">\n", escape(&href))
            }
            None => String::new(),
        };
        let html = TEMPLATE
            .replace("    {theme}\n", &theme)
            .replace("{body}", &body);
        if fs::read(output).is_ok_and(|old| old == html.as_bytes()) {
            return false;
        }
        fs::write(output, html).unwrap();
        true
    }
}

/// Re-renders the deck whenever one of its files changes, until interrupted.
fn watch(cache: &mut Cache, input: &Path, output: &Path) {
    let (sender, receiver) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(sender).unwrap();
    let mut watched = HashSet::new();
    watch_dirs(&mut watcher, &mut watched, cache);
    eprintln!("watching {} files", cache.files.len());

    while let Ok(event) = receiver.recv() {
        let mut paths: HashSet<PathBuf> = HashSet::new();
        let mut collect = |event: notify::Result<notify::Event>| {
            if let Ok(event) = event {
                paths.extend(event.paths);
            }
        };
        collect(event);
        // A save arrives as a burst of events, and several files may be saved at once.
        while let Ok(event) = receiver.recv_timeout(Duration::from_millis(50)) {
            collect(event);
        }

        let changed: Vec<PathBuf> = paths
            .iter()
            .map(|path| canonical(path))
            .filter(|path| cache.files.contains_key(path))
            .collect();
        if changed.is_empty() {
            continue;
        }

        // Only the changed files are rendered again; an import they still
        // share with the rest of the deck keeps its cached render.
        cache.update(&changed);
        cache.prune(input);
        watch_dirs(&mut watcher, &mut watched, cache);
        if cache.write(input, output) {
            eprintln!(
                "rendered {} after {} changed",
                output.display(),
                changed.len()
            );
        } else {
            eprintln!("{} is unchanged", output.display());
        }
    }
}

/// Watches the directory of every file in the deck, and no others: an editor
/// saves by replacing a file, which a watch on the file itself would not survive.
fn watch_dirs(watcher: &mut impl Watcher, watched: &mut HashSet<PathBuf>, cache: &Cache) {
    let dirs: HashSet<PathBuf> = cache
        .files
        .keys()
        .map(|path| parent(path).to_path_buf())
        .collect();
    for dir in dirs.difference(watched) {
        watcher.watch(dir, RecursiveMode::NonRecursive).unwrap();
    }
    for dir in watched.difference(&dirs) {
        // The directory itself may be gone, which already ends the watch.
        let _ = watcher.unwatch(dir);
    }
    *watched = dirs;
}

/// The canonical path of a file that need not exist yet, so that the same file
/// is one cache entry however it is reached.
fn canonical(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    match (parent(path).canonicalize(), path.file_name()) {
        (Ok(dir), Some(name)) => dir.join(name),
        _ => path.to_path_buf(),
    }
}

/// The directory holding a file, which for a bare file name is the current one.
fn parent(path: &Path) -> &Path {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    }
}

/// A file that cannot be read renders as nothing, so that watch mode
/// survives until it is created.
fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| {
        eprintln!("{}: {error}", path.display());
        String::new()
    })
}

/// Renders the markdown of the file at `path` into `<section>`s, taking the
/// YAML frontmatter out and splitting on horizontal rules, with a part
/// boundary at every import and at every code cell. The code of the cells
/// comes along, in order.
fn render(markdown: &str, path: &Path) -> File {
    let dir = parent(path);
    let mut metadata = false;
    let mut frontmatter = String::new();
    let mut code: Option<String> = None;
    let mut steps = math::Steps::default();
    let mut cells = Vec::new();
    // The events are consumed by the time the cells are needed again.
    let cell_codes = &mut cells;
    let yaml = &mut frontmatter;
    let events = Parser::new_ext(
        markdown,
        Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
            | Options::ENABLE_HEADING_ATTRIBUTES
            | Options::ENABLE_MATH,
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
    let mut hashes = output::hashes(&cells).into_iter();
    let mut parts = Vec::new();
    let mut rest = rendered.as_str();
    while let Some(start) = rest.find([IMPORT, CELL]) {
        let (html, marked) = rest.split_at(start);
        let (marker, after) = marked.split_once('\n').unwrap();
        parts.push(Part::Html(html.to_string()));
        parts.push(match marker.strip_prefix(IMPORT) {
            Some(import) => Part::Import(PathBuf::from(import)),
            None => Part::Cell(hashes.next().unwrap()),
        });
        rest = after;
    }
    parts.push(Part::Html(rest.to_string()));
    File {
        parts,
        cells,
        theme: Frontmatter::parse(&frontmatter, path).theme,
    }
}

/// What a file's frontmatter sets. Keys the deck does not read are ignored,
/// so the frontmatter can hold a title, an author, or notes of its own.
#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct Frontmatter {
    theme: Option<String>,
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
fn cell_html(root: &Path, hash: &str) -> String {
    let Ok(files) = output::saved(root, hash) else {
        // The cell has not run, or its notebook failed; its error was reported then.
        return format!("<!-- no outputs for cell {hash} -->\n");
    };
    let mut html = String::new();
    for file in files {
        let name = file.file_name().unwrap().to_str().unwrap();
        let extension = file.extension().and_then(|extension| extension.to_str());
        if let Some("png" | "jpeg" | "gif") = extension {
            html.push_str(&format!("<img src=\"{OUTPUT_DIR}/{hash}/{name}\">\n"));
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
            Some("html") => html.push_str(&text),
            Some("svg") => match svg::inline(&text) {
                Ok(svg) => html.push_str(&svg),
                Err(error) => eprintln!("{}: {error:#}", file.display()),
            },
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
        assert_eq!(file.theme.as_deref(), Some("dark"));
    }

    #[test]
    fn a_theme_is_read_as_yaml() {
        assert_eq!(theme("theme: \"paper\"").as_deref(), Some("paper"));
        assert_eq!(theme("theme: 'my theme.css'").as_deref(), Some("my theme.css"));
        assert_eq!(theme("theme: light # for daytime").as_deref(), Some("light"));
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
    fn a_theme_names_a_bundled_stylesheet_or_its_own() {
        assert_eq!(theme_href("dark"), "theme-dark.css");
        assert_eq!(theme_href("custom/talk.css"), "custom/talk.css");
    }
}
