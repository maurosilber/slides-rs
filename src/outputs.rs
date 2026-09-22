//! The content-addressed store of cell outputs.

use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Hex characters kept from the SHA-256 of a cell. 64 bits of prefix keeps
/// paths readable and collisions out of reach for a document.
const HASH_LEN: usize = 16;

/// The address of a cell: the hash of its source, and nothing else.
pub fn hash(code: &str) -> String {
    let digest = Sha256::digest(code.as_bytes());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    hex[..HASH_LEN].to_string()
}

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Result<Store, String> {
        let dir = dir.into();
        fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        Ok(Store { dir })
    }

    /// Where the Jupyter outputs of the cell with this hash live.
    pub fn manifest(&self, hash: &str) -> PathBuf {
        self.dir.join(format!("{hash}.json"))
    }

    pub fn contains(&self, hash: &str) -> bool {
        self.manifest(hash).exists()
    }

    /// Outputs stored for a previous run of the same code.
    pub fn read(&self, hash: &str) -> Result<Vec<Value>, String> {
        let path = self.manifest(hash);
        let json =
            fs::read(&path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
        serde_json::from_slice(&json).map_err(|e| format!("{} is not valid: {e}", path.display()))
    }

    /// Write the outputs, plus a sidecar `.png` per image so that a renderer
    /// can point an `<img>` at it without decoding any base64.
    pub fn write(&self, hash: &str, outputs: &[Value]) -> Result<Vec<PathBuf>, String> {
        let mut written = Vec::new();
        for (i, output) in outputs.iter().enumerate() {
            let Some(Value::String(png)) = output.pointer("/data/image~1png") else {
                continue;
            };
            let bytes = BASE64
                .decode(png.as_bytes())
                .map_err(|e| format!("output {i} is not valid base64 PNG: {e}"))?;
            let path = self.dir.join(format!("{hash}-{i}.png"));
            write_file(&path, &bytes)?;
            written.push(path);
        }

        let json = serde_json::to_vec_pretty(&outputs).expect("outputs came from JSON");
        let path = self.manifest(hash);
        write_file(&path, &json)?;
        written.push(path);
        Ok(written)
    }
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::write(path, bytes).map_err(|e| format!("could not write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hash_addresses_the_code() {
        assert_eq!(hash("2 + 2").len(), HASH_LEN);
        assert_eq!(hash("2 + 2"), hash("2 + 2"));
        assert_ne!(hash("2 + 2"), hash("2 + 3"));
    }
}
