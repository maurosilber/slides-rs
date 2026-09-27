//! A markdown file as the cells of a notebook, and back: its code cells are
//! the ones the deck runs, and the markdown between them is a cell of its own.

use serde::{Deserialize, Serialize};
use slides::markdown::{code, code_cells, source};

/// The language of a code cell whose fence names none, as the deck runs every
/// cell as Python.
const PYTHON: &str = "python";

/// The language a notebook shows a code cell in, given the one its fence names.
fn shown(language: &str) -> &str {
    if language.is_empty() { PYTHON } else { language }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Markdown,
    Code,
}

/// A cell of the notebook.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Cell {
    pub kind: Kind,
    pub source: String,
    /// The language a code cell is in: the one its fence names, or Python.
    #[serde(default)]
    pub language: String,
    /// How the cell was written in the file it was read from, if it was.
    #[serde(default)]
    pub written: Option<Written>,
}

/// How a cell was written, so that a file is written back as it was but for
/// the cells that changed.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Written {
    pub kind: Kind,
    pub source: String,
    pub language: String,
    /// The cell as it was written, fences included.
    pub text: String,
    /// The blank lines before it and after it.
    pub leading: String,
    pub trailing: String,
}

/// The cells of a file.
pub fn cells(markdown: &str) -> Vec<Cell> {
    let mut cells = Vec::new();
    // Blank lines before the first cell, which it is written after.
    let mut leading = String::new();
    let mut start = 0;
    for cell in code_cells(markdown) {
        push_markdown(&markdown[start..cell.range.start], &mut cells, &mut leading);
        let written = &markdown[cell.range.clone()];
        let text = written.trim_end();
        let source = source(&cell.code).to_string();
        let language = cell
            .info
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string();
        cells.push(Cell {
            kind: Kind::Code,
            source: source.clone(),
            language: shown(&language).to_string(),
            written: Some(Written {
                kind: Kind::Code,
                source,
                language,
                text: text.to_string(),
                leading: std::mem::take(&mut leading),
                trailing: written[text.len()..].to_string(),
            }),
        });
        start = cell.range.end;
    }
    push_markdown(&markdown[start..], &mut cells, &mut leading);
    // A blank file is an empty cell, which keeps the blank lines.
    if !leading.is_empty() {
        cells.push(Cell {
            kind: Kind::Markdown,
            source: String::new(),
            language: String::new(),
            written: Some(Written {
                kind: Kind::Markdown,
                source: String::new(),
                language: String::new(),
                text: String::new(),
                leading,
                trailing: String::new(),
            }),
        });
    }
    cells
}

/// Pushes the markdown between two code cells as a cell of its own, unless
/// it is blank, which goes after the cell before it.
fn push_markdown(markdown: &str, cells: &mut Vec<Cell>, leading: &mut String) {
    let end = markdown.trim_end().len();
    if end == 0 {
        match cells.last_mut().and_then(|cell| cell.written.as_mut()) {
            Some(written) => written.trailing.push_str(markdown),
            None => leading.push_str(markdown),
        }
        return;
    }
    // The cell starts on the line of its first character, as indentation is
    // markdown too.
    let first = markdown.len() - markdown.trim_start().len();
    let start = markdown[..first]
        .rfind('\n')
        .map_or(0, |newline| newline + 1);
    leading.push_str(&markdown[..start]);
    let source = markdown[start..end].to_string();
    cells.push(Cell {
        kind: Kind::Markdown,
        source: source.clone(),
        language: String::new(),
        written: Some(Written {
            kind: Kind::Markdown,
            source: source.clone(),
            language: String::new(),
            text: source,
            leading: std::mem::take(leading),
            trailing: markdown[end..].to_string(),
        }),
    });
}

/// The file the cells make. A cell that has not changed since it was read is
/// written as it was, and one that has keeps the blank lines around it.
pub fn markdown(cells: &[Cell]) -> String {
    let cells: Vec<Cell> = cells.iter().map(as_written).collect();
    let mut markdown = String::new();
    // Whether the cell before was new, and so has no blank line after it.
    let mut after_new = false;
    for cell in &cells {
        match &cell.written {
            Some(written) => {
                if after_new {
                    separate(&mut markdown);
                }
                markdown.push_str(&written.leading);
                let unchanged = written.kind == cell.kind
                    && written.source == cell.source
                    && written.language == cell.language;
                if unchanged {
                    markdown.push_str(&written.text);
                } else {
                    markdown.push_str(&text(cell, Some(written)));
                }
                markdown.push_str(&written.trailing);
                after_new = false;
            }
            None => {
                separate(&mut markdown);
                markdown.push_str(&text(cell, None));
                markdown.push('\n');
                after_new = true;
            }
        }
    }
    markdown
}

/// The cell in the language its fence names: none, if it names none and the
/// cell is still shown in Python.
fn as_written(cell: &Cell) -> Cell {
    let mut cell = cell.clone();
    if let Some(written) = &cell.written
        && cell.kind == Kind::Code
        && cell.language == shown(&written.language)
    {
        cell.language = written.language.clone();
    }
    cell
}

