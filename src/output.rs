//! Cell outputs, saved as the bytes an HTML document will embed.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use jupyter_protocol::media::MediaType;
use jupyter_protocol::{ErrorOutput, Media, StreamContent};
use sha2::{Digest, Sha256};

use crate::python;

/// Hex characters kept from the SHA-256 of a cell. 64 bits keeps the paths
/// readable and collisions out of reach for one document.
const HASH_LEN: usize = 16;

/// The file in a cell's directory listing the files read up to that cell. It
/// is not an output, which are named by number.
pub const FILES: &str = "files.txt";

/// One output of a cell: either text or an image, with the file extension it
/// should be saved under.
pub struct Output {
    pub extension: &'static str,
    pub bytes: Vec<u8>,
}

impl Output {
    fn text(extension: &'static str, text: &str) -> Output {
        Output {
            extension,
            bytes: text.as_bytes().to_vec(),
        }
    }

    /// Image payloads arrive base64-encoded; store the decoded bytes so the
    /// file on disk is a real image.
    fn image(extension: &'static str, base64: &str) -> Result<Output> {
        let bytes = BASE64
            .decode(base64.trim())
            .with_context(|| format!("{extension} output is not valid base64"))?;
        Ok(Output { extension, bytes })
    }

    fn from_media(media: &MediaType) -> Result<Output> {
        match media {
            MediaType::Html(html) => Ok(Output::text("html", html)),
            MediaType::Svg(svg) => Ok(Output::text("svg", svg)),
            MediaType::Markdown(markdown) => Ok(Output::text("md", markdown)),
            MediaType::Plain(text) => Ok(Output::text("txt", text)),
            MediaType::Png(data) => Output::image("png", data),
            MediaType::Jpeg(data) => Output::image("jpeg", data),
            MediaType::Gif(data) => Output::image("gif", data),
            other => anyhow::bail!("unsupported media type {}", other.mime_type()),
        }
    }
}

/// How much we prefer each representation of the same output. A rank of 0
/// means we cannot put it in an HTML document, so it is never chosen.
fn rank(media: &MediaType) -> usize {
    match media {
        MediaType::Html(_) => 7,
        MediaType::Svg(_) => 6,
        MediaType::Png(_) => 5,
        MediaType::Jpeg(_) => 4,
        MediaType::Gif(_) => 3,
        MediaType::Markdown(_) => 2,
        MediaType::Plain(_) => 1,
        _ => 0,
    }
}

/// Collects what a kernel publishes for one cell, in the order it arrives.
#[derive(Default)]
pub struct Outputs {
    outputs: Vec<Output>,
    /// Which stream the last output came from, if it was a stream at all.
    open_stream: Option<&'static str>,
}

impl Outputs {
    /// A kernel splits a long `print` across several messages, so consecutive
    /// text from the same stream belongs in one file.
    pub fn push_stream(&mut self, stream: &StreamContent) {
        let name = match stream.name {
            jupyter_protocol::Stdio::Stdout => "stdout",
            jupyter_protocol::Stdio::Stderr => "stderr",
        };
        match self.outputs.last_mut() {
            Some(last) if self.open_stream == Some(name) => {
                last.bytes.extend_from_slice(stream.text.as_bytes());
            }
            _ => {
                self.outputs.push(Output::text("txt", &stream.text));
                self.open_stream = Some(name);
            }
        }
    }

    /// Keep the richest representation the kernel offered; a figure arrives as
    /// both a PNG and a `<Figure ...>` repr, and only the PNG belongs on a slide.
    pub fn push_media(&mut self, media: &Media) -> Result<()> {
        self.open_stream = None;
        let Some(richest) = media.richest(rank) else {
            return Ok(());
        };
        self.outputs.push(Output::from_media(richest)?);
        Ok(())
    }

    pub fn push_error(&mut self, error: &ErrorOutput) {
        self.open_stream = None;
        let traceback = error.traceback.join("\n");
        self.outputs
            .push(Output::text("txt", &strip_ansi(&traceback)));
    }

    pub fn into_vec(self) -> Vec<Output> {
        self.outputs
    }
}

/// Tracebacks come coloured with ANSI escapes, which are noise in HTML.
fn strip_ansi(text: &str) -> String {
    let mut clean = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            clean.push(c);
            continue;
        }
        // Drop the escape up to and including its final byte, `@` to `~`.
        if chars.next() == Some('[') {
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        }
    }
    clean
}

/// The address of every cell: each one is the hash of all the code the kernel
/// has run up to and including that cell, one cell after another. A cell is
/// hashed by its syntax tree, so reformatting it keeps its address.
///
/// One kernel runs the whole document, so a cell's output depends on the state
/// the cells before it left behind. Hashing a cell on its own would give the
/// same address to two cells that read different values of the same name.
///
/// The chain starts from `environment`, the lock file of the packages the
/// kernel imports, so that changing a package re-runs every cell.
pub fn hashes(environment: &[u8], cells: &[impl AsRef<str>]) -> Vec<String> {
    let mut hasher = Sha256::new();
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

fn hex(digest: impl AsRef<[u8]>) -> String {
    digest.as_ref()[..HASH_LEN / 2]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Whether this cell's outputs have already been saved.
pub fn exists(root: &Path, hash: &str) -> bool {
    root.join(hash).is_dir()
}

/// The files a cell's outputs were saved in, in the order the kernel produced them.
pub fn saved(root: &Path, hash: &str) -> std::io::Result<Vec<PathBuf>> {
    let mut files: Vec<(usize, PathBuf)> = std::fs::read_dir(root.join(hash))?
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let index = path.file_stem()?.to_str()?.parse().ok()?;
            Some((index, path))
        })
        .collect();
    // Sorted by number, as `10.txt` comes before `2.txt` by name.
    files.sort();
    Ok(files.into_iter().map(|(_, path)| path).collect())
}

