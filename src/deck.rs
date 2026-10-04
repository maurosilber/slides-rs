//! The deck: every file it is made of, each rendered on its own and cached,
//! and the page they make together.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::markdown::{self, File, NO_STEPS, Part, SECTION};
use crate::notebook::{Notebook, Runner};
use crate::paths::{Importing, href, parent, read};
use crate::{page, progress, store};

/// Each file is rendered on its own, so that a change re-renders only that file.
pub struct Deck {
    pub files: HashMap<PathBuf, File>,
    /// Where the outputs of the cells are saved.
    outputs: PathBuf,
    runner: Runner,
    /// Whether the page holds every file it links, rather than linking them.
    self_contained: bool,
}

impl Deck {
    pub fn new(outputs: PathBuf, kernels: &'static [&'static str], self_contained: bool) -> Deck {
        Deck {
            files: HashMap::new(),
            outputs,
            runner: Runner::new(kernels),
            self_contained,
        }
    }

    /// Renders the files, along with every file they import that is not cached
    /// yet, and runs the cells of those whose outputs are not saved yet.
    pub fn update(&mut self, paths: &[PathBuf]) {
        let mut loaded = Vec::new();
        for path in paths {
            self.load(path, &mut loaded);
        }
        let notebooks: Vec<Notebook> = loaded
            .iter()
            .map(|path| {
                let file = &self.files[path];
                Notebook {
                    path,
                    lock: file.lock.as_deref(),
                    cells: &file.cells,
                    hashes: &file.hashes,
                }
            })
            .collect();
        self.runner.run(&self.outputs, &notebooks);
        for path in &loaded {
            self.warn(path);
        }
    }

    /// Reports what is wrong with how the slides of a file step, on its
    /// line, with the outputs of its cells, which can name steps too.
    fn warn(&self, path: &Path) {
        let file = &self.files[path];
        let outputs = |_: &[String]| {
            file.hashes
                .iter()
                .map(|hash| store::cell_html(&self.outputs, "", hash))
                .collect()
        };
        for slide in markdown::steps(&file.markdown, outputs) {
            for warning in slide.warnings {
                let line = warning.line + 1;
                eprintln!("{}:{line}: {}", path.display(), warning.message);
            }
        }
    }

    /// Renders a file, along with every file it imports that is not cached yet,
    /// adding each one to `loaded`.
    fn load(&mut self, path: &Path, loaded: &mut Vec<PathBuf>) {
        let file = markdown::render(&read(path), path);
        let imports: Vec<PathBuf> = file.imports().cloned().collect();

        // Caching before recursing keeps a cycle of imports from looping forever.
        self.files.insert(path.to_path_buf(), file);
        loaded.push(path.to_path_buf());
        for import in imports {
            if !self.files.contains_key(&import) {
                self.load(&import, loaded);
            }
        }
    }

    /// Removes the saved outputs that no cell of the deck uses, and the files
    /// the page of `input` does not link.
    pub fn clean(&self, input: &Path) {
        let keep: HashSet<&str> = self
            .files
            .values()
            .flat_map(|file| &file.hashes)
            .map(String::as_str)
            .collect();
        let files: HashSet<&str> = if self.self_contained {
            HashSet::new()
        } else {
            page::bundled_files(self.theme(input)).into_iter().collect()
        };
        match store::remove_stale(&self.outputs, &keep, &files) {
            Ok(removed) => eprintln!(
                "removed {} stale cell(s), {} output(s) and {} other file(s) from {}, recovering {}",
                removed.cells,
                removed.outputs,
                removed.other,
                self.outputs.display(),
                store::human_size(removed.bytes)
            ),
            Err(error) => eprintln!("{}: {error}", self.outputs.display()),
        }
    }

    /// The markdown files whose cells read each file, as listed with their
    /// outputs.
    pub fn readers(&self) -> HashMap<PathBuf, HashSet<PathBuf>> {
        let mut readers: HashMap<PathBuf, HashSet<PathBuf>> = HashMap::new();
        for (path, file) in &self.files {
            for hash in &file.hashes {
                for read in store::read_files(&self.outputs, hash) {
                    readers.entry(read).or_default().insert(path.clone());
                }
            }
        }
        readers
    }

    /// Forgets the files that `input` no longer imports, so that a change
    /// to one of them stops re-rendering the deck.
    pub fn prune(&mut self, input: &Path) {
        let mut imported = HashSet::new();
        self.imported(input, &mut imported);
        self.files.retain(|path, _| imported.contains(path));
    }

    /// The files the deck is made of, reached by following the imports.
    fn imported(&self, path: &Path, imported: &mut HashSet<PathBuf>) {
        if !imported.insert(path.to_path_buf()) {
            return;
        }
        for import in self.files.get(path).into_iter().flat_map(File::imports) {
            self.imported(import, imported);
        }
    }

    /// Concatenates the cached renders, following the imports from `path`.
    /// The slides of a file whose steps are off, as set in its own
    /// frontmatter or, without it, as `steps` says, are marked so. Outputs
    /// are linked through `outputs`, the outputs directory as the deck links it.
    fn body(
        &self,
        path: &Path,
        steps: bool,
        outputs: &str,
        importing: &mut Importing,
        body: &mut String,
    ) {
        if !importing.enter(path) {
            return;
        }
        let file = &self.files[path];
        let steps = file.steps.unwrap_or(steps);
        for part in &file.parts {
            match part {
                Part::Html(html) if steps => body.push_str(html),
                Part::Html(html) => body.push_str(&html.replace(SECTION, NO_STEPS)),
                Part::Cell(hash) => body.push_str(&store::cell_html(&self.outputs, outputs, hash)),
                Part::Import(import) => self.body(import, steps, outputs, importing, body),
            }
        }
        importing.leave();
    }

    /// Whether the slides of `target` step, as `path`, stepping as `steps`
    /// says, imports it, directly or through other files; nothing if it does
    /// not import it.
    fn steps_in(&self, path: &Path, target: &Path, steps: bool) -> Option<bool> {
        self.steps_through(path, target, steps, &mut HashSet::new())
    }

    fn steps_through(
        &self,
        path: &Path,
        target: &Path,
        steps: bool,
        visited: &mut HashSet<PathBuf>,
    ) -> Option<bool> {
        if path == target {
            return Some(steps);
        }
        if !visited.insert(path.to_path_buf()) {
            return None;
        }
        let file = self.files.get(path)?;
        let steps = file.steps.unwrap_or(steps);
        file.imports()
            .find_map(|import| self.steps_through(import, target, steps, visited))
    }

    /// Whether `deck` imports `slides`, directly or through other files, or is it.
    pub fn imports(&self, deck: &Path, slides: &Path) -> bool {
        self.steps_in(deck, slides, true).is_some()
    }

    /// The theme the page of `input` is in, as its frontmatter says.
    fn theme(&self, input: &Path) -> Option<&str> {
        self.files[input].page.theme.as_deref()
    }

    /// Writes the slides of `slides` as they are in `deck`, which is them or
    /// imports them: on a page as its frontmatter asks for, with links as from
    /// its directory, and stepping as it does. With the files it links in the
    /// outputs directory, unless the html on disk is already the same, so
    /// that a save that changes nothing does not reload the slides. Returns
    /// whether it wrote. The `output` is canonical, as `deck` and `slides` are.
    pub fn write(&self, deck: &Path, slides: &Path, output: &Path) -> bool {
        let Some(steps) = self.steps_in(deck, slides, true) else {
            eprintln!(
                "{} does not import {}",
                progress::relative(deck).display(),
                progress::relative(slides).display()
            );
            return false;
        };
        let theme = self.theme(deck);
        // The page may be in the outputs directory, which keeps it out of git too.
        if let Err(error) = store::create(&self.outputs) {
            eprintln!("{}: {error}", self.outputs.display());
        }
        if !self.self_contained {
            page::write_static(&self.outputs, &page::bundled_files(theme));
        }
        let dir = parent(deck);
        let outputs = href(dir, &self.outputs);
        let mut body = String::new();
        self.body(slides, steps, &outputs, &mut Importing::default(), &mut body);
        let settings = &self.files[deck].page;
        let html = page::page(&body, settings, dir, &outputs, output, self.self_contained);
        if fs::read(output).is_ok_and(|old| old == html.as_bytes()) {
            return false;
        }
        fs::write(output, html).unwrap();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notebook::KERNELS;

    #[test]
    fn an_imported_file_steps_through_its_slides_as_its_importer_unless_it_says() {
        let mut deck = Deck::new(PathBuf::from("/deck/_outputs"), KERNELS, false);
        let files = [
            (
                "/deck/index.md",
                "---\nsteps: false\n---\n# Off\n\n<import-slide src=\"same.md\" />\n<import-slide src=\"on.md\" />\n",
            ),
            ("/deck/same.md", "# Inherited\n"),
            ("/deck/on.md", "---\nsteps: true\n---\n# On\n"),
        ];
        for (path, markdown) in files {
            deck.files.insert(
                PathBuf::from(path),
                markdown::render(markdown, Path::new(path)),
            );
        }
        let mut body = String::new();
        let mut importing = Importing::default();
        deck.body(Path::new("/deck/index.md"), true, store::DIR, &mut importing, &mut body);
        let opening = |title: &str| {
            let heading = body.find(&format!(">{title}</h1>")).unwrap();
            let section = body[..heading].rfind("<section").unwrap();
            body[section..].lines().next().unwrap().to_string()
        };
        assert_eq!(opening("Off"), NO_STEPS);
        assert_eq!(opening("Inherited"), NO_STEPS);
        assert_eq!(opening("On"), SECTION);
    }

    #[test]
    fn a_file_imported_again_within_itself_brings_nothing() {
        let mut deck = Deck::new(PathBuf::from("/deck/_outputs"), KERNELS, false);
        let files = [
            ("/deck/a.md", "# A\n\n<import-slide src=\"b.md\" />\n"),
            ("/deck/b.md", "# B\n\n<import-slide src=\"a.md\" />\n"),
        ];
        for (path, markdown) in files {
            deck.files.insert(
                PathBuf::from(path),
                markdown::render(markdown, Path::new(path)),
            );
        }
        let mut body = String::new();
        let mut importing = Importing::default();
        deck.body(Path::new("/deck/a.md"), true, store::DIR, &mut importing, &mut body);
        assert_eq!(body.matches("<h1>").count(), 2, "{body}");
    }

    #[test]
    fn an_imported_file_steps_as_the_deck_it_is_shown_in_has_it() {
        let mut deck = Deck::new(PathBuf::from("/deck/_outputs"), KERNELS, false);
        let files = [
            (
                "/deck/index.md",
                "---\nsteps: false\n---\n<import-slide src=\"part.md\" />\n",
            ),
            ("/deck/part.md", "<import-slide src=\"same.md\" />\n"),
            ("/deck/same.md", "# Inherited\n"),
            ("/deck/other.md", "# Other\n"),
        ];
        for (path, markdown) in files {
            deck.files.insert(
                PathBuf::from(path),
                markdown::render(markdown, Path::new(path)),
            );
        }
        let steps = |target: &str| {
            deck.steps_in(Path::new("/deck/index.md"), Path::new(target), true)
        };
        assert_eq!(steps("/deck/index.md"), Some(true));
        assert_eq!(steps("/deck/same.md"), Some(false));
        assert_eq!(steps("/deck/other.md"), None);
    }
}
