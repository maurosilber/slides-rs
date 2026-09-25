//! Progress of the notebooks being run, drawn on stderr while it is a terminal.

use std::path::Path;
use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

/// A bar for the notebooks that are done, out of those that need to run.
pub fn total(multi: &MultiProgress, notebooks: usize) -> ProgressBar {
    let style = ProgressStyle::with_template(
        "{prefix:>8.bold} [{bar:30.cyan/blue}] {pos}/{len} files, {elapsed} elapsed, {eta} left",
    )
    .unwrap()
    .progress_chars("=> ");
    let bar = multi.add(ProgressBar::new(notebooks as u64).with_style(style));
    bar.set_prefix("running");
    bar.enable_steady_tick(Duration::from_millis(100));
    bar
}

/// A bar for the cells of a notebook that have run.
pub fn notebook(multi: &MultiProgress, path: &Path, cells: usize) -> ProgressBar {
    let style = ProgressStyle::with_template(
        "{spinner:.green} {prefix:.bold} [{bar:20}] {pos}/{len} cells, {elapsed}, {eta} left {msg:.dim}",
    )
    .unwrap()
    .progress_chars("=> ");
    let bar = multi.add(ProgressBar::new(cells as u64).with_style(style));
    bar.set_prefix(relative(path).display().to_string());
    bar.enable_steady_tick(Duration::from_millis(100));
    bar
}

/// Prints a line above the bars, or on its own when they are hidden, as they
/// are when stderr is not a terminal, so that the line still gets to a log.
pub fn log(bar: &ProgressBar, line: impl AsRef<str>) {
    if bar.is_hidden() {
        eprintln!("{}", line.as_ref());
    } else {
        bar.println(line);
    }
}

/// A duration as long as it takes to read, such as `1.2s` or `350ms`.
pub fn duration(duration: Duration) -> String {
    if duration < Duration::from_secs(1) {
        format!("{}ms", duration.as_millis())
    } else if duration < Duration::from_secs(60) {
        format!("{:.1}s", duration.as_secs_f64())
    } else {
        let secs = duration.as_secs();
        format!("{}m{:02}s", secs / 60, secs % 60)
    }
}

/// A path relative to the current directory, when it is inside it, as the
/// files are cached by their canonical path.
pub fn relative(path: &Path) -> &Path {
    std::env::current_dir()
        .ok()
        .and_then(|dir| path.strip_prefix(dir).ok())
        .unwrap_or(path)
}