/// Removes the saved outputs under `root` whose hash is not in `keep`,
/// returning how many were removed and how many bytes they took.
/// Anything that is not a cell's directory is left alone.
pub fn remove_stale(root: &Path, keep: &HashSet<&str>) -> std::io::Result<(usize, u64)> {
    let mut removed = 0;
    let mut bytes = 0;
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((0, 0)),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(hash) = name.to_str() else { continue };
        let is_hash = hash.len() == HASH_LEN && hash.bytes().all(|byte| byte.is_ascii_hexdigit());
        if !is_hash || !entry.file_type()?.is_dir() || keep.contains(hash) {
            continue;
        }
        let dir = entry.path();
        bytes += size(&dir)?;
        std::fs::remove_dir_all(&dir)?;
        removed += 1;
    }
    Ok((removed, bytes))
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

/// Save a cell's outputs under `<root>/<hash>/`, numbered in the order the
/// kernel produced them.
pub async fn save(root: &Path, hash: &str, outputs: &[Output]) -> Result<PathBuf> {
    let dir = root.join(hash);
    // Replace the directory so a cell that now produces fewer outputs does not
    // leave a stale file behind for the renderer to pick up.
    match tokio::fs::remove_dir_all(&dir).await {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).context(format!("could not clear {}", dir.display())),
    }
    tokio::fs::create_dir_all(&dir)
        .await
        .with_context(|| format!("could not create {}", dir.display()))?;

    for (i, output) in outputs.iter().enumerate() {
        let path = dir.join(format!("{i}.{}", output.extension));
        tokio::fs::write(&path, &output.bytes)
            .await
            .with_context(|| format!("could not write {}", path.display()))?;
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hash_covers_the_cells_before_it() {
        let hashes = hashes(b"", &["a = 1\n", "a\n"]);
        assert_eq!(hashes.len(), 2);
        assert!(hashes.iter().all(|hash| hash.len() == HASH_LEN));
        // The first cell is addressed by its own code alone.
        assert_eq!(hashes[0], self::hashes(b"", &["a = 1\n"])[0]);
        // The second is addressed by both.
        assert_ne!(hashes[1], self::hashes(b"", &["a\n"])[0]);
    }

    #[test]
    fn editing_a_cell_readdresses_the_ones_after_it() {
        let before = hashes(b"", &["a = 1\n", "a\n"]);
        let after = hashes(b"", &["a = 2\n", "a\n"]);
        assert_ne!(before[0], after[0]);
        assert_ne!(before[1], after[1]);
    }

    #[test]
    fn reformatting_a_cell_keeps_every_address() {
        let before = hashes(b"", &["a = 1\n", "a\n"]);
        let after = hashes(b"", &["a=1  # one\n\n", "a\n"]);
        assert_eq!(before, after);
    }

    #[test]
    fn changing_the_environment_readdresses_every_cell() {
        let before = hashes(b"numpy 1", &["a = 1\n", "a\n"]);
        let after = hashes(b"numpy 2", &["a = 1\n", "a\n"]);
        assert_ne!(before[0], after[0]);
        assert_ne!(before[1], after[1]);
    }

    #[test]
    fn stale_outputs_are_removed_and_measured() {
        let root = std::env::temp_dir().join(format!("slides-rs-{}", uuid::Uuid::new_v4()));
        let kept = "0123456789abcdef";
        let stale = "fedcba9876543210";
        std::fs::create_dir_all(root.join(kept)).unwrap();
        std::fs::create_dir_all(root.join(stale)).unwrap();
        std::fs::write(root.join(kept).join("0.txt"), "kept").unwrap();
        std::fs::write(root.join(stale).join("0.txt"), "stale").unwrap();
        std::fs::write(root.join(stale).join("1.png"), [0; 100]).unwrap();
        std::fs::write(root.join(".gitignore"), "*").unwrap();

        let removed = remove_stale(&root, &HashSet::from([kept])).unwrap();
        assert_eq!(removed, (1, 105));
        assert!(root.join(kept).is_dir());
        assert!(!root.join(stale).exists());
        assert!(root.join(".gitignore").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn sizes_are_shown_in_binary_units() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1536), "1.5 KiB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MiB");
    }

    #[test]
    fn ansi_escapes_are_removed() {
        let traceback = "\u{1b}[0;31mZeroDivisionError\u{1b}[0m: division by zero";
        assert_eq!(strip_ansi(traceback), "ZeroDivisionError: division by zero");
    }

    #[test]
    fn a_figure_is_saved_as_an_image_not_as_its_repr() {
        let media = Media {
            content: vec![
                MediaType::Plain("<Figure size 640x480>".to_string()),
                MediaType::Png(BASE64.encode(b"not really a png")),
            ],
        };
        let mut outputs = Outputs::default();
        outputs.push_media(&media).unwrap();
        let outputs = outputs.into_vec();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].extension, "png");
        assert_eq!(outputs[0].bytes, b"not really a png");
    }

    #[test]
    fn consecutive_stream_messages_become_one_output() {
        let mut outputs = Outputs::default();
        for text in ["one\n", "two\n"] {
            outputs.push_stream(&StreamContent {
                name: jupyter_protocol::Stdio::Stdout,
                text: text.to_string(),
            });
        }
        let outputs = outputs.into_vec();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].bytes, b"one\ntwo\n");
    }
}
