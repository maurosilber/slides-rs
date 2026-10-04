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
pub use outputs::{GITIGNORE, create, human_size, name, remove_stale, save, saved};

use std::fs;
use std::path::Path;

use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd, html};

/// Where the outputs of every cell are stored, next to the rendered html.
pub const DIR: &str = "_outputs";

/// Hex characters kept from the SHA-256 of a cell. 64 bits keeps the paths
/// readable and collisions out of reach for one document.
const HASH_LEN: usize = 16;

/// Hex characters in the full SHA-256 of an output, which names its file.
const OUTPUT_HASH_LEN: usize = 64;

/// The media types an output can be saved as, most preferred first, with the
/// extension it is saved under. The ones before are richer, so of the
/// representations offered for the same output, the first one here is kept.
pub const MEDIA_TYPES: &[(&str, &str)] = &[
    ("text/html", "html"),
    ("image/svg+xml", "svg"),
    ("image/png", "png"),
    ("image/jpeg", "jpeg"),
    ("image/gif", "gif"),
    ("text/markdown", "md"),
    ("text/plain", "txt"),
];

/// The media type a figure is shown as in a notebook, rather than as
/// `image/svg+xml`, so that the extension's renderer, which steps through it
/// as a slide does, shows it, and no other notebook's SVGs.
pub const FIGURE: &str = "application/vnd.slides-rs.svg+xml";

/// The media type a notebook shows an output saved as `extension` as.
pub fn notebook_mime(extension: &str) -> &'static str {
    match extension {
        "svg" => FIGURE,
        _ => MEDIA_TYPES
            .iter()
            .find(|&&(_, known)| known == extension)
            .map_or("text/plain", |&(mime, _)| mime),
    }
}

/// An output saved as `extension` as a notebook shows it: a figure with its
/// steps numbered, as a slide that holds it alone numbers them, for the
/// extension's renderer to step through.
pub fn notebook_data(extension: &str, bytes: Vec<u8>) -> Vec<u8> {
    match (extension, String::from_utf8(bytes)) {
        ("svg", Ok(svg)) => crate::step::number_figure(&svg).into_bytes(),
        (_, Ok(text)) => text.into_bytes(),
        (_, Err(error)) => error.into_bytes(),
    }
}

/// An output as it is saved, from what a notebook shows as `mime`: a figure
/// as it was before its steps were numbered.
pub fn saved_data(mime: &str, bytes: Vec<u8>) -> Vec<u8> {
    match (mime, String::from_utf8(bytes)) {
        (FIGURE, Ok(svg)) => crate::step::unnumber_figure(&svg).into_bytes(),
        (_, Ok(text)) => text.into_bytes(),
        (_, Err(error)) => error.into_bytes(),
    }
}

/// Where the outputs of the cells of a file in `dir` are saved: in the
/// closest outputs directory above it, as the deck that imports it saves them
/// next to itself, or else next to it, as when it is rendered on its own.
pub fn root(dir: &std::path::Path) -> std::path::PathBuf {
    dir.ancestors()
        .map(|dir| dir.join(DIR))
        .find(|root| root.is_dir())
        .unwrap_or_else(|| dir.join(DIR))
}

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

/// A file in the outputs directory, as the deck links it from where it is:
/// through `outputs`, the outputs directory as the deck links it.
pub fn in_outputs(outputs: &str, name: &str) -> String {
    if outputs.is_empty() {
        name.to_string()
    } else {
        format!("{outputs}/{name}")
    }
}

/// The outputs a cell saved under `root`, in the order the kernel produced
/// them. Raster images are linked through `outputs`, the outputs directory
/// as the deck links it, and the rest is inlined: an SVG too, so that the
/// slides can reach into it for steps.
pub fn cell_html(root: &Path, outputs: &str, hash: &str) -> String {
    let Ok(files) = saved(root, hash) else {
        // The cell has not run, or its notebook failed; its error was reported then.
        return format!("<!-- no outputs for cell {hash} -->\n");
    };
    let mut html = String::new();
    for file in files {
        let name = file.file_name().unwrap().to_str().unwrap();
        let extension = file.extension().and_then(|extension| extension.to_str());
        if let Some("png" | "jpeg" | "gif") = extension {
            html.push_str(&format!("<img src=\"{}\">\n", in_outputs(outputs, name)));
            continue;
        }
        let Ok(mut text) = fs::read_to_string(&file) else {
            eprintln!("{}: could not read", file.display());
            continue;
        };
        if !text.ends_with('\n') {
            text.push('\n');
        }
        match extension {
            // SVGs were prepared for inlining when they were saved.
            Some("html" | "svg") => html.push_str(&text),
            Some("md") => html::push_html(&mut html, Parser::new(&text)),
            // Plain text keeps its layout, and is escaped on the way in.
            _ => html::push_html(
                &mut html,
                [
                    Event::Start(Tag::CodeBlock(CodeBlockKind::Indented)),
                    Event::Text(text.into()),
                    Event::End(TagEnd::CodeBlock),
                ]
                .into_iter(),
            ),
        }
    }
    html
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
    pub fn save_cell(root: &std::path::Path, hash: &str, outputs: &[Output]) {
        let dir = save(root, hash, outputs).unwrap();
        std::fs::write(dir.join(INPUTS), "").unwrap();
    }
}
