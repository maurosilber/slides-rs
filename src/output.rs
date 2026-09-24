//! Cell outputs, saved as the bytes an HTML document will embed.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use jupyter_protocol::media::MediaType;
use jupyter_protocol::{ErrorOutput, Media, StreamContent};
use sha2::{Digest, Sha256};

/// Hex characters kept from the SHA-256 of a cell. 64 bits keeps the paths
/// readable and collisions out of reach for one document.
const HASH_LEN: usize = 16;

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
/// has run up to and including that cell, concatenated.
///
/// One kernel runs the whole document, so a cell's output depends on the state
/// the cells before it left behind. Hashing a cell on its own would give the
/// same address to two cells that read different values of the same name.
pub fn hashes(cells: &[&str]) -> Vec<String> {
    let mut hasher = Sha256::new();
    cells
        .iter()
        .map(|code| {
            hasher.update(code.as_bytes());
            // Clone to read the digest so far without ending the chain.
            hex(hasher.clone().finalize())
        })
        .collect()
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
        let hashes = hashes(&["a = 1\n", "a\n"]);
        assert_eq!(hashes.len(), 2);
        assert!(hashes.iter().all(|hash| hash.len() == HASH_LEN));
        // The first cell is addressed by its own code alone.
        assert_eq!(hashes[0], self::hashes(&["a = 1\n"])[0]);
        // The second is addressed as if both had been one cell.
        assert_eq!(hashes[1], self::hashes(&["a = 1\na\n"])[0]);
    }

    #[test]
    fn editing_a_cell_readdresses_the_ones_after_it() {
        let before = hashes(&["a = 1\n", "a\n"]);
        let after = hashes(&["a = 2\n", "a\n"]);
        assert_ne!(before[0], after[0]);
        assert_ne!(before[1], after[1]);
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
