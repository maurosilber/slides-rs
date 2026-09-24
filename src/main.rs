use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use clap::Parser as _;
use notify::{RecursiveMode, Watcher};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd, html};

const TEMPLATE: &str = include_str!("template.html");

/// Marks an import in the rendered html, followed by its path and a newline.
const IMPORT: char = '\u{0}';

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
}

fn main() {
    let Cli {
        input,
        output,
        watch: watching,
    } = Cli::parse();
    let output = output.unwrap_or_else(|| input.with_extension("html"));
    // The cache is keyed by canonical path, as watch events report those.
    let input = canonical(&input);

    let mut cache = Cache::default();
    cache.load(&input);
    cache.write(&input, &output);

    if watching {
        watch(&mut cache, &input, &output);
    }
}

/// A file's slides, in the order they are rendered: its own html, and the
/// files it imports, which bring their own.
enum Part {
    Html(String),
    Import(PathBuf),
}

/// Each file is rendered on its own, so that a change re-renders only that file.
#[derive(Default)]
struct Cache {
    files: HashMap<PathBuf, Vec<Part>>,
}

impl Cache {
    /// Renders a file, along with every file it imports that is not cached yet.
    fn load(&mut self, path: &Path) {
        let parts = render(&read(path), parent(path));
        let imports: Vec<PathBuf> = parts
            .iter()
            .filter_map(|part| match part {
                Part::Import(path) => Some(path.clone()),
                Part::Html(_) => None,
            })
            .collect();

        // Caching before recursing keeps a cycle of imports from looping forever.
        self.files.insert(path.to_path_buf(), parts);
        for import in imports {
            if !self.files.contains_key(&import) {
                self.load(&import);
            }
        }
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
        for part in self.files.get(path).into_iter().flatten() {
            if let Part::Import(import) = part {
                self.imported(import, imported);
            }
        }
    }

    /// Concatenates the cached renders, following the imports from `path`.
    fn body(&self, path: &Path, body: &mut String) {
        for part in &self.files[path] {
            match part {
                Part::Html(html) => body.push_str(html),
                Part::Import(import) => self.body(import, body),
            }
        }
    }

    fn write(&self, input: &Path, output: &Path) {
        let mut body = String::new();
        self.body(input, &mut body);
        // A slide break at either end of a file, or two in a row, leaves an empty slide.
        let body = indent(&body.replace("<section>\n</section>\n", ""));
        fs::write(output, TEMPLATE.replace("{body}", &body)).unwrap();
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
        for path in &changed {
            cache.load(path);
        }
        cache.prune(input);
        watch_dirs(&mut watcher, &mut watched, cache);
        cache.write(input, output);
        eprintln!(
            "rendered {} after {} changed",
            output.display(),
            changed.len()
        );
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

/// Renders the markdown into `<section>`s, dropping the YAML frontmatter and
/// splitting on horizontal rules, with a part boundary at every import.
fn render(markdown: &str, dir: &Path) -> Vec<Part> {
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
        event => Some(event),
    });

    let mut rendered = String::from("<section>\n");
    html::push_html(&mut rendered, events);
    rendered.push_str("</section>\n");

    // Split at the markers, so every file is cached apart from the ones it imports.
    let mut parts = Vec::new();
    let mut rest = rendered.as_str();
    while let Some((html, marked)) = rest.split_once(IMPORT) {
        let (import, after) = marked.split_once('\n').unwrap();
        parts.push(Part::Html(html.to_string()));
        parts.push(Part::Import(PathBuf::from(import)));
        rest = after;
    }
    parts.push(Part::Html(rest.to_string()));
    parts
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

/// Identifies a code block by its content, so its output can be cached.
fn hash(code: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    code.hash(&mut hasher);
    hasher.finish()
}
