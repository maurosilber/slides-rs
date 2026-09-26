//! The files a cell read, which decide whether its saved outputs are still
//! its outputs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

use sha2::{Digest, Sha256};

use super::{full_hex, saved};

/// The file in a cell's directory listing the files read up to that cell,
/// outside the environment, with the hash of each, in the format of
/// `sha256sum`.
pub const INPUTS: &str = "inputs.txt";

/// Stands for the hash of a file that cannot be read, such as one that does
/// not exist, so that creating it later re-runs the cell.
const UNREADABLE: &str = "-";

/// Whether this cell's outputs have been saved, and the files it read have
/// not changed since. Outputs saved without a list of the files read are
/// not trusted.
pub fn is_fresh(root: &Path, hash: &str) -> bool {
    let saved = saved(root, hash).is_ok_and(|outputs| outputs.iter().all(|path| path.is_file()));
    saved
        && listed(root, hash).is_some_and(|files| {
            files
                .iter()
                .all(|(hash, path)| file_hash(Path::new(path)) == *hash)
        })
}

/// The files a cell read, as listed with its outputs, or none if there is no
/// list.
pub fn read_files(root: &Path, hash: &str) -> Vec<PathBuf> {
    listed(root, hash)
        .unwrap_or_default()
        .into_iter()
        .map(|(_, path)| PathBuf::from(path))
        .collect()
}

/// The hash and path of every file listed with a cell's outputs, or `None`
/// if there is no list or it cannot be parsed.
fn listed(root: &Path, hash: &str) -> Option<Vec<(String, String)>> {
    let files = std::fs::read_to_string(root.join(hash).join(INPUTS)).ok()?;
    files
        .lines()
        .map(|line| {
            let (hash, path) = line.split_once("  ")?;
            Some((hash.to_string(), path.to_string()))
        })
        .collect()
}

/// Replaces the list of files at `files`, one path per line, with the same
/// list along with the hash of each file as it is now.
pub fn hash_files(files: &Path) -> std::io::Result<()> {
    let list = std::fs::read_to_string(files)?;
    let hashed: String = list
        .lines()
        .map(|path| format!("{}  {path}\n", file_hash(Path::new(path))))
        .collect();
    std::fs::write(files, hashed)
}

/// The hash of each file as last read, along with its size and modification
/// time then, so that a file is read again only once it changes. Every cell
/// lists the files read by the cells before it too, so without this the same
/// files would be read once per cell, and again on every change in watch mode.
static FILE_HASHES: LazyLock<Mutex<HashMap<PathBuf, HashedFile>>> = LazyLock::new(Default::default);

/// A file's size and modification time, and the hash of its contents then.
type HashedFile = (u64, SystemTime, String);

fn file_hash(path: &Path) -> String {
    let Ok((len, modified)) =
        std::fs::metadata(path).and_then(|metadata| Ok((metadata.len(), metadata.modified()?)))
    else {
        return UNREADABLE.to_string();
    };
    if let Some((cached_len, cached_modified, hash)) = FILE_HASHES.lock().unwrap().get(path)
        && (*cached_len, *cached_modified) == (len, modified)
    {
        return hash.clone();
    }
    // A file written again after its metadata was read keeps the older time,
    // so the next check reads it again rather than trusting this hash.
    let hash: String = match std::fs::read(path) {
        Ok(bytes) => full_hex(Sha256::digest(bytes)),
        Err(_) => return UNREADABLE.to_string(),
    };
    FILE_HASHES
        .lock()
        .unwrap()
        .insert(path.to_path_buf(), (len, modified, hash.clone()));
    hash
}

#[cfg(test)]
mod tests {
    use super::super::save;
    use super::super::tests::{temp_root, text};
    use super::*;

    #[test]
    fn a_cell_is_fresh_until_a_file_it_read_changes() {
        let root = temp_root();
        let hash = "0123456789abcdef";
        let data = root.join("data.txt");
        let created = root.join("created.txt");
        std::fs::write(&data, "1").unwrap();
        let dir = save(&root, hash, &[text("out")]).unwrap();
        // No list of the files read yet.
        assert!(!is_fresh(&root, hash));

        let inputs = dir.join(INPUTS);
        let list = format!("{}\n{}\n", data.display(), created.display());
        std::fs::write(&inputs, list).unwrap();
        hash_files(&inputs).unwrap();
        assert!(is_fresh(&root, hash));

        std::fs::write(&data, "2").unwrap();
        assert!(!is_fresh(&root, hash));
        std::fs::write(&data, "1").unwrap();
        assert!(is_fresh(&root, hash));

        std::fs::write(&created, "").unwrap();
        assert!(!is_fresh(&root, hash));
        std::fs::remove_file(&created).unwrap();

        // An output that went missing makes the cell run again.
        std::fs::remove_file(&saved(&root, hash).unwrap()[0]).unwrap();
        assert!(!is_fresh(&root, hash));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
