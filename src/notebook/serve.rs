//! Running cells one at a time, as they are sent, for the VS Code extension,
//! whose WebAssembly cannot start a kernel: they run on the kernel the deck
//! would run them on, where the deck would run them, and their outputs are
//! saved where the deck reads them.
//!
//! Each line of stdin is a request, and each line of stdout the answer to one,
//! both in JSON. The first line out says which kernel started, or why none
//! did. Closing stdin shuts the kernel down, and an interrupt is passed on to
//! it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::audit;
use super::environment::Environment;
use super::execute::list_inputs;
use super::kernel::{Kernel, Ran};
use crate::markdown::code;
use crate::paths::parent;
use crate::store::{self, Output};

#[derive(Deserialize)]
struct Request {
    /// The sources of the notebook's code cells up to the one to run, which
    /// is the last: its address is the hash of them all.
    cells: Vec<String>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Answer {
    Started {
        kernel: String,
    },
    Ran {
        outputs: Vec<Item>,
        ok: bool,
        count: usize,
        /// Whether the outputs were saved along with the files the cell read,
        /// so that the deck shows them rather than running the cell again.
        trusted: bool,
    },
    Failed {
        error: String,
    },
}

/// An output, as VS Code holds it.
#[derive(Serialize)]
struct Item {
    mime: &'static str,
    /// Its bytes, in base64.
    data: String,
    /// The file it is saved in.
    name: String,
}

/// An output as the deck saves it, so that the notebook shows what the slides
/// will: a figure as the SVG a slide inlines, and a traceback as plain text.
fn item(output: &Output) -> Item {
    Item {
        mime: store::notebook_mime(output.extension),
        data: BASE64.encode(&output.bytes),
        name: store::name(output),
    }
}

/// A notebook's kernel, and what the addresses of its cells start from.
struct Notebook {
    kernel: Kernel,
    /// The directory of the file, which the kernel runs in.
    dir: PathBuf,
    /// The lock file of its environment, which its cells are addressed by.
    environment: Vec<u8>,
    /// The code the kernel has run, in order, unless it was interrupted,
    /// which leaves its state that of no cells in any order.
    history: Option<Vec<String>>,
    /// Whether an interrupt came while the cell ran.
    interrupted: Arc<AtomicBool>,
}

impl Notebook {
    /// Starts a kernel as the deck does for `file`: next to it, in the
    /// environment its lock file pins, if it has one.
    async fn start(file: &Path, kernels: &'static [&'static str]) -> Result<Notebook> {
        let dir = parent(file).to_path_buf();
        let lock = store::lock_file(&dir);
        let environment = match &lock {
            Some(lock) => Some(Environment::activate(lock).await?),
            None => None,
        };
        let mut kernel = Kernel::start(kernels, &dir, environment.as_ref()).await?;
        kernel.run_silent(&audit::install()).await?;
        Ok(Notebook {
            kernel,
            environment: store::environment(lock.as_deref()),
            dir,
            history: Some(Vec::new()),
            interrupted: Arc::default(),
        })
    }

    /// Runs the last of `cells` and saves its outputs under its address,
    /// along with the files it read if the kernel has run the cells before it,
    /// and nothing else, since it started. The outputs of a cell that was
    /// interrupted are not saved. Returns whether they were saved with the
    /// files read.
    async fn run(&mut self, cells: &[String]) -> Result<(Ran, bool)> {
        let codes: Vec<String> = cells.iter().map(|source| code(source)).collect();
        let code = codes.last().context("no cell to run")?;
        self.interrupted.store(false, Ordering::SeqCst);
        let ran = self.kernel.run_cell(code).await?;
        if self.interrupted.load(Ordering::SeqCst) {
            self.history = None;
            return Ok((ran, false));
        }
        let in_order = self.history.as_mut().is_some_and(|history| {
            history.push(code.clone());
            *history == codes
        });
        let address = store::hashes(&self.dir, &self.environment, &codes)
            .pop()
            .unwrap();
        let root = store::root(&self.dir);
        store::create(&root)?;
        let dir = store::save(&root, &address, &ran.outputs)?;
        if !in_order {
            return Ok((ran, false));
        }
        match list_inputs(&mut self.kernel, &dir).await {
            Ok(()) => Ok((ran, true)),
            Err(error) => {
                eprintln!("{error:#}");
                Ok((ran, false))
            }
        }
    }
}

/// Runs the cells of the markdown file `file` as they are sent, on the first
/// of `kernels` that is installed.
pub fn serve(file: &Path, kernels: &'static [&'static str]) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        let mut notebook = match Notebook::start(file, kernels).await {
            Ok(notebook) => notebook,
            Err(error) => {
                let error = format!("{error:#}");
                answer(&Answer::Failed { error }).await?;
                return Ok(());
            }
        };
        answer(&Answer::Started {
            kernel: notebook.kernel.name.clone(),
        })
        .await?;
        // An interrupt is for the cell running, which the kernel stops when
        // signalled, as it does in a terminal.
        let mut interrupter = notebook.kernel.interrupter().await?;
        let interrupted = notebook.interrupted.clone();
        tokio::spawn(async move {
            while tokio::signal::ctrl_c().await.is_ok() {
                interrupted.store(true, Ordering::SeqCst);
                if let Err(error) = interrupter.interrupt().await {
                    eprintln!("could not interrupt the kernel: {error:#}");
                }
            }
        });

        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        while let Some(line) = lines.next_line().await? {
            let ran = match serde_json::from_str::<Request>(&line) {
                Ok(request) => notebook.run(&request.cells).await,
                Err(error) => Err(error.into()),
            };
            match ran {
                Ok((ran, trusted)) => {
                    answer(&Answer::Ran {
                        outputs: ran.outputs.iter().map(item).collect(),
                        ok: ran.ok,
                        count: ran.count,
                        trusted,
                    })
                    .await?
                }
                Err(error) => {
                    let error = format!("{error:#}");
                    answer(&Answer::Failed { error }).await?;
                    break;
                }
            }
        }
        notebook.kernel.shutdown().await
    })
}

async fn answer(answer: &Answer) -> Result<()> {
    let mut line = serde_json::to_vec(answer)?;
    line.push(b'\n');
    let mut stdout = tokio::io::stdout();
    stdout.write_all(&line).await?;
    stdout.flush().await?;
    Ok(())
}
