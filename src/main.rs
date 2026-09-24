//! Run the code cells of a document through a Jupyter kernel and save their
//! outputs, ready to be inserted into HTML.

mod kernel;
mod output;

use std::path::Path;

use anyhow::Result;

use kernel::Kernel;

/// Where the outputs of every cell are stored.
const OUTPUT_DIR: &str = "_outputs";

/// The kernelspec to run the cells with, as `jupyter kernelspec list` names it.
const KERNEL: &str = "python3";

const INDEP_CELLS: &[&str] = &[
    "2 + 2",
    r#"
for i in range(3):
    print("line", i)
"#,
    r#"
import matplotlib.pyplot as plt

plt.plot([1, 2, 1])
"#,
];

const DEP_CELLS: &[&str] = &["x = 1", "x", "2 * x"];

#[tokio::main]
async fn main() -> Result<()> {
    let root = Path::new(OUTPUT_DIR);
    tokio::fs::create_dir_all(root).await?;
    tokio::fs::write(root.join(".gitignore"), "*").await?;

    execute_cells(root, INDEP_CELLS).await?;
    execute_cells(root, DEP_CELLS).await?;
    Ok(())
}

async fn execute_cells(root: &Path, cells: &[&str]) -> Result<()> {
    let hashes = output::hashes(cells);
    let missing: Vec<bool> = hashes
        .iter()
        .map(|hash| !output::exists(root, hash))
        .collect();
    if !missing.contains(&true) {
        println!("every cell is already in {}", root.display());
        return Ok(());
    }

    // One kernel runs every cell in order: a cell can use what an earlier one
    // defined, so none of them can be skipped just because its output is
    // already saved. Only the writing is skipped.
    let mut kernel = Kernel::start(KERNEL).await?;
    for ((code, hash), missing) in cells.iter().zip(&hashes).zip(missing) {
        let outputs = kernel.run(code).await?;
        if !missing {
            println!("{hash}: already saved");
            continue;
        }
        let dir = output::save(root, hash, &outputs).await?;
        println!("{}: {} output(s)", dir.display(), outputs.len());
    }
    kernel.shutdown().await
}
