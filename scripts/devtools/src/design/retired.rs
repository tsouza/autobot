//! The retired-term check behind `just check-retired-terms`.
//!
//! The list of retired identifiers is read at run time from GLOSSARY §Retired terms; there is
//! no other copy of it. A retired identifier is a code span in the bold label of an entry of
//! that section whose text is a single identifier (letters, digits and `_`), unless the same
//! identifier occurs inside a code span elsewhere in the glossary: a label may name a live
//! term to say how the retired form misused it (`DISPATCHING` without `send_attempt`), and a
//! term the glossary defines is live. Spans that are not a single identifier
//! (`Healthy … Fenced`) name a family by example and are not checked.
//!
//! The check fails when a retired identifier occurs, as a whole identifier and with its exact
//! case, in any of these files under the repository root:
//!
//! - every file under `docs/design/`, with the body of GLOSSARY §Retired terms left out;
//! - every `*.rs` file under `crates/`;
//! - every file under `formal/` and `deploy/`.
//!
//! Directories that do not exist are skipped, and so is any directory named `target`. Files
//! are read as UTF-8 with invalid bytes replaced.

use crate::design::check::GLOSSARY;
use crate::design::{self, is_word_char, offset_in};
use crate::{Error, Result, markdown};
use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;
use std::process::ExitCode;

/// The design directory, relative to the repository root.
pub const DESIGN_DIR: &str = "docs/design";

/// The heading of the GLOSSARY section that lists the retired terms.
pub const SECTION: &str = "Retired terms";

/// One occurrence of a retired identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Hit {
    /// The file, relative to the repository root, with `/` separators.
    pub file: String,
    /// The 1-based line.
    pub line: usize,
    /// The retired identifier found.
    pub identifier: String,
}

impl fmt::Display for Hit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: `{}` is retired (GLOSSARY §{SECTION})",
            self.file, self.line, self.identifier
        )
    }
}

/// Checks the repository at `root`, prints every hit or a clean line, and returns the exit code.
///
/// # Errors
/// Fails if the glossary or its §Retired terms is missing, the section yields no identifier,
/// or a directory or file cannot be read.
pub fn run(root: &Path) -> Result<ExitCode> {
    let (retired, hits) = check(root)?;
    if hits.is_empty() {
        println!("retired terms: none in use ({} checked)", retired.len());
        return Ok(ExitCode::SUCCESS);
    }
    for h in &hits {
        println!("{h}");
    }
    println!("retired terms: {} occurrence(s)", hits.len());
    Ok(ExitCode::FAILURE)
}

/// The retired identifiers of the glossary at `root`, and every hit, sorted by file and line.
///
/// # Errors
/// As [`run`].
pub fn check(root: &Path) -> Result<(BTreeSet<String>, Vec<Hit>)> {
    let glossary_rel = format!("{DESIGN_DIR}/{GLOSSARY}");
    let glossary_path = root.join(&glossary_rel);
    let glossary = std::fs::read_to_string(&glossary_path)
        .map_err(|e| Error::Parse(format!("reading {}: {e}", glossary_path.display())))?;
    let retired = retired_identifiers(&glossary)?;

    let mut files = Vec::new();
    for (dir, only_rs) in [
        (DESIGN_DIR, false),
        ("crates", true),
        ("formal", false),
        ("deploy", false),
    ] {
        let dir = root.join(dir);
        if dir.is_dir() {
            files.extend(
                design::walk(root, &dir)?
                    .into_iter()
                    .filter(|(rel, _)| !only_rs || rel.ends_with(".rs")),
            );
        }
    }

    let mut hits = Vec::new();
    for (rel, path) in files {
        let bytes = std::fs::read(&path)
            .map_err(|e| Error::Parse(format!("reading {}: {e}", path.display())))?;
        let mut text = String::from_utf8_lossy(&bytes).into_owned();
        if rel == glossary_rel {
            text = without_section(&text);
        }
        hits.extend(scan(&rel, &text, &retired));
    }
    hits.sort();
    Ok((retired, hits))
}

