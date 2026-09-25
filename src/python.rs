//! Python code reduced to what the kernel runs, so that reformatting a cell
//! does not re-run it.

use std::hash::{Hash, Hasher};

use ruff_python_ast::PySourceType;
use ruff_python_ast::comparable::ComparableModModule;
use sha2::Digest;

/// Feeds `hasher` the syntax tree of `code`, as ruff compares two trees: not
/// the whitespace, comments, parentheses or quotes it was written with, but
/// still its indentation, the values of its strings and the text of its
/// IPython magics and shell escapes.
///
/// Code that does not parse is fed as written.
pub fn update(hasher: &mut impl Digest, code: &str) {
    let parsed = ruff_python_parser::parse_unchecked_source(code, PySourceType::Ipynb);
    if parsed.has_valid_syntax() {
        hasher.update(b"tree");
        ComparableModModule::from(parsed.syntax()).hash(&mut DigestHasher(hasher));
    } else {
        hasher.update(b"text");
        hasher.update(code.as_bytes());
    }
}

/// Lets a `Hash` implementation write into a digest. The derived
/// implementations prefix every sequence with its length and end every
/// string with a marker, so no two trees write the same bytes.
struct DigestHasher<'a, D>(&'a mut D);

impl<D: Digest> Hasher for DigestHasher<'_, D> {
    fn write(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    fn finish(&self) -> u64 {
        unreachable!("the digest is read from the hasher it writes into")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Sha256;

    fn hash(code: &str) -> Vec<u8> {
        let mut hasher = Sha256::new();
        update(&mut hasher, code);
        hasher.finalize().to_vec()
    }

    #[test]
    fn whitespace_and_comments_are_ignored() {
        assert_eq!(hash("a=f(1,2)\n"), hash("a = f( 1, 2 )  # call\n\n\n"));
        assert_eq!(hash("x = [1,\n2]\n"), hash("x = [1, 2]\n"));
        assert_eq!(hash("x = 1 + \\\n 2\n"), hash("x = 1 + 2\n"));
    }

    #[test]
    fn quotes_and_parentheses_are_ignored() {
        assert_eq!(hash("x = 'a'\n"), hash("x = \"a\"\n"));
        assert_eq!(hash("x = (a + b)\n"), hash("x = a + b\n"));
    }

    #[test]
    fn indentation_is_not_ignored() {
        assert_ne!(hash("if a:\n    b\n    c\n"), hash("if a:\n    b\nc\n"));
    }

    #[test]
    fn whitespace_inside_strings_is_not_ignored() {
        assert_ne!(hash("print('a b')\n"), hash("print('a  b')\n"));
        assert_ne!(hash("print('a\\n b')\n"), hash("print('a\\nb')\n"));
        assert_ne!(hash("f'{x} {y}'\n"), hash("f'{x}{y}'\n"));
        assert_ne!(hash("'''a\n b'''\n"), hash("'''a\nb'''\n"));
        assert_ne!(hash("f'{x:>10}'\n"), hash("f'{x:<10}'\n"));
        assert_ne!(hash("f'{x: >10}'\n"), hash("f'{x:>10}'\n"));
        // A self-documenting expression prints as written.
        assert_ne!(hash("f'{x = }'\n"), hash("f'{x=}'\n"));
    }

    #[test]
    fn tokens_are_not_run_together() {
        assert_ne!(hash("ab\n"), hash("a b\n"));
        assert_ne!(hash("x = 1\n"), hash("x = 1\ny\n"));
    }

    #[test]
    fn magics_and_shell_escapes_keep_their_text() {
        assert_eq!(hash("%matplotlib inline\n"), hash("%matplotlib inline\n\n"));
        assert_ne!(hash("!ls -la\n"), hash("!ls - la\n"));
        assert_ne!(hash("%time f()\n"), hash("%timeit f()\n"));
    }

    #[test]
    fn code_that_does_not_parse_is_hashed_as_written() {
        assert_ne!(hash("def f(:\n"), hash("def f( :\n"));
    }
}
