//! Run the code cells of a document through a Jupyter kernel and save their
//! outputs, ready to be inserted into HTML.

mod kernel;
mod output;

use std::path::Path;

use anyhow::Result;
use clap::{Parser, ValueEnum};

use kernel::Kernel;

/// Where the outputs of every cell are stored.
const OUTPUT_DIR: &str = "_outputs";

/// The kernels the cells can run on, most preferred first, as
/// `jupyter kernelspec list` names them: xeus-python's `xpython`, then
/// ipykernel's `python3`. Both carry a shell that forwards stdout and renders
/// figures, unlike xeus-python's `xpython-raw`, which is left out on purpose.
const KERNELS: &[&str] = &["xpython", "python3"];

/// Which kernel to run the cells on.
#[derive(Clone, Copy, ValueEnum)]
enum KernelChoice {
    /// xeus-python's kernel.
    #[value(name = "xpython")]
    Xpython,
    /// ipykernel's kernel.
    #[value(name = "python3")]
    Python3,
}

impl KernelChoice {
    /// The name this kernel is installed under.
    fn kernelspec(self) -> &'static str {
        match self {
            KernelChoice::Xpython => "xpython",
            KernelChoice::Python3 => "python3",
        }
    }
}

/// Run the code cells of a document through a Jupyter kernel and save their
/// outputs.
#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Kernel to run the cells on. Without this, the first of xpython and
    /// python3 that is installed is used.
    #[arg(short, long, value_enum)]
    kernel: Option<KernelChoice>,
}

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
    let args = Args::parse();
    // Naming a kernel narrows the search to that one, so asking for a kernel
    // that is not installed is an error rather than a quiet fallback.
    let kernels = match args.kernel {
        Some(kernel) => &[kernel.kernelspec()][..],
        None => KERNELS,
    };

    let root = Path::new(OUTPUT_DIR);
    tokio::fs::create_dir_all(root).await?;

    let mut kernel = Kernel::start(kernels).await?;
    // One kernel runs every cell in order, so a cell can use what an earlier
    // one defined, and none of them can be skipped.
    for code in CELLS {
        let outputs = kernel.run(code).await?;
        let dir = output::save(root, code, &outputs).await?;
        println!("{}: {} output(s)", dir.display(), outputs.len());
    }
    kernel.shutdown().await
}
