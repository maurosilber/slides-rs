//! The part of the VS Code extension that is shared with the deck, run as a
//! WASI command: it reads a request from the file named by its first argument
//! and writes the answer to the file named by its second, both in JSON. Not
//! to stdout, whose last chunk VS Code's WASI can drop when the process exits.

mod notebook;
mod outputs;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "camelCase")]
enum Request {
    /// The cells of a markdown file.
    Cells { markdown: String },
    /// The markdown file that cells make.
    Markdown { cells: Vec<notebook::Cell> },
    /// The saved outputs of each code cell of a file.
    Load {
        place: outputs::Place,
        sources: Vec<String>,
    },
    /// Saves the outputs of the code cells of a file.
    Save {
        place: outputs::Place,
        cells: Vec<outputs::Cell>,
    },
}

fn respond(request: Request) -> Result<serde_json::Value> {
    Ok(match request {
        Request::Cells { markdown } => serde_json::to_value(notebook::cells(&markdown))?,
        Request::Markdown { cells } => serde_json::to_value(notebook::markdown(&cells))?,
        Request::Load { place, sources } => serde_json::to_value(outputs::load(&place, &sources))?,
        Request::Save { place, cells } => serde_json::to_value(outputs::save(&place, &cells)?)?,
    })
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args.next().context("no request given")?;
    let answer = args.next().context("no file given for the answer")?;
    let request = std::fs::read(&path).with_context(|| format!("could not read {path}"))?;
    let request: Request = serde_json::from_slice(&request).context("invalid request")?;
    let response = serde_json::to_vec(&respond(request)?)?;
    // In one write, as VS Code's WASI writes the whole file on each.
    std::fs::write(&answer, response).with_context(|| format!("could not write {answer}"))?;
    Ok(())
}
