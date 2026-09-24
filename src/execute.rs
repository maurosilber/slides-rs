//! Run the code cells of a document through a Jupyter kernel and save their
//! outputs, ready to be inserted into HTML.

use std::path::PathBuf;

use anyhow::Result;

use crate::kernel::Kernel;
use crate::output;

/// The kernels the cells can run on, most preferred first, as
/// `jupyter kernelspec list` names them: xeus-python's `xpython`, then
/// ipykernel's `python3`. Both carry a shell that forwards stdout and renders
/// figures, unlike xeus-python's `xpython-raw`, which is left out on purpose.
pub const KERNELS: &[&str] = &["xpython", "python3"];

/// Runs the cells of one notebook, in order, on a kernel of its own (the
/// first of `kernels` that is installed), and saves
/// the outputs of those that are not saved yet under `root`.
pub async fn execute_cells(kernels: &[&str], root: PathBuf, cells: Vec<String>) -> Result<()> {
    let hashes = output::hashes(&cells);
    let missing: Vec<bool> = hashes
        .iter()
        .map(|hash| !output::exists(&root, hash))
        .collect();
    if !missing.contains(&true) {
        println!("every cell is already in {}", root.display());
        return Ok(());
    }

    // One kernel runs every cell in order: a cell can use what an earlier one
    // defined, so none of them can be skipped just because its output is
    // already saved. Only the writing is skipped.
    let mut kernel = Kernel::start(kernels).await?;
    for ((code, hash), missing) in cells.iter().zip(&hashes).zip(missing) {
        let outputs = kernel.run(code).await?;
        if !missing {
            println!("{hash}: already saved");
            continue;
        }
        let dir = output::save(&root, hash, &outputs).await?;
        println!("{}: {} output(s)", dir.display(), outputs.len());
    }
    kernel.shutdown().await
}