/// Ends the file so far with a blank line, so that the next cell is apart.
fn separate(markdown: &mut String) {
    if markdown.is_empty() {
        return;
    }
    while !markdown.ends_with("\n\n") {
        markdown.push('\n');
    }
}

/// A cell written anew. A code cell keeps the fences it was written with,
/// unless it takes longer ones now, or another language.
fn text(cell: &Cell, written: Option<&Written>) -> String {
    match cell.kind {
        Kind::Markdown => cell.source.clone(),
        Kind::Code => {
            let (open, close) = written
                .filter(|written| written.kind == Kind::Code && written.language == cell.language)
                .and_then(|written| fences(&written.text))
                .filter(|(open, _)| tildes(open) > longest_tildes(&cell.source))
                .unwrap_or_else(|| {
                    let fence = "~".repeat((longest_tildes(&cell.source) + 1).max(3));
                    (format!("{fence}{}", cell.language), fence)
                });
            format!("{open}\n{}{close}", code(&cell.source))
        }
    }
}

/// The opening and closing fence of a code cell as written.
fn fences(text: &str) -> Option<(String, String)> {
    let (open, rest) = text.split_once('\n')?;
    let close = rest.rsplit('\n').next()?;
    Some((open.to_string(), close.to_string()))
}

/// The tildes a fence opens with.
fn tildes(line: &str) -> usize {
    line.trim_start().chars().take_while(|&c| c == '~').count()
}

/// The most tildes any line of the code starts with, which a fence around it
/// must outnumber.
fn longest_tildes(code: &str) -> usize {
    code.lines().map(tildes).max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;


    const SLIDES: &str = "---\ntheme: dark\n---\n\n# Title\n\n~~~python\nx = 1\n~~~\n\n~~~python {.hidden}\n\nx\n~~~\n\n---\n\n    indented\n\n~~~\n~~~\n";

    #[test]
    fn a_file_is_written_back_as_it_was() {
        assert_eq!(markdown(&cells(SLIDES)), SLIDES);
        for text in [
            "",
            "\n\n",
            "# Only\n",
            "~~~python\na\n~~~",
            "\n~~~\n~~~\n\nend",
        ] {
            assert_eq!(markdown(&cells(text)), text);
        }
    }

    #[test]
    fn the_cells_are_the_code_and_the_markdown_between() {
        let cells = cells(SLIDES);
        let kinds: Vec<Kind> = cells.iter().map(|cell| cell.kind).collect();
        use Kind::*;
        assert_eq!(kinds, [Markdown, Code, Code, Markdown, Code]);
        assert_eq!(cells[0].source, "---\ntheme: dark\n---\n\n# Title");
        assert_eq!(cells[1].source, "x = 1");
        assert_eq!(cells[1].language, "python");
        assert_eq!(cells[2].source, "\nx");
        assert_eq!(cells[3].source, "---\n\n    indented");
        assert_eq!(cells[4].source, "");
    }

    #[test]
    fn a_fence_that_names_no_language_is_shown_in_python() {
        let mut cells = cells("~~~
a
~~~
");
        assert_eq!(cells[0].language, "python");
        cells[0].source = "b".to_string();
        assert_eq!(markdown(&cells), "~~~
b
~~~
");
        cells[0].language = "bash".to_string();
        assert_eq!(markdown(&cells), "~~~bash
b
~~~
");
    }

    #[test]
    fn a_cell_has_the_code_the_deck_reads() {
        let codes: Vec<String> = cells(SLIDES)
            .iter()
            .filter(|cell| cell.kind == Kind::Code)
            .map(|cell| code(&cell.source))
            .collect();
        let rendered = slides::markdown::render(SLIDES, std::path::Path::new("slides.md"));
        assert_eq!(codes, rendered.cells);
    }

    #[test]
    fn a_changed_cell_keeps_its_fences_and_blank_lines() {
        let mut cells = cells(SLIDES);
        cells[2].source = "y".to_string();
        cells[3].source = "# Changed".to_string();
        let changed = markdown(&cells);
        assert!(changed.contains("\n\n~~~python {.hidden}\ny\n~~~\n\n# Changed\n\n~~~\n"));
    }

    #[test]
    fn a_new_cell_is_set_apart() {
        let mut cells = cells("# One\n~~~python\na\n~~~");
        let new = |kind, source: &str| Cell {
            kind,
            source: source.to_string(),
            language: "python".to_string(),
            written: None,
        };
        cells.insert(1, new(Kind::Code, "b"));
        cells.push(new(Kind::Markdown, "# Two"));
        assert_eq!(
            markdown(&cells),
            "# One\n\n~~~python\nb\n~~~\n\n~~~python\na\n~~~\n\n# Two\n"
        );
    }

    #[test]
    fn a_fence_outnumbers_the_tildes_in_its_code() {
        let mut cells = cells("~~~python\na\n~~~\n");
        cells[0].source = "~~~~\n".to_string();
        assert_eq!(markdown(&cells), "~~~~~python\n~~~~\n\n~~~~~\n");
    }
}
