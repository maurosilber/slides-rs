//! Opening the page in the default browser.

use std::path::Path;
use std::process::{Command, Stdio};

/// Opens the page at `path` in the default browser, with whatever the
/// system opens files with, reporting it if that cannot run.
pub fn open(path: &Path) {
    let mut command = if cfg!(target_os = "macos") {
        Command::new("open")
    } else if cfg!(windows) {
        // `start` is built into the shell, and takes its first quoted
        // argument for the title of the window.
        let mut command = Command::new("cmd");
        command.args(["/C", "start", ""]);
        command
    } else {
        Command::new("xdg-open")
    };
    let spawned = command
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match spawned {
        // Waited for apart, as `xdg-open` may wait for the browser, so that
        // it is not left behind for as long as the deck is watched.
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(error) => eprintln!("could not open {} in the browser: {error}", path.display()),
    }
}
