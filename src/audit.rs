//! Recording the files the cells read, from the kernel's side: the files they
//! open and those of the modules they import.
//!
//! `audit.py` runs in a module of its own rather than in the cells'
//! namespace, so a cell cannot see or clobber its names.

use std::path::Path;

const AUDIT: &str = include_str!("audit.py");

/// The module `audit.py` runs in.
const MODULE: &str = "__import__('sys').modules['_slides_audit']";

/// Installs the audit hook, recording from here on.
pub fn install() -> String {
    format!(
        "exec({}, __import__('sys').modules.setdefault('_slides_audit', \
         __import__('types').ModuleType('_slides_audit')).__dict__)\n",
        python_str(AUDIT)
    )
}

/// Adds the files of the modules imported so far, and writes every file
/// recorded so far to `path`.
pub fn save(path: &Path) -> String {
    format!(
        "{MODULE}.audit.add_modules()\n{MODULE}.audit.save({})\n",
        python_str(&path.to_string_lossy())
    )
}

/// A Python string literal holding `text`. Every escape JSON uses means the
/// same in Python.
fn python_str(text: &str) -> String {
    serde_json::to_string(text).unwrap()
}