/// The retired identifiers listed in `glossary`'s §Retired terms (see the module docs).
///
/// # Errors
/// Fails if the section is missing or yields no identifier.
pub fn retired_identifiers(glossary: &str) -> Result<BTreeSet<String>> {
    let section = markdown::section(glossary, SECTION)
        .ok_or_else(|| Error::Parse(format!("{GLOSSARY} has no §{SECTION}")))?;
    let rest = without_section(glossary);
    let live: BTreeSet<&str> = code_spans(&rest)
        .into_iter()
        .flat_map(identifiers)
        .collect();
    let retired: BTreeSet<String> = section
        .lines()
        .filter_map(|l| l.trim_start().strip_prefix("- **")?.split_once("**"))
        .flat_map(|(label, _)| code_spans(label))
        .filter(|s| is_identifier(s) && !live.contains(s))
        .map(str::to_owned)
        .collect();
    if retired.is_empty() {
        return Err(Error::Parse(format!(
            "{GLOSSARY} §{SECTION} lists no retired identifier"
        )));
    }
    Ok(retired)
}

/// Every occurrence in `text` of an identifier from `retired`, as a whole identifier.
#[must_use]
pub fn scan(file: &str, text: &str, retired: &BTreeSet<String>) -> Vec<Hit> {
    let mut hits = Vec::new();
    for (n, line) in text.lines().enumerate() {
        for word in identifiers(line) {
            if retired.contains(word) {
                hits.push(Hit {
                    file: file.to_owned(),
                    line: n + 1,
                    identifier: word.to_owned(),
                });
            }
        }
    }
    hits
}

/// `glossary` with the body of §Retired terms replaced by as many empty lines, so that line
/// numbers are kept.
fn without_section(glossary: &str) -> String {
    let Some(body) = markdown::section(glossary, SECTION) else {
        return glossary.to_owned();
    };
    let start = offset_in(glossary, body);
    let end = start + body.len();
    let blank = "\n".repeat(body.matches('\n').count());
    format!("{}{blank}{}", &glossary[..start], &glossary[end..])
}

/// The contents of the inline code spans of `text` (single backticks, no nesting).
fn code_spans(text: &str) -> Vec<&str> {
    text.split('`').skip(1).step_by(2).collect()
}

fn is_identifier(s: &str) -> bool {
    !s.is_empty() && s.chars().all(is_word_char)
}

