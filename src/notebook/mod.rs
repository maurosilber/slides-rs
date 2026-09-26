//! Running the code cells of the deck, a notebook per file, each on a
//! Jupyter kernel of its own.

mod audit;
mod execute;
mod kernel;
mod outputs;
mod svg;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use indicatif::MultiProgress;

use crate::paths::parent;
use crate::{progress, store};

/// The kernels the cells can run on, most preferred first, as
/// `jupyter kernelspec list` names them: xeus-python's `xpython`, then
/// ipykernel's `python3`. Both carry a shell that forwards stdout and renders
/// figures, unlike xeus-python's `xpython-raw`, which is left out on purpose.
pub const KERNELS: &[&str] = &["xpython", "python3"];

/// The code cells of one file, and the address each one's outputs are saved
/// under.
pub struct Notebook<'a> {
    pub path: &'a Path,
    pub cells: &'a [String],
    pub hashes: &'a [String],
}

/// Runs notebooks on the kernels they are allowed.
pub struct Runner {
    /// The kernels the cells can run on, most preferred first.
    kernels: &'static [&'static str],
    runtime: tokio::runtime::Runtime,
}

impl Runner {
    pub fn new(kernels: &'static [&'static str]) -> Runner {
        Runner {
            kernels,
            runtime: tokio::runtime::Runtime::new().unwrap(),
        }
    }

    /// Runs every notebook with a cell whose outputs are not saved under
    /// `root`, all at once, as each one has a kernel of its own. A notebook
    /// that fails leaves its missing outputs out of the deck, rather than the
    /// rest of the deck too.
    pub fn run(&self, root: &Path, notebooks: &[Notebook]) {
        let notebooks: Vec<_> = notebooks
            .iter()
            .filter_map(|notebook| {
                let missing: Vec<bool> = notebook
                    .hashes
                    .iter()
                    .map(|hash| !store::is_fresh(root, hash))
                    .collect();
                missing.contains(&true).then_some((notebook, missing))
            })
            .collect();
        if notebooks.is_empty() {
            return;
        }

        fs::create_dir_all(root).unwrap();
        fs::write(root.join(store::GITIGNORE), "*").unwrap();
        let multi = MultiProgress::new();
        // With a single notebook, its own bar already says it all.
        let total = (notebooks.len() > 1).then(|| progress::total(&multi, notebooks.len()));
        self.runtime.block_on(async {
            let tasks: Vec<_> = notebooks
                .into_iter()
                .map(|(notebook, missing)| {
                    let path: PathBuf = notebook.path.to_path_buf();
                    // A cell reads and writes files next to the markdown it is in.
                    let dir = parent(&path).to_path_buf();
                    let root = root.to_path_buf();
                    let cells = notebook.cells.to_vec();
                    let hashes = notebook.hashes.to_vec();
                    let bar = progress::notebook(&multi, &path, cells.len());
                    let total = total.clone();
                    let kernels = self.kernels;
                    let name = progress::relative(&path).display().to_string();
                    let task = async move {
                        let start = Instant::now();
                        let result = execute::execute_cells(
                            kernels,
                            dir,
                            root,
                            cells,
                            hashes,
                            missing,
                            bar.clone(),
                        )
                        .await;
                        let cells = bar.length().unwrap_or(0);
                        let took = progress::duration(start.elapsed());
                        progress::log(
                            &bar,
                            match &result {
                                Ok(saved) => format!(
                                    "{:>8} {name}: {cells} cell(s), {saved} saved, in {took}",
                                    "done",
                                ),
                                Err(error) => {
                                    format!("{:>8} {name} after {took}: {error:#}", "failed")
                                }
                            },
                        );
                        bar.finish_and_clear();
                        if let Some(total) = total {
                            total.inc(1);
                        }
                    };
                    (path, tokio::spawn(task))
                })
                .collect();
            // Let every notebook finish, so a failure in one does not cut
            // another short while it is still writing.
            for (path, task) in tasks {
                if let Err(error) = task.await {
                    eprintln!("{}: {error}", path.display());
                }
            }
        });
        if let Some(total) = total {
            total.finish_and_clear();
        }
    }
}
