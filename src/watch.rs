//! Re-rendering the deck as its files change.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};

use crate::deck::Deck;
use crate::paths::{canonical, parent};
use crate::{progress, store};

/// Re-renders the slides of `input`, as they are in the deck of `root`,
/// whenever one of the deck's files changes, until interrupted.
pub fn watch(deck: &mut Deck, root: &Path, input: &Path, output: &Path) {
    let (sender, receiver) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(sender).unwrap();
    let mut watched = HashSet::new();
    let mut readers = deck.readers();
    watch_dirs(&mut watcher, &mut watched, deck, &readers);
    eprintln!("watching {} files", deck.files.len() + readers.len());

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

        let paths: HashSet<PathBuf> = paths.iter().map(|path| canonical(path)).collect();
        // A lock file that changes, appears or goes away changes the hashes
        // of the cells in every file below it.
        let locks: Vec<&Path> = paths
            .iter()
            .filter(|path| is_lock_file(path))
            .filter_map(|path| path.parent())
            .collect();
        let mut changed: HashSet<PathBuf> = deck
            .files
            .keys()
            .filter(|path| paths.contains(*path) || locks.iter().any(|dir| path.starts_with(dir)))
            .cloned()
            .collect();
        // A file a cell read may have changed its outputs. Rendering the
        // markdown it is in again re-runs whichever of its cells are stale.
        for path in &paths {
            changed.extend(readers.get(path).into_iter().flatten().cloned());
        }
        let changed: Vec<PathBuf> = changed.into_iter().collect();
        if changed.is_empty() {
            continue;
        }

        // Only the changed files are rendered again; an import they still
        // share with the rest of the deck keeps its cached render.
        let start = Instant::now();
        let names: Vec<String> = changed
            .iter()
            .map(|path| progress::relative(path).display().to_string())
            .collect();
        eprintln!("changed: {}", names.join(", "));
        deck.update(&changed);
        deck.prune(root);
        readers = deck.readers();
        watch_dirs(&mut watcher, &mut watched, deck, &readers);
        let took = progress::duration(start.elapsed());
        if deck.write(root, input, output) {
            eprintln!(
                "rendered {} in {took}",
                progress::relative(output).display()
            );
        } else {
            eprintln!(
                "{} is unchanged, after {took}",
                progress::relative(output).display()
            );
        }
    }
}

/// Watches the directory of every file in the deck, of the lock files they
/// use and of the files their cells read, and no others: an editor saves by
/// replacing a file, which a watch on the file itself would not survive.
fn watch_dirs(
    watcher: &mut impl Watcher,
    watched: &mut HashSet<PathBuf>,
    deck: &Deck,
    readers: &HashMap<PathBuf, HashSet<PathBuf>>,
) {
    let mut dirs: HashSet<PathBuf> = deck
        .files
        .iter()
        .flat_map(|(path, file)| [Some(path), file.lock.as_ref()])
        .flatten()
        .chain(readers.keys())
        .map(|path| parent(path).to_path_buf())
        .collect();
    let new: Vec<PathBuf> = dirs.difference(watched).cloned().collect();
    for dir in new {
        // A cell may have tried to read a file in a directory that does not exist.
        if watcher.watch(&dir, RecursiveMode::NonRecursive).is_err() {
            dirs.remove(&dir);
        }
    }
    for dir in watched.difference(&dirs) {
        // The directory itself may be gone, which already ends the watch.
        let _ = watcher.unwatch(dir);
    }
    *watched = dirs;
}

fn is_lock_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| store::LOCK_FILES.contains(&name))
}
