//! Minimal CommonMark fenced-code-block scanner.
//!
//! We only need fences, so we skip a full markdown parse: it keeps the byte
//! offsets intact for whatever renders the slides later.

#[derive(Debug)]
pub struct Fence {
    /// Info string as written, without the fence characters (e.g. `{python}`).
    pub info: String,
    /// Contents of the fence, with the opening fence's indentation removed.
    pub code: String,
    /// 1-based line number of the opening fence.
    pub line: usize,
}

struct Marker {
    ch: u8,
    len: usize,
    indent: usize,
    info: String,
}

impl Marker {
    fn parse(line: &str) -> Option<Marker> {
        let indent = line.len() - line.trim_start_matches(' ').len();
        if indent > 3 {
            return None;
        }
        let rest = &line[indent..];
        let ch = match rest.as_bytes().first()? {
            b'`' => b'`',
            b'~' => b'~',
            _ => return None,
        };
        let len = rest.bytes().take_while(|&b| b == ch).count();
        if len < 3 {
            return None;
        }
        let info = rest[len..].trim().to_string();
        // A backtick fence's info string may not contain a backtick.
        if ch == b'`' && info.contains('`') {
            return None;
        }
        Some(Marker {
            ch,
            len,
            indent,
            info,
        })
    }

    fn closes(&self, open: &Marker) -> bool {
        self.ch == open.ch && self.len >= open.len && self.info.is_empty()
    }
}

/// Every fenced code block in `src`, in document order.
pub fn fences(src: &str) -> Vec<Fence> {
    let mut fences = Vec::new();
    let mut lines = src.lines().enumerate();
    while let Some((i, line)) = lines.next() {
        let Some(open) = Marker::parse(line) else {
            continue;
        };
        let mut code = String::new();
        for (_, line) in lines.by_ref() {
            if Marker::parse(line).is_some_and(|close| close.closes(&open)) {
                break;
            }
            // Strip up to as many leading spaces as the opening fence had.
            let strip = line.len() - line.trim_start_matches(' ').len();
            code.push_str(&line[strip.min(open.indent)..]);
            code.push('\n');
        }
        fences.push(Fence {
            info: open.info,
            code,
            line: i + 1,
        });
    }
    fences
}

/// The language of a braced info string: `{python}` and `{python, echo=false}`
/// both give `python`, while a bare `python` gives `None`.
fn braced_language(info: &str) -> Option<&str> {
    let inner = info.strip_prefix('{')?.strip_suffix('}')?;
    let first = inner.split([',', ' ', '\t']).next()?;
    Some(first.trim().trim_start_matches('.'))
}

/// Executable cells: fences tagged `{python}`, never a plain `python` fence.
pub fn python_cells(src: &str) -> Vec<Fence> {
    fences(src)
        .into_iter()
        .filter(|f| braced_language(&f.info) == Some("python"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_braced_python_is_executable() {
        let src = "\
```{python}
a
```

```python
b
```

``` {python, echo=false}
c
```

~~~{python}
d
~~~
";
        let code: Vec<_> = python_cells(src).into_iter().map(|f| f.code).collect();
        assert_eq!(code, ["a\n", "c\n", "d\n"]);
    }

    #[test]
    fn longer_fences_nest() {
        let src = "````{python}\n```\nx\n```\n````\n";
        assert_eq!(python_cells(src)[0].code, "```\nx\n```\n");
    }

    #[test]
    fn indented_fence_is_dedented() {
        let src = "  ```{python}\n  x = 1\n   y = 2\n  ```\n";
        assert_eq!(python_cells(src)[0].code, "x = 1\n y = 2\n");
    }

    #[test]
    fn unterminated_fence_runs_to_end_of_file() {
        let src = "```{python}\nx\n";
        assert_eq!(python_cells(src)[0].code, "x\n");
    }
}
