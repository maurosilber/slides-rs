//! Run the code cells of one notebook through a Jupyter kernel and save
//! their outputs, ready to be inserted into HTML.

use std::path::PathBuf;

use anyhow::Result;
use indicatif::ProgressBar;

use super::audit;
use super::kernel::Kernel;
use crate::{progress, store};

/// Runs the cells of one notebook, in order, on a kernel of its own (the
/// first of `kernels` that is installed) started in `dir`, and saves under
/// `root` the outputs of those that are `missing`, each under its hash in
/// `hashes`. Advances `bar` by a cell as each one runs, and returns how many
/// outputs it saved.
pub async fn execute_cells(
    kernels: &[&str],
    dir: PathBuf,
    root: PathBuf,
    cells: Vec<String>,
    hashes: Vec<String>,
    missing: Vec<bool>,
    bar: ProgressBar,
) -> Result<usize> {
    // The cells after the last one to save need not run: none of them is
    // saved, and no cell to save comes after them to use what they define.
    let Some(last) = missing.iter().rposition(|&missing| missing) else {
        return Ok(0);
    };
    bar.set_length(last as u64 + 1);
    bar.set_message("starting the kernel");
    // One kernel runs every cell in order: a cell can use what an earlier one
    // defined, so none of them can be skipped just because its output is
    // already saved. Only the writing is skipped.
    let mut kernel = Kernel::start(kernels, &dir).await?;
    kernel.run_silent(&audit::install()).await?;
    bar.set_message(format!("on {}", kernel.name));
    let mut saved = 0;
    for ((code, hash), missing) in cells.iter().zip(&hashes).zip(missing).take(last + 1) {
        let outputs = kernel.run(code).await?;
        bar.inc(1);
        if !missing {
            continue;
        }
        let dir = store::save(&root, hash, &outputs).await?;
        saved += 1;
        // The kernel may run in another directory, so it gets the full path.
        let files = std::path::absolute(dir.join(store::INPUTS))?;
        let listed = async {
            kernel.run_silent(&audit::save(&files)).await?;
            store::hash_files(&files)?;
            anyhow::Ok(())
        };
        if let Err(error) = listed.await {
            progress::log(&bar, format!("{}: {error:#}", files.display()));
        }
    }
    kernel.shutdown().await?;
    Ok(saved)
}
