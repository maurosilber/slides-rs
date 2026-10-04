mod browser;
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
use slides::{markdown, paths, step, store};

/// Renders a markdown file into an HTML slide deck.
#[derive(clap::Parser)]
#[command(subcommand_negates_reqs = true, args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// The markdown file to render.
    #[arg(required = true)]
    input: Option<PathBuf>,
    /// Where to write the HTML. Defaults to the input's name with an `.html`
    /// extension, in the outputs directory, which keeps it out of git.
    output: Option<PathBuf>,
    /// A deck that imports the input, directly or through other files, to
    /// render the input's slides as they are in it: in its theme and shape,
    /// with links as from its directory, and stepping as it does.
    #[arg(long, value_name = "DECK")]
    deck: Option<PathBuf>,
    /// Re-render on every change to the input or to a file it imports.
    #[arg(short, long)]
    watch: bool,
    /// Kernel to run the cells on. Without this, the first of xpython and
    /// python3 that is installed is used.
    #[arg(short, long, value_enum)]
    kernel: Option<KernelChoice>,
    /// After rendering, remove the saved outputs that no cell of the deck
    /// uses anymore, and anything else in the outputs directory but the
    /// pages in it. It is shared by every deck rendered next to it, so their
    /// outputs are removed too.
    #[arg(long)]
    clean: bool,
    /// Put every file the page links inside it: the stylesheets, the script,
    /// and the images of the slides and of the outputs, so that the html is
    /// all there is to share. KaTeX still loads from its CDN.
    #[arg(long)]
    self_contained: bool,
    /// Open the page in the default browser once it is rendered. With
    /// `--watch`, it opens once, and a reload shows each render after.
    #[arg(long)]
    open: bool,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Runs the cells of a markdown file as they are sent, for the VS Code
    /// extension.
    ///
    /// Each line of stdin names the code cells up to the one to run, as the
    /// JSON `{"cells": [...]}`, answered by a line of JSON on stdout with its
    /// outputs, which are saved where the deck reads them.
    Kernel {
        /// The markdown file the cells are in. They run next to it, in the
        /// environment its lock file pins, as when the deck renders it.
        file: PathBuf,
        /// Kernel to run the cells on. Without this, the first of xpython and
        /// python3 that is installed is used.
        #[arg(short, long, value_enum)]
        kernel: Option<KernelChoice>,
    },
    /// Prints the environment the cells of a markdown file run in, for the
    /// VS Code extension, as a line of JSON: the one its lock file pins,
    /// installed and activated as for its kernel.
    Environment {
        /// The markdown file the cells are in.
        file: PathBuf,
    },
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
        command,
        input,
        output,
        deck: importer,
        watch: watching,
        kernel,
        clean,
        self_contained,
        open,
    } = Cli::parse();
    if let Some(command) = command {
        let done = match command {
            Command::Kernel { file, kernel } => {
                let kernels = kernel.map_or(notebook::KERNELS, KernelChoice::kernelspecs);
                notebook::serve(&canonical(&file), kernels)
            }
            Command::Environment { file } => notebook::print_environment(&canonical(&file)),
        };
        if let Err(error) = done {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
        return;
    }
    // Required unless there is a command, which returned above.
    let input = input.unwrap();
    // The cache is keyed by canonical path, as watch events report those.
    let input = canonical(&input);
    let root = importer.as_deref().map_or_else(|| input.clone(), canonical);
    // The outputs are saved where the extension saves them for the same file.
    let outputs = store::root(parent(&root));
    let output = match output {
        Some(output) => {
            // Made first, for the path to be canonical, as links are written
            // from where the page is.
            if let Err(error) = std::fs::create_dir_all(parent(&output)) {
                eprintln!("{}: {error}", parent(&output).display());
                std::process::exit(1);
            }
            canonical(&output)
        }
        None => outputs.join(input.with_extension("html").file_name().unwrap()),
    };
    // Naming a kernel narrows the search to that one, so asking for a kernel
    // that is not installed is an error rather than a quiet fallback.
    let kernels = kernel.map_or(notebook::KERNELS, KernelChoice::kernelspecs);

    let start = Instant::now();
    let mut deck = Deck::new(outputs, kernels, self_contained);
    deck.update(std::slice::from_ref(&root));
    if !deck.imports(&root, &input) {
        eprintln!(
            "{} does not import {}",
            progress::relative(&root).display(),
            progress::relative(&input).display()
        );
        std::process::exit(1);
    }
    deck.write(&root, &input, &output);
    eprintln!(
        "rendered {} in {}",
        progress::relative(&output).display(),
        progress::duration(start.elapsed())
    );
    if clean {
        deck.clean(&root);
    }
    if open {
        browser::open(&output);
    }

    if watching {
        watch::watch(&mut deck, &root, &input, &output);
    }
}
