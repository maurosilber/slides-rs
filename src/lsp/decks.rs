//! Which markdown files are decks, or parts of one: a file with what only a
//! deck has, as slides-rs frontmatter, an import or a code cell, and a file
//! another one imports. A file in the slides language, as `*.slides.md` is,
//! is one whatever it holds.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use slides::markdown::breaks;
use slides::paths::{joined, parent};

/// What a file must hold to be a deck, which `breaks` then says whether it
/// is: a frontmatter, an import or a tilde fence. It spares parsing every
/// other file.
fn maybe_deck(text: &str) -> bool {
    text.starts_with("---") || text.contains("<import-slide") || text.contains("~~~")
}

/// The directories a workspace's markdown is not looked for in.
fn skipped(name: &str) -> bool {
    name.starts_with('.') || name == "node_modules" || name == "_outputs"
}

/// What a file holds, as far as telling decks apart goes.
#[derive(Default)]
struct Scanned {
    /// Whether it has what only a deck has.
    deck: bool,
    /// The files it imports.
    imports: Vec<PathBuf>,
}

#[derive(Default)]
pub struct Decks {
    /// Each markdown file of the workspace, and those opened from outside it.
    files: HashMap<PathBuf, Scanned>,
    /// The files that are decks, or parts of one.
    decks: BTreeSet<PathBuf>,
}

impl Decks {
    /// Reads every markdown file under the workspace's folders.
    pub fn scan_all(&mut self, roots: &[PathBuf]) {
        let mut dirs = roots.to_vec();
        while let Some(dir) = dirs.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name();
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                if kind.is_dir() {
                    if !skipped(&name.to_string_lossy()) {
                        dirs.push(path);
                    }
                } else if path.extension().is_some_and(|extension| extension == "md") {
                    self.scan(&path);
                }
            }
        }
    }

    /// Reads the file at `path` again, as it is on the disk.
    pub fn scan(&mut self, path: &Path) {
        let Ok(text) = fs::read_to_string(path) else {
            self.files.remove(path);
            return;
        };
        let scanned = if maybe_deck(&text) {
            let breaks = breaks(&text);
            Scanned {
                deck: breaks.deck,
                imports: breaks
                    .imports
                    .iter()
                    .map(|import| joined(parent(path), &import.src))
                    .collect(),
            }
        } else {
            Scanned::default()
        };
        self.files.insert(path.to_path_buf(), scanned);
    }

    pub fn remove(&mut self, path: &Path) {
        self.files.remove(path);
    }

    /// Finds again which files are decks, and whether they changed.
    pub fn update(&mut self) -> bool {
        let mut decks = BTreeSet::new();
        for (path, file) in &self.files {
            if file.deck {
                decks.insert(path.clone());
            }
            decks.extend(file.imports.iter().cloned());
        }
        let changed = decks != self.decks;
        self.decks = decks;
        changed
    }

    /// The files that are decks, or parts of one.
    pub fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.decks.iter()
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.decks.contains(path)
    }

    /// The files that import the one at `path`, in order.
    fn importers(&self, path: &Path) -> Vec<&PathBuf> {
        let mut importers: Vec<&PathBuf> = self
            .files
            .iter()
            .filter(|(_, file)| file.imports.iter().any(|import| import == path))
            .map(|(importer, _)| importer)
            .collect();
        importers.sort();
        importers
    }

    /// The deck the file at `path` is part of: the file that imports it, or
    /// that imports one that does, which nothing imports itself. Nothing, if
    /// no file imports it. Of several, the first by path.
    pub fn deck_of(&self, path: &Path) -> Option<PathBuf> {
        let mut deck: Option<PathBuf> = None;
        let mut seen = vec![path.to_path_buf()];
        loop {
            let at = deck.as_deref().unwrap_or(path);
            let found: Vec<&PathBuf> = self
                .importers(at)
                .into_iter()
                .filter(|file| !seen.contains(file))
                .collect();
            let Some(first) = found.first() else {
                return deck;
            };
            if found.len() > 1 {
                let names: Vec<String> = found
                    .iter()
                    .map(|file| file.display().to_string())
                    .collect();
                eprintln!(
                    "{}: imported by {}, shown as in the first",
                    at.display(),
                    names.join(", ")
                );
            }
            deck = Some((*first).clone());
            seen.push((*first).clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_imported_by_a_deck_is_part_of_it() {
        let dir = std::env::temp_dir().join(format!("slides-decks-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sections")).unwrap();
        fs::create_dir_all(dir.join("node_modules")).unwrap();
        fs::write(
            dir.join("talk.md"),
            "<import-slide src=\"sections/part.md\" />\n",
        )
        .unwrap();
        fs::write(dir.join("sections").join("part.md"), "# Part\n").unwrap();
        fs::write(dir.join("README.md"), "# Readme\n\n---\n\n# More\n").unwrap();
        fs::write(
            dir.join("node_modules").join("deck.md"),
            "~~~python\n1\n~~~\n",
        )
        .unwrap();

        let mut decks = Decks::default();
        decks.scan_all(std::slice::from_ref(&dir));
        assert!(decks.update());
        let part = dir.join("sections").join("part.md");
        assert_eq!(
            decks.paths().collect::<Vec<_>>(),
            [&dir.join("sections").join("part.md"), &dir.join("talk.md")]
        );
        assert_eq!(decks.deck_of(&part), Some(dir.join("talk.md")));
        assert_eq!(decks.deck_of(&dir.join("talk.md")), None);
        assert!(!decks.update());
        fs::remove_dir_all(&dir).unwrap();
    }
}
