//! The address of every cell, which its outputs are saved under.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::{hex, python};

/// The address of every cell: each one is the hash of all the code the kernel
/// has run up to and including that cell, one cell after another. A cell is
/// hashed by its syntax tree, so reformatting it keeps its address.
///
/// One kernel runs the whole document, so a cell's output depends on the state
/// the cells before it left behind. Hashing a cell on its own would give the
/// same address to two cells that read different values of the same name.
///
/// The chain starts from `dir`, the directory the kernel runs in, as the same
/// code reads other files elsewhere, and from `environment`, the lock file of
/// the packages the kernel imports, so that changing a package re-runs every
/// cell.
pub fn hashes(dir: &Path, environment: &[u8], cells: &[impl AsRef<str>]) -> Vec<String> {
    let mut hasher = Sha256::new();
    // A path holds no NUL, which ends it unambiguously.
    hasher.update(dir.as_os_str().as_encoded_bytes());
    hasher.update([0]);
    hasher.update(environment);
    cells
        .iter()
        .map(|code| {
            python::update(&mut hasher, code.as_ref());
            // Clone to read the digest so far without ending the chain.
            hex(hasher.clone().finalize())
        })
        .collect()
}

/// The lock files that pin the Python environment, most preferred first.
pub const LOCK_FILES: &[&str] = &["pixi.lock", "uv.lock"];

/// The lock file in `dir` or the closest directory above it, if the
/// environment is locked at all.
pub fn lock_file(dir: &Path) -> Option<PathBuf> {
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    dir.ancestors().find_map(|dir| {
        LOCK_FILES
            .iter()
            .map(|name| dir.join(name))
            .find(|path| path.is_file())
    })
}

/// What the cells' hashes start from: the contents of `lock`, or nothing.
pub fn environment(lock: Option<&Path>) -> Vec<u8> {
    lock.and_then(|path| std::fs::read(path).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::super::HASH_LEN;
    use super::*;

    #[test]
    fn a_hash_covers_the_cells_before_it() {
        let deck = Path::new("/deck");
        let hashes = hashes(deck, b"", &["a = 1\n", "a\n"]);
        assert_eq!(hashes.len(), 2);
        assert!(hashes.iter().all(|hash| hash.len() == HASH_LEN));
        // The first cell is addressed by its own code alone.
        assert_eq!(hashes[0], self::hashes(deck, b"", &["a = 1\n"])[0]);
        // The second is addressed by both.
        assert_ne!(hashes[1], self::hashes(deck, b"", &["a\n"])[0]);
    }

    #[test]
    fn editing_a_cell_readdresses_the_ones_after_it() {
        let before = hashes(Path::new("/deck"), b"", &["a = 1\n", "a\n"]);
        let after = hashes(Path::new("/deck"), b"", &["a = 2\n", "a\n"]);
        assert_ne!(before[0], after[0]);
        assert_ne!(before[1], after[1]);
    }

    #[test]
    fn reformatting_a_cell_keeps_every_address() {
        let before = hashes(Path::new("/deck"), b"", &["a = 1\n", "a\n"]);
        let after = hashes(Path::new("/deck"), b"", &["a=1  # one\n\n", "a\n"]);
        assert_eq!(before, after);
    }

    #[test]
    fn the_same_cells_elsewhere_have_other_addresses() {
        let here = hashes(Path::new("/deck"), b"", &["open('data.txt')\n"]);
        let there = hashes(Path::new("/deck/sub"), b"", &["open('data.txt')\n"]);
        assert_ne!(here, there);
    }

    #[test]
    fn changing_the_environment_readdresses_every_cell() {
        let before = hashes(Path::new("/deck"), b"numpy 1", &["a = 1\n", "a\n"]);
        let after = hashes(Path::new("/deck"), b"numpy 2", &["a = 1\n", "a\n"]);
        assert_ne!(before[0], after[0]);
        assert_ne!(before[1], after[1]);
    }
}
