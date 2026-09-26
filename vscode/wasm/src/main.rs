//! The part of the VS Code extension that is shared with the deck, run as a
//! WASI command: it reads a request from the file named by its argument and
//! writes the response to stdout, both in JSON.

mod notebook;
mod outputs;

use std::io::Write;

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
    let path = std::env::args().nth(1).context("no request given")?;
    let request = std::fs::read(&path).with_context(|| format!("could not read {path}"))?;
    let request: Request = serde_json::from_slice(&request).context("invalid request")?;
    let response = respond(request)?;
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &response)?;
    stdout.flush()?;
    Ok(())
}
