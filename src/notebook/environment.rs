//! The environment a notebook's cells run in: the one its lock file pins,
//! which the addresses of its cells cover, rather than whichever one the deck
//! is rendered from.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use tokio::sync::OnceCell;

use crate::paths::parent;

/// The variables that point Python at an environment of their own, which the
/// environment the deck is rendered from may have set. Left to the kernel,
/// they would bring its packages in.
pub const FOREIGN: &[&str] = &["VIRTUAL_ENV", "PYTHONPATH", "PYTHONHOME"];

/// An environment, activated.
pub struct Environment {
    /// Where it is installed, which holds its kernels.
    pub prefix: PathBuf,
    /// The variables that activate it, which its kernel starts with.
    pub vars: HashMap<String, String>,
}

impl Environment {
    /// Activates the environment that `lock` pins, installing it first if it
    /// is not yet.
    pub async fn activate(lock: &Path) -> Result<Environment> {
        let dir = parent(lock);
        match lock.file_name().and_then(|name| name.to_str()) {
            Some("pixi.lock") => pixi(dir).await,
            Some("uv.lock") => Ok(venv(dir)),
            _ => anyhow::bail!("{}: not a lock file", lock.display()),
        }
    }
}

/// The environments of the notebooks in one run, each activated once, by the
/// first notebook that needs it, however many share it.
#[derive(Clone, Default)]
pub struct Environments(Arc<Mutex<HashMap<PathBuf, Activation>>>);

/// An environment being activated, or the outcome of activating it.
type Activation = Arc<OnceCell<Result<Arc<Environment>, String>>>;

impl Environments {
    /// The environment `lock` pins, yet to be activated.
    pub fn pinned(&self, lock: PathBuf) -> Pinned {
        let activation = self
            .0
            .lock()
            .unwrap()
            .entry(lock.clone())
            .or_default()
            .clone();
        Pinned { lock, activation }
    }
}

/// The environment a lock file pins, activated when a notebook first needs it.
pub struct Pinned {
    pub lock: PathBuf,
    activation: Activation,
}

impl Pinned {
    pub async fn activate(&self) -> Result<Arc<Environment>> {
        self.activation
            .get_or_init(|| async {
                Environment::activate(&self.lock)
                    .await
                    .map(Arc::new)
                    .map_err(|error| format!("{error:#}"))
            })
            .await
            .clone()
            .map_err(anyhow::Error::msg)
    }
}

/// The default environment of the pixi workspace in `dir`. Frozen, pixi
/// installs it as the lock file has it, without solving it again, so that it
/// is the environment the cells are addressed by.
async fn pixi(dir: &Path) -> Result<Environment> {
    let output = tokio::process::Command::new("pixi")
        .args(["shell-hook", "--json", "--frozen", "--manifest-path"])
        .arg(dir)
        .output()
        .await
        .context("could not run pixi, which the pixi.lock asks for")?;
    if !output.status.success() {
        anyhow::bail!(
            "pixi could not activate the environment in {}: {}",
            dir.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    #[derive(serde::Deserialize)]
    struct Hook {
        environment_variables: HashMap<String, String>,
    }
    let hook: Hook = serde_json::from_slice(&output.stdout)
        .context("pixi shell-hook printed something other than its environment")?;
    let prefix = hook
        .environment_variables
        .get("CONDA_PREFIX")
        .context("pixi activated no environment")?
        .into();
    Ok(Environment {
        prefix,
        vars: hook.environment_variables,
    })
}

/// The virtual environment uv keeps next to its lock file.
fn venv(dir: &Path) -> Environment {
    let prefix = dir.join(".venv");
    let bin = prefix.join(if cfg!(windows) { "Scripts" } else { "bin" });
    let path = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&path)))
        .unwrap_or(path);
    let vars = HashMap::from([
        ("VIRTUAL_ENV".to_string(), prefix.display().to_string()),
        ("PATH".to_string(), path.to_string_lossy().into_owned()),
    ]);
    Environment { prefix, vars }
}
