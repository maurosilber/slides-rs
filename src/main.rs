mod deck;
mod notebook;
mod page;
mod progress;
mod watch;

use std::path::PathBuf;
use std::time::Instant;

use clap::{Parser as _, ValueEnum};

use deck::Deck;
use paths::{canonical, parent};
use slides::{markdown, paths, store};

/// Renders a markdown file into an HTML slide deck.
#[derive(clap::Parser)]
struct Cli {
    /// The markdown file to render.
    input: PathBuf,
    /// Where to write the HTML. Defaults to the input with an `.html` extension.
    output: Option<PathBuf>,
    /// Re-render on every change to the input or to a file it imports.
    #[arg(short, long)]
    watch: bool,
    /// Kernel to run the cells on. Without this, the first of xpython and
    /// python3 that is installed is used.
    #[arg(short, long, value_enum)]
    kernel: Option<KernelChoice>,
    /// After rendering, remove the saved outputs that no cell of the deck
    /// uses anymore, and anything else in the outputs directory. It is shared
    /// by every deck rendered next to it, so their outputs are removed too.
    #[arg(long)]
    clean: bool,
}

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
    /// The names this kernel is installed under.
    fn kernelspecs(self) -> &'static [&'static str] {
        match self {
            KernelChoice::Xpython => &["xpython"],
            KernelChoice::Python3 => &["python3"],
        }
    }
}

fn main() {
    let Cli {
        input,
        output,
        watch: watching,
        kernel,
        clean,
    } = Cli::parse();
    let output = output.unwrap_or_else(|| input.with_extension("html"));
    // The cache is keyed by canonical path, as watch events report those.
    let input = canonical(&input);
    // Naming a kernel narrows the search to that one, so asking for a kernel
    // that is not installed is an error rather than a quiet fallback.
    let kernels = kernel.map_or(notebook::KERNELS, KernelChoice::kernelspecs);

    let start = Instant::now();
    let mut deck = Deck::new(parent(&output).join(store::DIR), kernels);
    deck.update(std::slice::from_ref(&input));
    deck.write(&input, &output);
    eprintln!(
        "rendered {} in {}",
        output.display(),
        progress::duration(start.elapsed())
    );
    if clean {
        deck.clean();
    }

    if watching {
        watch::watch(&mut deck, &input, &output);
    }
}
