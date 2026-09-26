//! Saving the outputs of the cells, and removing those no cell uses.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use super::{HASH_LEN, INPUTS, OUTPUT_HASH_LEN, Output, full_hex, is_hash};

/// The file in a cell's directory listing its outputs, one file name per
/// line, in the order the kernel produced them. Each output is saved next to
/// the cells' directories, named by the hash of its contents, so cells with
/// the same output share one file.
const OUTPUTS: &str = "outputs.txt";

/// Keeps the outputs out of git, next to them.
pub const GITIGNORE: &str = ".gitignore";

/// The name an output is saved under: the hash of its contents, with its extension.
pub fn name(output: &Output) -> String {
    format!(
        "{}.{}",
        full_hex(Sha256::digest(&output.bytes)),
        output.extension
    )
}

/// Saves each of a cell's outputs under `root`, named by the hash of its
/// contents, unless one with the same contents is there already, and lists
/// them in `<root>/<hash>/`, which it returns.
pub fn save(root: &Path, hash: &str, outputs: &[Output]) -> Result<PathBuf> {
    let mut list = String::new();
    for output in outputs {
        let name = name(output);
        let path = root.join(&name);
        if !path.try_exists().unwrap_or(false) {
            // Written aside and moved into place, so that a notebook that
            // saves the same output at the same time never reads it halfway.
            let partial = root.join(format!(".{name}.{}", uuid::Uuid::new_v4()));
            fs::write(&partial, &output.bytes)
                .with_context(|| format!("could not write {}", partial.display()))?;
            fs::rename(&partial, &path)
                .with_context(|| format!("could not write {}", path.display()))?;
        }
        list.push_str(&name);
        list.push('\n');
    }

    let dir = root.join(hash);
    // Replace the directory, so that the list of the files the cell read
    // from before does not outlive the outputs it was made for.
    match fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).context(format!("could not clear {}", dir.display())),
    }
    fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;
    let path = dir.join(OUTPUTS);
    fs::write(&path, list).with_context(|| format!("could not write {}", path.display()))?;
    Ok(dir)
}

/// The files a cell's outputs were saved in, in the order the kernel produced them.
pub fn saved(root: &Path, hash: &str) -> std::io::Result<Vec<PathBuf>> {
    let list = std::fs::read_to_string(root.join(hash).join(OUTPUTS))?;
    Ok(list.lines().map(|name| root.join(name)).collect())
}

/// What `remove_stale` removed.
#[derive(Debug, Default, PartialEq)]
pub struct Removed {
    pub cells: usize,
    pub outputs: usize,
    /// Whatever else was there, such as the outputs of an older layout.
    pub other: usize,
    pub bytes: u64,
}

impl Removed {
    /// Removes `path`, whatever it is, counting its bytes.
    fn remove(&mut self, path: &Path, file_type: std::fs::FileType) -> std::io::Result<()> {
        if file_type.is_dir() {
            self.bytes += size(path)?;
            std::fs::remove_dir_all(path)
        } else {
            self.bytes += std::fs::symlink_metadata(path)?.len();
            std::fs::remove_file(path)
        }
    }
}

/// Removes everything under `root` but its `.gitignore`, the directories of
/// the cells whose hash is in `keep`, holding only their lists, and the
/// outputs those cells list.
pub fn remove_stale(root: &Path, keep: &HashSet<&str>) -> std::io::Result<Removed> {
    let mut removed = Removed::default();
    let entries: Vec<std::fs::DirEntry> = match std::fs::read_dir(root) {
        Ok(entries) => entries.collect::<std::io::Result<_>>()?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(removed),
        Err(error) => return Err(error),
    };

    // The cells go first, as a cell that is kept keeps its outputs, whichever
    // cell saved them first.
    let mut listed = HashSet::new();
    for entry in &entries {
        let file_type = entry.file_type()?;
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let hash = name.to_str().filter(|name| is_hash(name, HASH_LEN));
        match hash {
            Some(hash) if keep.contains(hash) => {
                listed.extend(saved(root, hash).unwrap_or_default());
                for entry in std::fs::read_dir(entry.path())? {
                    let entry = entry?;
                    if entry.file_name() != INPUTS && entry.file_name() != OUTPUTS {
                        removed.remove(&entry.path(), entry.file_type()?)?;
                        removed.other += 1;
                    }
                }
            }
            Some(_) => {
                removed.remove(&entry.path(), file_type)?;
                removed.cells += 1;
            }
            None => {
                removed.remove(&entry.path(), file_type)?;
                removed.other += 1;
            }
        }
    }
    for entry in &entries {
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() || entry.file_name() == GITIGNORE || listed.contains(&path) {
            continue;
        }
        let is_output = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| is_hash(stem, OUTPUT_HASH_LEN))
            && path.extension().is_some();
        removed.remove(&path, file_type)?;
        if is_output {
            removed.outputs += 1;
        } else {
            removed.other += 1;
        }
    }
    Ok(removed)
}

/// The bytes taken by the files in `dir`, however deep.
fn size(dir: &Path) -> std::io::Result<u64> {
    let mut bytes = 0;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            bytes += size(&entry.path())?;
        } else {
            bytes += entry.metadata()?.len();
        }
    }
    Ok(bytes)
}

