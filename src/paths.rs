//! Paths and reading files, the same way everywhere.

use std::fs;
use std::path::{Path, PathBuf};

/// The canonical path of a file that need not exist yet, so that the same file
/// is one cache entry however it is reached.
pub fn canonical(path: &Path) -> PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    match (parent(path).canonicalize(), path.file_name()) {
        (Ok(dir), Some(name)) => dir.join(name),
        _ => path.to_path_buf(),
    }
}

/// The directory holding a file, which for a bare file name is the current one.
pub fn parent(path: &Path) -> &Path {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    }
}

/// A file that cannot be read renders as nothing, so that watch mode
/// survives until it is created.
pub fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| {
        eprintln!("{}: {error}", path.display());
        String::new()
    })
}

/// How a page in `dir` links `path`, as a URL relative to it, with `/` for
/// every separator; nothing for `dir` itself. Both are canonical.
pub fn href(dir: &Path, path: &Path) -> String {
    let common = dir
        .components()
        .zip(path.components())
        .take_while(|(a, b)| a == b)
        .count();
    let up = dir.components().skip(common).map(|_| "..".to_string());
    let down = path
        .components()
        .skip(common)
        .map(|component| component.as_os_str().to_string_lossy().into_owned());
    up.chain(down).collect::<Vec<_>>().join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_href_climbs_out_of_the_page_and_down_to_the_file() {
        let href = |dir: &str, path: &str| href(Path::new(dir), Path::new(path));
        assert_eq!(href("/deck", "/deck"), "");
        assert_eq!(href("/deck", "/deck/_outputs"), "_outputs");
        assert_eq!(href("/deck/_outputs", "/deck"), "..");
        assert_eq!(href("/deck/out", "/deck/_outputs"), "../_outputs");
    }
}
