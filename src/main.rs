//! Run the code cells of a document through a Jupyter kernel and save their
//! outputs, ready to be inserted into HTML.

mod kernel;
mod output;

use std::path::Path;

use anyhow::Result;

use kernel::Kernel;

/// Where the outputs of every cell are stored.
const OUTPUT_DIR: &str = "_outputs";

/// The kernels the cells can run on, most preferred first, as
/// `jupyter kernelspec list` names them: xeus-python's `xpython`, then
/// ipykernel's `python3`. Both carry a shell that forwards stdout and renders
/// figures, unlike xeus-python's `xpython-raw`, which is left out on purpose.
const KERNELS: &[&str] = &["xpython", "python3"];

const CELLS: &[&str] = &[
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

#[tokio::main]
async fn main() -> Result<()> {
    let root = Path::new(OUTPUT_DIR);
    tokio::fs::create_dir_all(root).await?;

    let mut kernel = Kernel::start(KERNELS).await?;
    // One kernel runs every cell in order, so a cell can use what an earlier
    // one defined, and none of them can be skipped.
    for code in CELLS {
        let outputs = kernel.run(code).await?;
        let dir = output::save(root, code, &outputs).await?;
        println!("{}: {} output(s)", dir.display(), outputs.len());
    }
    kernel.shutdown().await
}
