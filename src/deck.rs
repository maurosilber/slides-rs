//! The deck: every file it is made of, each rendered on its own and cached,
//! and the page they make together.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::markdown::{self, File, NO_STEPS, Part, SECTION};
use crate::notebook::{Notebook, Runner};
use crate::paths::read;
use crate::{page, store};

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
    /// frontmatter or, without it, as `steps` says, are marked so.
    fn body(&self, path: &Path, steps: bool, body: &mut String) {
        let file = &self.files[path];
        let steps = file.steps.unwrap_or(steps);
        for part in &file.parts {
            match part {
                Part::Html(html) if steps => body.push_str(html),
                Part::Html(html) => body.push_str(&html.replace(SECTION, NO_STEPS)),
                Part::Cell(hash) => body.push_str(&page::cell_html(&self.outputs, hash)),
                Part::Import(import) => self.body(import, steps, body),
            }
        }
    }

    /// The theme the page of `input` is in, as its frontmatter says.
    fn theme(&self, input: &Path) -> Option<&str> {
        self.files[input].theme.as_deref()
    }

    /// Writes the deck, and the files it links next to it, unless the html on
    /// disk is already the same, so that a save that changes nothing does not
    /// reload the slides. Returns whether it wrote.
    pub fn write(&self, input: &Path, output: &Path) -> bool {
        let theme = self.theme(input);
        if !self.self_contained {
            page::write_static(&self.outputs, &page::bundled_files(theme));
        }
        let mut body = String::new();
        self.body(input, true, &mut body);
        let aspect_ratio = self.files[input].aspect_ratio;
        let html = page::page(&body, theme, aspect_ratio, output, self.self_contained);
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
        deck.body(Path::new("/deck/index.md"), true, &mut body);
        let opening = |title: &str| {
            let heading = body.find(&format!(">{title}</h1>")).unwrap();
            let section = body[..heading].rfind("<section").unwrap();
            body[section..].lines().next().unwrap().to_string()
        };
        assert_eq!(opening("Off"), NO_STEPS);
        assert_eq!(opening("Inherited"), NO_STEPS);
        assert_eq!(opening("On"), SECTION);
    }
}
