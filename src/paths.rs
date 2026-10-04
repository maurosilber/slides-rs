//! Paths and reading files, the same way everywhere.

use std::fs;
use std::path::{Component, Path, PathBuf};

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
    relative(dir, path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// The path of `path` from `dir`, climbing out of `dir` as far as they
/// differ; empty for `dir` itself. Both are canonical, or joined.
pub fn relative(dir: &Path, path: &Path) -> PathBuf {
    let common = dir
        .components()
        .zip(path.components())
        .take_while(|(a, b)| a == b)
        .count();
    let up = dir.components().skip(common).map(|_| Component::ParentDir);
    up.chain(path.components().skip(common)).collect()
}

/// The path of `src` in `dir`, with its `.` and `..` taken out, as a path
/// is joined where it cannot be made canonical, as in the extension.
pub fn joined(dir: &Path, src: &str) -> PathBuf {
    let mut joined = PathBuf::new();
    for component in dir.join(src).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if joined.file_name().is_some() => {
                joined.pop();
            }
            component => joined.push(component),
        }
    }
    joined
}

/// The files being imported, each by the one before it, as the slides of a
/// deck are gathered. A file imported again within itself brings none,
/// rather than importing itself forever.
#[derive(Default)]
pub struct Importing(Vec<PathBuf>);

impl Importing {
    /// Imports the file at `path`, unless it is being imported already.
    /// Returns whether it is, to `leave` once its slides are gathered.
    pub fn enter(&mut self, path: &Path) -> bool {
        if self.0.iter().any(|importing| importing == path) {
            return false;
        }
        self.0.push(path.to_path_buf());
        true
    }

    /// Done with the file imported last.
    pub fn leave(&mut self) {
        self.0.pop();
    }
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
