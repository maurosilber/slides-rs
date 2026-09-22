//! Running a cell in its own Python process.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;

const RUNNER: &str = include_str!("runner.py");

/// Execute `code` in a fresh interpreter and return its Jupyter outputs.
///
/// Each cell gets its own process, so nothing a cell defines leaks into the
/// next one.
pub fn run_cell(interpreter: &str, code: &str) -> Result<Vec<Value>, String> {
    let mut child = Command::new(interpreter)
        .arg("-c")
        .arg(RUNNER)
        // Figures are rendered to PNG, so there is no display to talk to.
        .env("MPLBACKEND", "Agg")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start `{interpreter}`: {e}"))?;

    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(code.as_bytes())
        .map_err(|e| format!("could not send the cell to `{interpreter}`: {e}"))?;

    let output = child
        .wait_with_output()
        .map_err(|e| format!("`{interpreter}` failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "`{interpreter}` exited with {}: {}",
            output.status,
            stderr.trim()
        ));
    }

    serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("could not read the outputs of `{interpreter}`: {e}"))
}

/// The first interpreter on `PATH` that runs, preferring `python3`.
pub fn find_interpreter() -> Result<String, String> {
    ["python3", "python"]
        .into_iter()
        .find(|candidate| {
            Command::new(candidate)
                .arg("-c")
                .arg("")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        })
        .map(String::from)
        .ok_or_else(|| "no `python3` or `python` on PATH".to_string())
}
