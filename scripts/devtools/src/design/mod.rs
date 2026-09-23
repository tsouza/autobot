//! The design set under `docs/design`: its consistency check, the retired-term check, the traceability lint and the KERNEL §10 parser.
//!
//! This module also holds what the checks, and the formal toolchain, share: the [`Violation`]
//! they report, the walk over a directory tree, and the path, offset, line and identifier
//! helpers.

pub mod check;
pub mod lifecycle;
pub mod retired;
pub mod trace;

use crate::{Error, Result};
use std::fmt;
use std::path::{Path, PathBuf};

/// A rule of one design check, printed by name.
pub trait RuleName: Copy {
    /// The rule's name as printed.
    fn name(self) -> &'static str;
}

/// One broken rule `R` at one place, printed as `file:line: [rule] message`, or
/// `file: [rule] message` without a line.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Violation<R> {
    /// The file, relative to the directory the check reads.
    pub file: String,
    /// The 1-based line, when the violation has one.
    pub line: Option<usize>,
    /// The rule broken.
    pub rule: R,
    /// What is wrong.
    pub message: String,
}

impl<R> Violation<R> {
    /// A violation of `rule` in `file`, at `line` when given.
    #[must_use]
    pub fn new(file: &str, line: Option<usize>, rule: R, message: String) -> Self {
        Self {
            file: file.to_owned(),
            line,
            rule,
            message,
        }
    }
}

impl<R: RuleName> fmt::Display for Violation<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(l) => write!(f, "{}:{l}: ", self.file)?,
            None => write!(f, "{}: ", self.file)?,
        }
        write!(f, "[{}] {}", self.rule.name(), self.message)
    }
}

/// Every file under `dir`, recursively and skipping directories named `target`, as (path
/// relative to `root`, path), sorted.
///
/// # Errors
/// Fails if a directory, `dir` included, cannot be listed.
pub(crate) fn walk(root: &Path, dir: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let listing = |e: std::io::Error| Error::Parse(format!("listing {}: {e}", d.display()));
        for entry in std::fs::read_dir(&d).map_err(listing)? {
            let path = entry.map_err(listing)?.path();
            if path.is_dir() {
                if path.file_name().is_none_or(|n| n != "target") {
                    stack.push(path);
                }
            } else {
                out.push((relative(root, &path), path));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// `path` relative to `root`, with `/` separators.
pub(crate) fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// The byte offset in `doc` at which `part`, a subslice of `doc`, starts.
fn offset_in(doc: &str, part: &str) -> usize {
    part.as_ptr() as usize - doc.as_ptr() as usize
}

/// The 1-based line of `doc` holding byte `offset`.
fn line_of(doc: &str, offset: usize) -> usize {
    doc[..offset].matches('\n').count() + 1
}

/// Whether `c` belongs to an identifier: a letter, a digit or `_`.
pub(crate) fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum TestRule {
        Second,
        First,
    }

    impl RuleName for TestRule {
        fn name(self) -> &'static str {
            match self {
                Self::Second => "second",
                Self::First => "first",
            }
        }
    }

    #[test]
    fn violation_prints_file_line_rule_and_message() {
        let with = Violation::new("a/b.md", Some(3), TestRule::First, "bad".to_owned());
        let without = Violation::new("a/b.md", None, TestRule::Second, "worse".to_owned());
        assert_eq!(with.to_string(), "a/b.md:3: [first] bad");
        assert_eq!(without.to_string(), "a/b.md: [second] worse");
    }

    #[test]
    fn violations_sort_by_file_line_rule_order_then_message() {
        let v = |file: &str, line, rule, message: &str| {
            Violation::new(file, line, rule, message.to_owned())
        };
        let mut all = [
            v("b", Some(1), TestRule::Second, "x"),
            v("a", Some(2), TestRule::Second, "x"),
            v("a", Some(2), TestRule::First, "a"),
            v("a", Some(2), TestRule::Second, "a"),
            v("a", None, TestRule::First, "z"),
        ];
        all.sort();
        let printed: Vec<String> = all.iter().map(ToString::to_string).collect();
        assert_eq!(
            printed,
            [
                "a: [first] z",
                "a:2: [second] a",
                "a:2: [second] x",
                "a:2: [first] a",
                "b:1: [second] x",
            ]
        );
    }

    #[test]
    fn walk_lists_files_relative_to_root_sorted_without_target() {
        let root = std::env::temp_dir().join(format!("autobot-design-walk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for rel in [
            "d/z.md",
            "d/a/b.rs",
            "d/a/target/skip.rs",
            "d/target/skip.md",
            "d/targets/kept.md",
            "outside.md",
        ] {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, rel).unwrap();
        }
        let found = walk(&root, &root.join("d"));
        let missing = walk(&root, &root.join("none"));
        std::fs::remove_dir_all(&root).unwrap();
        let found = found.unwrap();
        let rels: Vec<&str> = found.iter().map(|(r, _)| r.as_str()).collect();
        assert_eq!(rels, ["d/a/b.rs", "d/targets/kept.md", "d/z.md"]);
        assert!(found.iter().all(|(r, p)| p == &root.join(r)));
        let Err(Error::Parse(e)) = missing else {
            panic!("walking a missing directory succeeded");
        };
        assert!(e.starts_with("listing "), "{e}");
    }

    #[test]
    fn relative_joins_components_with_slashes() {
        let root = Path::new("/r");
        assert_eq!(
            relative(root, &Path::new("/r").join("a").join("b.md")),
            "a/b.md"
        );
    }

    #[test]
    fn offsets_map_to_one_based_lines() {
        let doc = "one\ntwo\nthree\n";
        let part = &doc[8..13];
        assert_eq!(part, "three");
        assert_eq!(offset_in(doc, part), 8);
        assert_eq!(line_of(doc, 0), 1);
        assert_eq!(line_of(doc, 3), 1);
        assert_eq!(line_of(doc, 4), 2);
        assert_eq!(line_of(doc, offset_in(doc, part)), 3);
    }

    #[test]
    fn word_chars_are_letters_digits_and_underscore() {
        assert!("aZ9_é".chars().all(is_word_char));
        assert!(!"-. `§".chars().any(is_word_char));
    }
}