/// The maximal runs of word characters in `text`.
fn identifiers(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !is_word_char(c))
        .filter(|w| !w.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

    const GLOSSARY_TEXT: &str = "\
# Glossary

## CORE

- **`WorkContext`** — The aggregate; its phase is `phase ∈ {ACTIVE, DRAINING}`. *(KERNEL §3)*
- **`RecordSendAttempt`** — Operation to `DISPATCHING` with `send_attempt`. *(KERNEL §3)*

## Retired terms

- **`OldReceipt`** — subsumed by the `CommandReceipt`.
- **`old_register` register, `stale_revision`** — a pin.
- **`DISPATCHING` without `send_attempt`; `GONE_STATE`** — an operation enters `DISPATCHING` only with `send_attempt`.
- **`DRAINING` hold state** — a Manager phase.
- **Modes (`Healthy … Fenced`) as states** — conditions.
- **Plain words as a role** — nothing to check.
";

    fn set(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|s| (*s).to_owned()).collect()
    }

    /// A scratch repository root under the system temporary directory, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("autobot-retired-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn write(&self, rel: &str, text: &str) {
            let path = self.0.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn identifiers_come_from_labels_minus_live_terms() {
        assert_eq!(
            retired_identifiers(GLOSSARY_TEXT).unwrap(),
            set(&["GONE_STATE", "OldReceipt", "old_register", "stale_revision"])
        );
    }

    #[test]
    fn missing_or_empty_section_is_an_error() {
        let err = retired_identifiers("# Glossary\n\n## CORE\n").unwrap_err();
        assert!(err.to_string().contains("has no §Retired terms"), "{err}");
        let err = retired_identifiers("## Retired terms\n\n- **Plain** — none.\n").unwrap_err();
        assert!(
            err.to_string().contains("lists no retired identifier"),
            "{err}"
        );
    }

    #[test]
    fn scan_matches_whole_identifiers_with_exact_case() {
        let retired = set(&["OldReceipt"]);
        let text =
            "fn a(r: OldReceipt) {}\nOldReceipts oldreceipt MyOldReceipt\nx.OldReceipt::new()\n";
        let hits = scan("f.rs", text, &retired);
        assert_eq!(
            hits.iter().map(|h| h.line).collect::<Vec<_>>(),
            [1, 3],
            "{hits:#?}"
        );
        assert_eq!(
            hits[0].to_string(),
            "f.rs:1: `OldReceipt` is retired (GLOSSARY §Retired terms)"
        );
    }

    #[test]
    fn imported_set_passes() {
        let (retired, hits) = check(Path::new(ROOT)).unwrap();
        assert!(!retired.is_empty());
        assert_eq!(hits, []);
    }

    #[test]
    fn planted_identifier_in_a_rust_file_fails() {
        let real =
            std::fs::read_to_string(Path::new(ROOT).join(DESIGN_DIR).join(GLOSSARY)).unwrap();
        let retired = retired_identifiers(&real).unwrap();
        let planted = retired.first().unwrap();
        let scratch = Scratch::new("planted");
        scratch.write(&format!("{DESIGN_DIR}/{GLOSSARY}"), &real);
        scratch.write(
            "crates/autobot-x/src/lib.rs",
            &format!("//! Fixture.\n\npub struct {planted};\n"),
        );
        scratch.write("crates/autobot-x/README.md", &format!("{planted}\n"));
        scratch.write(
            "crates/autobot-x/target/debug/gen.rs",
            &format!("{planted}\n"),
        );
        let (_, hits) = check(&scratch.0).unwrap();
        assert_eq!(
            hits,
            [Hit {
                file: "crates/autobot-x/src/lib.rs".to_owned(),
                line: 3,
                identifier: planted.clone(),
            }]
        );
    }

    #[test]
    fn every_scanned_tree_is_checked_and_the_section_itself_is_not() {
        let scratch = Scratch::new("trees");
        let glossary = format!("{GLOSSARY_TEXT}\n## After\n\nUses GONE_STATE in prose.\n");
        scratch.write(&format!("{DESIGN_DIR}/{GLOSSARY}"), &glossary);
        scratch.write(
            &format!("{DESIGN_DIR}/extensions/x.md"),
            "The `OldReceipt`.\n",
        );
        scratch.write("formal/model.qnt", "val old_register = 1\n");
        scratch.write("deploy/crd.yaml", "kind: stale_revision\n");
        scratch.write("docs/other.md", "OldReceipt outside the design set\n");
        let (_, hits) = check(&scratch.0).unwrap();
        let found: Vec<(&str, usize, &str)> = hits
            .iter()
            .map(|h| (h.file.as_str(), h.line, h.identifier.as_str()))
            .collect();
        let glossary_line = glossary.lines().count();
        assert_eq!(
            found,
            [
                ("deploy/crd.yaml", 1, "stale_revision"),
                (
                    &*format!("{DESIGN_DIR}/{GLOSSARY}"),
                    glossary_line,
                    "GONE_STATE"
                ),
                (&*format!("{DESIGN_DIR}/extensions/x.md"), 1, "OldReceipt"),
                ("formal/model.qnt", 1, "old_register"),
            ]
        );
    }

    #[test]
    fn missing_glossary_is_an_error() {
        let scratch = Scratch::new("missing");
        let err = check(&scratch.0).unwrap_err();
        assert!(err.to_string().contains(GLOSSARY), "{err}");
    }
}
