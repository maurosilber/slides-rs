//! Running cells one at a time, as they are sent, for the VS Code extension,
//! whose WebAssembly cannot start a kernel: they run on the kernel the deck
//! would run them on, where the deck would run them.
//!
//! Each line of stdin is a request, and each line of stdout the answer to one,
//! both in JSON. The first line out says which kernel started, or why none
//! did. Closing stdin shuts the kernel down, and an interrupt is passed on to
//! it.

use std::path::Path;

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::environment::Environment;
use super::kernel::Kernel;
use crate::paths::parent;
use crate::store::{self, Output};

#[derive(Deserialize)]
struct Request {
    /// The code of the cell to run.
    code: String,
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
}

/// An output as the deck saves it, so that the notebook shows what the slides
/// will: a figure as the SVG a slide inlines, and a traceback as plain text.
fn item(output: &Output) -> Item {
    let mime = store::MEDIA_TYPES
        .iter()
        .find(|&&(_, extension)| extension == output.extension)
        .map_or("text/plain", |&(mime, _)| mime);
    Item {
        mime,
        data: BASE64.encode(&output.bytes),
    }
}

/// Runs the cells of the markdown file `file` as they are sent, on the first
/// of `kernels` that is installed.
pub fn serve(file: &Path, kernels: &'static [&'static str]) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        let mut kernel = match start(file, kernels).await {
            Ok(kernel) => kernel,
            Err(error) => {
                let error = format!("{error:#}");
                answer(&Answer::Failed { error }).await?;
                return Ok(());
            }
        };
        answer(&Answer::Started {
            kernel: kernel.name.clone(),
        })
        .await?;
        // An interrupt is for the cell running, which the kernel stops when
        // signalled, as it does in a terminal.
        let mut interrupter = kernel.interrupter().await?;
        tokio::spawn(async move {
            while tokio::signal::ctrl_c().await.is_ok() {
                if let Err(error) = interrupter.interrupt().await {
                    eprintln!("could not interrupt the kernel: {error:#}");
                }
            }
        });

        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        while let Some(line) = lines.next_line().await? {
            let ran = match serde_json::from_str::<Request>(&line) {
                Ok(request) => kernel.run_cell(&request.code).await,
                Err(error) => Err(error.into()),
            };
            match ran {
                Ok(ran) => {
                    answer(&Answer::Ran {
                        outputs: ran.outputs.iter().map(item).collect(),
                        ok: ran.ok,
                        count: ran.count,
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
        kernel.shutdown().await
    })
}

/// Starts a kernel as the deck does for `file`: next to it, in the
/// environment its lock file pins, if it has one.
async fn start(file: &Path, kernels: &'static [&'static str]) -> Result<Kernel> {
    let dir = parent(file);
    let environment = match store::lock_file(dir) {
        Some(lock) => Some(Environment::activate(&lock).await?),
        None => None,
    };
    Kernel::start(kernels, dir, environment.as_ref()).await
}

async fn answer(answer: &Answer) -> Result<()> {
    let mut line = serde_json::to_vec(answer)?;
    line.push(b'\n');
    let mut stdout = tokio::io::stdout();
    stdout.write_all(&line).await?;
    stdout.flush().await?;
    Ok(())
}
