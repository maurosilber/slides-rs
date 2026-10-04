//! The outputs of a notebook's code cells, read from and saved to where the
//! deck saves them, under the address of each cell.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use slides::markdown::{Files, code};
use slides::paths::{joined, relative};
use slides::store::{self, FIGURE, MEDIA_TYPES, Output};

/// Where a notebook's file is.
#[derive(Deserialize)]
pub struct Place {
    /// Its directory, as the deck hashes it: on the disk, outside the WASM.
    pub dir: String,
    /// Its directory as this process reads it.
    pub path: PathBuf,
}

impl Place {
    /// The address of each code cell, given their sources in order.
    fn hashes(&self, sources: &[&str]) -> Vec<String> {
        let codes: Vec<String> = sources.iter().map(|source| code(source)).collect();
        self.addresses(&codes)
    }

    /// The address of each code cell, given their code in order, as the deck
    /// reads it from the markdown.
    fn addresses(&self, codes: &[String]) -> Vec<String> {
        let lock = store::lock_file(&self.path);
        store::hashes(
            Path::new(&self.dir),
            &store::environment(lock.as_deref()),
            codes,
        )
    }

    /// The html of the saved outputs of each code cell, given their code in
    /// order, as the deck shows them.
    pub fn html(&self, codes: &[String]) -> Vec<String> {
        let root = self.root();
        self.addresses(codes)
            .iter()
            .map(|hash| store::cell_html(&root, "", hash))
            .collect()
    }

    fn root(&self) -> PathBuf {
        store::root(&self.path)
    }
}

/// The files of the deck around `place`, read as this process reads them,
/// with the saved outputs of their cells, each hashed by its directory on the
/// disk, as the deck hashes it.
pub fn files(
    place: &Place,
) -> Files<impl FnMut(&Path) -> Option<String>, impl FnMut(&Path, &[String]) -> Vec<String>>
{
    let disk = PathBuf::from(&place.dir);
    let here = place.path.clone();
    Files {
        load: |path: &Path| fs::read_to_string(path).ok(),
        outputs: move |path: &Path, codes: &[String]| {
            let dir = path.parent().unwrap_or(&here);
            // Where the file is on the disk, as it is from the place.
            let relative = relative(&here, dir);
            let place = Place {
                dir: joined(&disk, &relative.to_string_lossy())
                    .to_string_lossy()
                    .into_owned(),
                path: dir.to_path_buf(),
            };
            place.html(codes)
        },
    }
}

/// One representation of an output, as VS Code holds it.
#[derive(Serialize, Deserialize)]
pub struct Item {
    pub mime: String,
    /// Its bytes, in base64.
    pub data: String,
}

/// An output read from the store.
#[derive(Serialize)]
pub struct Saved {
    /// The file it is saved in.
    pub name: String,
    #[serde(flatten)]
    pub item: Item,
}

/// The saved outputs of each code cell, given their sources in order. A
/// cell whose outputs are not saved has none.
pub fn load(place: &Place, sources: &[String]) -> Vec<Vec<Saved>> {
    let sources: Vec<&str> = sources.iter().map(String::as_str).collect();
    let root = place.root();
    place
        .hashes(&sources)
        .iter()
        .map(|hash| {
            let files = store::saved(&root, hash).unwrap_or_default();
            files.iter().filter_map(|file| read(file)).collect()
        })
        .collect()
}

fn read(file: &Path) -> Option<Saved> {
    let bytes = fs::read(file)
        .inspect_err(|error| eprintln!("{}: {error}", file.display()))
        .ok()?;
    let extension = file.extension().and_then(|extension| extension.to_str());
    let mime = store::notebook_mime(extension.unwrap_or_default());
    let bytes = store::notebook_data(extension.unwrap_or_default(), bytes);
    Some(Saved {
        name: file.file_name()?.to_str()?.to_string(),
        item: Item {
            mime: mime.to_string(),
            data: BASE64.encode(bytes),
        },
    })
}

/// A code cell and its outputs in the notebook.
#[derive(Deserialize)]
pub struct Cell {
    pub source: String,
    pub outputs: Vec<CellOutput>,
}

/// An output in the notebook, in every representation it has.
#[derive(Deserialize)]
pub struct CellOutput {
    /// The file it was read from, if it was read from the store.
    pub name: Option<String>,
    pub items: Vec<Item>,
}

/// Saves the outputs of each code cell that differ from the ones saved
/// under its address, and returns how many cells it saved.
///
/// Outputs that were all read from the store are left as they are, since
/// they may be of the code before it was edited, which the deck would still
/// run again. Saved from here, the outputs come without the list of the
/// files the cell read, so the deck does not trust them and runs the cell
/// again too, but they show until then.
pub fn save(place: &Place, cells: &[Cell]) -> Result<usize> {
    let sources: Vec<&str> = cells.iter().map(|cell| cell.source.as_str()).collect();
    let root = place.root();
    let mut saved = 0;
    for (cell, hash) in cells.iter().zip(place.hashes(&sources)) {
        let read =
            !cell.outputs.is_empty() && cell.outputs.iter().all(|output| output.name.is_some());
        if read {
            continue;
        }
        let outputs = cell
            .outputs
            .iter()
            .filter_map(|output| richest(output).transpose())
            .collect::<Result<Vec<Output>>>()?;
        let names: Vec<String> = outputs.iter().map(store::name).collect();
        let before: Vec<String> = store::saved(&root, &hash)
            .unwrap_or_default()
            .iter()
            .filter_map(|file| Some(file.file_name()?.to_str()?.to_string()))
            .collect();
        if names == before {
            continue;
        }
        store::create(&root)?;
        store::save(&root, &hash, &outputs)?;
        saved += 1;
    }
    Ok(saved)
}