/// A size in bytes, in the largest binary unit that keeps it at least one.
pub fn human_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut size = bytes as f64 / 1024.0;
    let mut unit = 0;
    while size >= 1024.0 && unit + 1 < UNITS.len() {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::super::is_fresh;
    use super::super::tests::{save_cell, temp_root, text};
    use super::*;

    #[test]
    fn outputs_are_saved_by_their_contents_in_order() {
        let root = temp_root();
        let hash = "0123456789abcdef";
        let png = Output {
            extension: "png",
            bytes: vec![0; 3],
        };
        save_cell(&root, hash, &[text("b"), png, text("a")]);

        let saved = saved(&root, hash).unwrap();
        let contents: Vec<Vec<u8>> = saved
            .iter()
            .map(|path| std::fs::read(path).unwrap())
            .collect();
        assert_eq!(contents, [b"b".to_vec(), vec![0; 3], b"a".to_vec()]);
        let name = |path: &PathBuf| path.file_name().unwrap().to_str().unwrap().to_string();
        assert_eq!(
            name(&saved[0]),
            format!("{}.txt", full_hex(Sha256::digest(b"b")))
        );
        assert!(name(&saved[1]).ends_with(".png"));
        assert_eq!(saved[0].parent().unwrap(), root);
        assert!(is_fresh(&root, hash));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn cells_with_the_same_output_share_its_file() {
        let root = temp_root();
        save_cell(&root, "0123456789abcdef", &[text("same"), text("one")]);
        save_cell(&root, "fedcba9876543210", &[text("same")]);

        let first = saved(&root, "0123456789abcdef").unwrap();
        let second = saved(&root, "fedcba9876543210").unwrap();
        assert_eq!(first[0], second[0]);
        let outputs = std::fs::read_dir(&root)
            .unwrap()
            .filter(|entry| entry.as_ref().unwrap().file_type().unwrap().is_file())
            .count();
        assert_eq!(outputs, 2);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn saving_again_replaces_the_list_of_outputs() {
        let root = temp_root();
        let hash = "0123456789abcdef";
        save_cell(&root, hash, &[text("one"), text("two")]);
        save(&root, hash, &[text("one")]).unwrap();
        assert_eq!(saved(&root, hash).unwrap().len(), 1);
        // The list of the files read went with the outputs it was made for.
        assert!(!is_fresh(&root, hash));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn stale_cells_and_their_outputs_are_removed_and_measured() {
        let root = temp_root();
        let kept = "0123456789abcdef";
        let stale = "fedcba9876543210";
        save_cell(&root, kept, &[text("kept"), text("shared")]);
        save_cell(&root, stale, &[text("shared"), text("stale")]);
        std::fs::write(root.join(GITIGNORE), "*").unwrap();

        let removed = remove_stale(&root, &HashSet::from([kept])).unwrap();
        // The stale cell's list of outputs, its empty list of the files it
        // read, and the one output no kept cell lists.
        let outputs_list = 2 * (OUTPUT_HASH_LEN + ".txt\n".len());
        let expected_bytes = (outputs_list + "stale".len()) as u64;
        assert_eq!(
            removed,
            Removed {
                cells: 1,
                outputs: 1,
                other: 0,
                bytes: expected_bytes
            }
        );
        assert!(
            saved(&root, kept)
                .unwrap()
                .iter()
                .all(|path| path.is_file())
        );
        assert!(is_fresh(&root, kept));
        assert!(!root.join(stale).exists());
        assert!(root.join(GITIGNORE).exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn anything_else_is_removed_too() {
        let root = temp_root();
        let kept = "0123456789abcdef";
        save_cell(&root, kept, &[text("kept")]);
        std::fs::write(root.join(GITIGNORE), "*").unwrap();
        // An older layout: numbered outputs, next to a list of the files read.
        std::fs::write(root.join(kept).join("0.txt"), "old").unwrap();
        std::fs::write(root.join(kept).join("files.txt"), "").unwrap();
        // An output named by a shorter hash, a file left half written, and
        // things that were never outputs.
        std::fs::write(root.join("0123456789abcdef.txt"), "short").unwrap();
        std::fs::write(root.join(".partial.txt.1234"), "half").unwrap();
        std::fs::write(root.join("notes.txt"), "notes").unwrap();
        std::fs::create_dir_all(root.join("images")).unwrap();
        std::fs::write(root.join("images").join("a.png"), "png").unwrap();

        let removed = remove_stale(&root, &HashSet::from([kept])).unwrap();
        assert_eq!((removed.cells, removed.outputs, removed.other), (0, 0, 6));
        assert_eq!(removed.bytes, 3 + 5 + 4 + 5 + 3);
        let mut left: Vec<String> = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        let output = saved(&root, kept).unwrap()[0].clone();
        let output = output.file_name().unwrap().to_str().unwrap();
        assert_eq!(left, [GITIGNORE, kept, output]);
        let mut lists: Vec<String> = std::fs::read_dir(root.join(kept))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        lists.sort();
        assert_eq!(lists, [INPUTS, OUTPUTS]);
        assert!(is_fresh(&root, kept));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn sizes_are_shown_in_binary_units() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1536), "1.5 KiB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MiB");
    }
}
