//! The outputs of the cells, saved next to the rendered html: a directory
//! per cell, named by its address, listing the files it read and the outputs
//! it produced, which are saved beside the directories, named by the hash of
//! their contents.

mod address;
mod inputs;
mod outputs;
mod python;

pub use address::{LOCK_FILES, environment, hashes, lock_file};
pub use inputs::{INPUTS, hash_files, is_fresh, read_files};
pub use outputs::{GITIGNORE, human_size, remove_stale, save, saved};

/// Where the outputs of every cell are stored, next to the rendered html.
pub const DIR: &str = "_outputs";

/// Hex characters kept from the SHA-256 of a cell. 64 bits keeps the paths
/// readable and collisions out of reach for one document.
const HASH_LEN: usize = 16;

/// Hex characters in the full SHA-256 of an output, which names its file.
const OUTPUT_HASH_LEN: usize = 64;

/// One output of a cell: either text or an image, with the file extension it
/// should be saved under.
pub struct Output {
    pub extension: &'static str,
    pub bytes: Vec<u8>,
}

/// The first `HASH_LEN` hex characters of a digest.
fn hex(digest: impl AsRef<[u8]>) -> String {
    full_hex(&digest.as_ref()[..HASH_LEN / 2])
}

fn full_hex(digest: impl AsRef<[u8]>) -> String {
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Whether `name` is a hash of `len` hex characters, as the cells'
/// directories and the outputs are named.
fn is_hash(name: &str, len: usize) -> bool {
    name.len() == len && name.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    pub fn temp_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!("slides-rs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    pub fn text(text: &str) -> Output {
        Output {
            extension: "txt",
            bytes: text.as_bytes().to_vec(),
        }
    }

    /// Saves a cell's outputs, with an empty list of the files it read.
    pub async fn save_cell(root: &std::path::Path, hash: &str, outputs: &[Output]) {
        let dir = save(root, hash, outputs).await.unwrap();
        std::fs::write(dir.join(INPUTS), "").unwrap();
    }
}