/// VS Code's own media types, for what a kernel prints and the errors it
/// raises.
const STDOUT: &str = "application/vnd.code.notebook.stdout";
const STDERR: &str = "application/vnd.code.notebook.stderr";
const ERROR: &str = "application/vnd.code.notebook.error";

/// The richest representation of an output that the store can save, as the
/// deck keeps the richest one a kernel offers, if it has any.
fn richest(output: &CellOutput) -> Result<Option<Output>> {
    let mut best: Option<(usize, &Item)> = None;
    for item in &output.items {
        let mime = match item.mime.as_str() {
            STDOUT | STDERR | ERROR => "text/plain",
            FIGURE => "image/svg+xml",
            mime => mime,
        };
        let Some(rank) = MEDIA_TYPES.iter().position(|&(known, _)| known == mime) else {
            continue;
        };
        if best.is_none_or(|(best, _)| rank < best) {
            best = Some((rank, item));
        }
    }
    let Some((rank, item)) = best else {
        return Ok(None);
    };
    let mut bytes = BASE64
        .decode(&item.data)
        .with_context(|| format!("{} output is not valid base64", item.mime))?;
    if item.mime == ERROR {
        bytes = error_text(&bytes).into_bytes();
    }
    let bytes = store::saved_data(&item.mime, bytes);
    Ok(Some(Output {
        extension: MEDIA_TYPES[rank].1,
        bytes,
    }))
}

/// An error as VS Code holds it, as the deck saves a traceback.
fn error_text(json: &[u8]) -> String {
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Error {
        name: String,
        message: String,
        stack: Option<String>,
    }
    let error: Error = serde_json::from_slice(json).unwrap_or_default();
    error
        .stack
        .unwrap_or_else(|| format!("{}: {}", error.name, error.message))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(mime: &str, data: &[u8]) -> Item {
        Item {
            mime: mime.to_string(),
            data: BASE64.encode(data),
        }
    }

    fn output(items: Vec<Item>) -> CellOutput {
        CellOutput { name: None, items }
    }

    fn temp_dir(test: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("slides-notebook-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("deck").join("sections")).unwrap();
        dir
    }

    #[test]
    fn the_richest_representation_is_saved() {
        let figure = output(vec![
            item("text/plain", b"<Figure>"),
            item("image/png", b"png"),
            item("application/json", b"{}"),
        ]);
        let saved = richest(&figure).unwrap().unwrap();
        assert_eq!((saved.extension, saved.bytes), ("png", b"png".to_vec()));
        assert!(
            richest(&output(vec![item("application/json", b"{}")]))
                .unwrap()
                .is_none()
        );
        let stdout = richest(&output(vec![item(STDOUT, b"hi\n")]))
            .unwrap()
            .unwrap();
        assert_eq!(stdout.extension, "txt");
    }

    #[test]
    fn outputs_are_saved_where_the_deck_reads_them() {
        let dir = temp_dir("saved");
        let deck = dir.join("deck");
        fs::create_dir_all(deck.join(store::DIR)).unwrap();
        // The file is imported by a deck above it, which runs its cells there.
        let place = Place {
            dir: "/somewhere/deck/sections".to_string(),
            path: deck.join("sections"),
        };
        let cells = [
            Cell {
                source: "x = 1".to_string(),
                outputs: vec![],
            },
            Cell {
                source: "x".to_string(),
                outputs: vec![output(vec![item("text/plain", b"1")])],
            },
        ];
        assert_eq!(save(&place, &cells).unwrap(), 1);
        // Saving again changes nothing.
        assert_eq!(save(&place, &cells).unwrap(), 0);

        let sources = ["x = 1".to_string(), "x".to_string()];
        let loaded = load(&place, &sources);
        assert!(loaded[0].is_empty());
        assert_eq!(loaded[1].len(), 1);
        assert_eq!(loaded[1][0].item.mime, "text/plain");
        assert_eq!(BASE64.decode(&loaded[1][0].item.data).unwrap(), b"1");

        // The hash is the one the deck gives the cell.
        let hashes = store::hashes(
            Path::new("/somewhere/deck/sections"),
            &store::environment(store::lock_file(&place.path).as_deref()),
            &["x = 1\n", "x\n"],
        );
        assert!(deck.join(store::DIR).join(&hashes[1]).is_dir());
        assert!(!deck.join("sections").join(store::DIR).exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn outputs_read_from_the_store_are_left_as_they_are() {
        let dir = temp_dir("read");
        let place = Place {
            dir: "/deck".to_string(),
            path: dir.clone(),
        };
        let cells = [Cell {
            source: "edited".to_string(),
            outputs: vec![CellOutput {
                name: Some("old.txt".to_string()),
                items: vec![item("text/plain", b"old")],
            }],
        }];
        assert_eq!(save(&place, &cells).unwrap(), 0);
        assert!(!dir.join(store::DIR).exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
