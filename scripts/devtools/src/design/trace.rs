//! The traceability lint behind `just trace-lint`: I-n → F-n → fixture group → owning test →
//! model invariant.
//!
//! The lint reads the THESIS invariant table, FORMAL §4 (the F-n and the I-n each is grouped
//! under), FORMAL §5 (the negative variants and the F-n each must violate) and M0 §3 (the
//! fixture groups and the F-n each lists). A list `F-a … F-b` (or `F-a..F-b`) names every F-n
//! from `a` to `b`.
//!
//! **Stage 1 (design)** always runs: every I-n has an F-n, every I-n that §4 groups under is a
//! THESIS invariant, no F-n is defined twice, every F-n is listed by exactly one group, and
//! every group and variant names only known F-n.
//!
//! **Stage 2 (code)** runs for a group as soon as a fixture directory
//! `crates/*/tests/g_<group>/` exists, where `<group>` is the group's name without `G-`,
//! lowercased, with `-` as `_` (`G-CUSTODY-M0` is `g_custody_m0`). A test is a function with a
//! `#[test]` or `#[<path>::test]` attribute, read line by line from every `.rs` file under the
//! directory. In the group's directories:
//!
//! - every F-n of the group has exactly one test named `f<n>_<slug>`, its owning test;
//! - no test is named `f<n>_<slug>` for an F-n of another group: a test that exercises an F-n
//!   owned elsewhere carries a `/// exercises: F-n` doc line instead, which the lint reports
//!   and never counts as ownership;
//! - every ignored test carries exactly the line `#[ignore = "awaiting #N"]`;
//! - the directory's `mod.rs` starts with the line `//! fixture-task: #N`, naming the group's
//!   fixture task, and every directory of one group names the same fixture task.
//!
//! With `--closed G-X` the lint also fails while G-X has no fixture directory or any owning
//! test of G-X is ignored. With `--awaiting #N` it only lists the tests still awaiting #N.
//!
//! **The diff rule** (`--diff`) runs on a pull request: the event payload at
//! `GITHUB_EVENT_PATH` gives the base and head SHAs and the body, whose first `Closes #N`
//! names the issue the pull request closes. Against the merge base, a change under
//! `crates/*/tests/g_*/` may only delete `#[ignore = "awaiting #N"]` lines for that issue,
//! unless that issue is the fixture task of the directory's group, which may change anything
//! there. The group's fixture task is read from the `mod.rs` of its directories at the merge
//! base, or at the head when the group has no directory at the merge base, so a pull request
//! cannot name itself the fixture task of a group that already has one, neither by rewriting
//! `mod.rs` nor by adding another directory for the group.
//!
//! **Stage 3 (model)** runs for every Quint module `formal/<name>.qnt` whose first line is
//! `// owns: F-…`, the F-n that module owns: each owned F-n has exactly one invariant
//! (`val`, `def` or `temporal`) named `F<n>_<Name>` in that module, no F-n is owned by two
//! modules, and no module declares an `F<n>_` invariant for an F-n it does not own. An F-n
//! whose owning module does not exist yet is not checked. `formal/instances/` and
//! `formal/variants/` hold instances and mutated copies, not owning modules, and are not read.

use crate::design::check::M0;
use crate::process::Cmd;
use crate::{Error, Result, git, markdown};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;
use std::process::ExitCode;

/// The design set, relative to the repository root.
pub const DESIGN_DIR: &str = "docs/design";
/// The THESIS document, whose invariant table defines the I-n.
pub const THESIS: &str = "AUTOBOT-THESIS.md";
/// The FORMAL document, whose §4 defines the F-n and §5 the negative variants.
pub const FORMAL: &str = "AUTOBOT-FORMAL-SURFACE.md";
/// The heading of the THESIS invariant section.
pub const THESIS_SECTION: &str = "Invariants the core exists to make true";
/// The heading of FORMAL §4.
pub const PROPERTIES_SECTION: &str = "4. Safety invariants";
/// The heading of FORMAL §5.
pub const VARIANTS_SECTION: &str = "5. Negative variants";
/// The heading of M0 §3.
pub const GROUPS_SECTION: &str = "3. Fixture groups";
/// The directory of the Quint model, relative to the repository root.
pub const MODEL_DIR: &str = "formal";

/// The rule a violation breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rule {
    /// An I-n without an F-n, an unknown I-n, or an F-n defined twice.
    Invariant,
    /// An F-n not listed by exactly one group, or a group listing an unknown F-n.
    Group,
    /// A negative variant naming an unknown F-n or none.
    Variant,
    /// A missing, duplicated or misplaced owning test, or an unknown fixture directory.
    OwningTest,
    /// An ignored test without `#[ignore = "awaiting #N"]`, a `mod.rs` naming no fixture task, or
    /// directories of one group naming different fixture tasks.
    Ignore,
    /// A group asked to be closed that still has ignored owning tests.
    Closed,
    /// A pull request changing a fixture directory beyond what its issue may change.
    Diff,
    /// A missing, duplicated or misplaced model invariant.
    Model,
}

impl Rule {
    /// The rule's name as printed.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Invariant => "invariant",
            Self::Group => "group",
            Self::Variant => "variant",
            Self::OwningTest => "owning-test",
            Self::Ignore => "ignore",
            Self::Closed => "closed",
            Self::Diff => "diff",
            Self::Model => "model",
        }
    }
}

/// One broken rule at one place.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Violation {
    /// The file, relative to the repository root.
    pub file: String,
    /// The 1-based line, when the violation has one.
    pub line: Option<usize>,
    /// The rule broken.
    pub rule: Rule,
    /// What is wrong.
    pub message: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(l) => write!(f, "{}:{l}: ", self.file)?,
            None => write!(f, "{}: ", self.file)?,
        }
        write!(f, "[{}] {}", self.rule.name(), self.message)
    }
}

fn violation(file: &str, line: Option<usize>, rule: Rule, message: String) -> Violation {
    Violation {
        file: file.to_owned(),
        line,
        rule,
        message,
    }
}

/// Every `<prefix><n>` in `text` (such as `F-7` or `I-10`) in order, with `a … b` and `a..b`
/// between two of them expanded to every number from `a` to `b`.
#[must_use]
pub fn ids(text: &str, prefix: &str) -> Vec<u32> {
    let mut found: Vec<(usize, usize, u32)> = Vec::new();
    let mut from = 0;
    while let Some(i) = text[from..].find(prefix) {
        let at = from + i;
        let start = at + prefix.len();
        let digits = text[start..].bytes().take_while(u8::is_ascii_digit).count();
        let end = start + digits;
        let bounded = text[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_');
        if bounded && let Ok(n) = text[start..end].parse() {
            found.push((at, end, n));
        }
        from = end.max(start);
    }
    let mut out = Vec::new();
    for (k, &(at, _, n)) in found.iter().enumerate() {
        if let (Some(&last), Some(&(_, prev_end, _))) =
            (out.last(), k.checked_sub(1).map(|p| &found[p]))
            && matches!(text[prev_end..at].trim(), "…" | "..")
            && n > last
        {
            out.extend(last + 1..=n);
        } else {
            out.push(n);
        }
    }
    out
}

/// The lines of the section of `doc` headed `heading`, numbered 1-based within `doc`.
fn section_lines<'a>(doc: &'a str, heading: &str) -> Option<Vec<(usize, &'a str)>> {
    let body = markdown::section(doc, heading)?;
    let first = doc[..body.as_ptr() as usize - doc.as_ptr() as usize]
        .matches('\n')
        .count()
        + 1;
    Some(
        body.lines()
            .enumerate()
            .map(|(i, l)| (first + i, l))
            .collect(),
    )
}

fn missing(file: &str, heading: &str) -> Error {
    Error::Parse(format!("{file} has no section `{heading}`"))
}

/// An F-n as FORMAL §4 defines it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Property {
    /// The `n` of F-n.
    pub id: u32,
    /// The I-n it is grouped under, or `None` for a cross-cutting property.
    pub invariant: Option<u32>,
    /// The line of FORMAL defining it.
    pub line: usize,
}

/// A fixture group of M0 §3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// The name, such as `G-COMMIT`.
    pub name: String,
    /// The F-n it lists, in order.
    pub properties: Vec<u32>,
    /// The line of M0 defining it.
    pub line: usize,
}

impl Group {
    /// The name of the group's fixture directories: `G-CUSTODY-M0` is `g_custody_m0`.
    #[must_use]
    pub fn dir_name(&self) -> String {
        let bare = self.name.strip_prefix("G-").unwrap_or(&self.name);
        format!("g_{}", bare.to_ascii_lowercase().replace('-', "_"))
    }
}

/// A negative variant of FORMAL §5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    /// The guard removed, as written.
    pub guard: String,
    /// The F-n it must violate.
    pub properties: Vec<u32>,
    /// The line of FORMAL defining it.
    pub line: usize,
}

/// The traceability facts of the design set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Design {
    /// The I-n of the THESIS table, with their lines.
    pub invariants: Vec<(u32, usize)>,
    /// The F-n of FORMAL §4.
    pub properties: Vec<Property>,
    /// The groups of M0 §3.
    pub groups: Vec<Group>,
    /// The variants of FORMAL §5.
    pub variants: Vec<Variant>,
}

impl Design {
    /// Reads THESIS, FORMAL and M0 from the design set in `dir`.
    ///
    /// # Errors
    /// Fails if a document cannot be read or lacks a section the lint reads.
    pub fn load(dir: &Path) -> Result<Self> {
        let read = |name: &str| {
            std::fs::read_to_string(dir.join(name))
                .map_err(|e| Error::Parse(format!("reading {name}: {e}")))
        };
        Self::parse(&read(THESIS)?, &read(FORMAL)?, &read(M0)?)
    }

    /// Parses the three documents.
    ///
    /// # Errors
    /// Fails if a document lacks a section the lint reads.
    pub fn parse(thesis: &str, formal: &str, m0: &str) -> Result<Self> {
        let rows =
            section_lines(thesis, THESIS_SECTION).ok_or_else(|| missing(THESIS, THESIS_SECTION))?;
        let invariants = rows
            .into_iter()
            .filter_map(|(n, l)| {
                let first = l.trim().strip_prefix('|')?.split('|').next()?;
                ids(first, "I-").first().map(|&i| (i, n))
            })
            .collect();

        let mut properties = Vec::new();
        let mut current = None;
        for (n, l) in section_lines(formal, PROPERTIES_SECTION)
            .ok_or_else(|| missing(FORMAL, PROPERTIES_SECTION))?
        {
            if let Some(rest) = l.strip_prefix("**") {
                let bold = rest.split("**").next().unwrap_or("");
                current = ids(bold, "I-").first().copied();
            }
            for id in ids(l, "F-") {
                properties.push(Property {
                    id,
                    invariant: current,
                    line: n,
                });
            }
        }

        let groups = section_lines(m0, GROUPS_SECTION)
            .ok_or_else(|| missing(M0, GROUPS_SECTION))?
            .into_iter()
            .filter_map(|(n, l)| {
                let rest = l.strip_prefix("- **")?;
                let (name, after) = rest.split_once("**")?;
                if !name.starts_with("G-") {
                    return None;
                }
                let list = after.trim_start().strip_prefix('(')?.split(')').next()?;
                Some(Group {
                    name: name.to_owned(),
                    properties: ids(list, "F-"),
                    line: n,
                })
            })
            .collect();

        let variants = section_lines(formal, VARIANTS_SECTION)
            .ok_or_else(|| missing(FORMAL, VARIANTS_SECTION))?
            .into_iter()
            .filter_map(|(n, l)| {
                let row = l.trim().strip_prefix('|')?.strip_suffix('|')?;
                let (guard, last) = row.rsplit_once('|')?;
                let delimiter = row.bytes().all(|b| matches!(b, b'|' | b'-' | b':' | b' '));
                if delimiter || last.trim() == "Must violate" {
                    return None;
                }
                Some(Variant {
                    guard: guard.trim().to_owned(),
                    properties: ids(last, "F-"),
                    line: n,
                })
            })
            .collect();

        Ok(Self {
            invariants,
            properties,
            groups,
            variants,
        })
    }

    fn known(&self, f: u32) -> bool {
        self.properties.iter().any(|p| p.id == f)
    }

    /// The group listing F-`f` first, if any.
    #[must_use]
    pub fn group_of(&self, f: u32) -> Option<&Group> {
        self.groups.iter().find(|g| g.properties.contains(&f))
    }

    fn group(&self, name: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.name == name)
    }
}

fn design_file(name: &str) -> String {
    format!("{DESIGN_DIR}/{name}")
}

/// Stage 1: the violations of the design itself.
#[must_use]
pub fn stage1(d: &Design) -> Vec<Violation> {
    let (thesis, formal, m0) = (design_file(THESIS), design_file(FORMAL), design_file(M0));
    let mut out = Vec::new();
    for &(i, line) in &d.invariants {
        if !d.properties.iter().any(|p| p.invariant == Some(i)) {
            out.push(violation(
                &thesis,
                Some(line),
                Rule::Invariant,
                format!("I-{i} has no F-n in FORMAL §4"),
            ));
        }
    }
    let mut seen = BTreeSet::new();
    for p in &d.properties {
        if let Some(i) = p.invariant
            && !d.invariants.iter().any(|&(k, _)| k == i)
        {
            out.push(violation(
                &formal,
                Some(p.line),
                Rule::Invariant,
                format!(
                    "F-{} is grouped under I-{i}, which THESIS does not define",
                    p.id
                ),
            ));
        }
        if !seen.insert(p.id) {
            out.push(violation(
                &formal,
                Some(p.line),
                Rule::Invariant,
                format!("F-{} is defined twice", p.id),
            ));
        }
    }
    for &f in &seen {
        let listing: Vec<&str> = d
            .groups
            .iter()
            .flat_map(|g| {
                g.properties
                    .iter()
                    .filter(|&&x| x == f)
                    .map(|_| g.name.as_str())
            })
            .collect();
        if listing.len() != 1 {
            let line = d.properties.iter().find(|p| p.id == f).map(|p| p.line);
            let by = if listing.is_empty() {
                "no fixture group".to_owned()
            } else {
                listing.join(", ")
            };
            out.push(violation(
                &formal,
                line,
                Rule::Group,
                format!("F-{f} is listed by {by}; it needs exactly one"),
            ));
        }
    }
    for g in &d.groups {
        for &f in g.properties.iter().filter(|&&f| !d.known(f)) {
            out.push(violation(
                &m0,
                Some(g.line),
                Rule::Group,
                format!("{} lists F-{f}, which FORMAL §4 does not define", g.name),
            ));
        }
    }
    for v in &d.variants {
        if v.properties.is_empty() {
            out.push(violation(
                &formal,
                Some(v.line),
                Rule::Variant,
                format!("the variant `{}` names no F-n", v.guard),
            ));
        }
        for &f in v.properties.iter().filter(|&&f| !d.known(f)) {
            out.push(violation(
                &formal,
                Some(v.line),
                Rule::Variant,
                format!(
                    "the variant `{}` names F-{f}, which FORMAL §4 does not define",
                    v.guard
                ),
            ));
        }
    }
    out.sort();
    out
}

/// The attribute line that marks a test as awaiting issue `n`.
#[must_use]
pub fn awaiting_line(n: u64) -> String {
    format!("#[ignore = \"awaiting #{n}\"]")
}

/// The issue an exact `#[ignore = "awaiting #N"]` line (surrounding space aside) awaits.
#[must_use]
pub fn awaited(line: &str) -> Option<u64> {
    let n = line
        .trim()
        .strip_prefix("#[ignore = \"awaiting #")?
        .strip_suffix("\"]")?;
    if n.bytes().all(|b| b.is_ascii_digit()) {
        n.parse().ok()
    } else {
        None
    }
}

/// The fixture task a group's `mod.rs` names on its first line, `//! fixture-task: #N`.
#[must_use]
pub fn fixture_task(mod_rs: &str) -> Option<u64> {
    let n = mod_rs
        .lines()
        .next()?
        .trim()
        .strip_prefix("//! fixture-task: #")?;
    if n.bytes().all(|b| b.is_ascii_digit()) {
        n.parse().ok()
    } else {
        None
    }
}

/// How a test is ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ignore {
    /// `#[ignore = "awaiting #N"]`.
    Awaiting(u64),
    /// Any other `ignore` attribute, as written.
    Other(String),
}

/// A test function found in a fixture directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestFn {
    /// The file, relative to the repository root.
    pub file: String,
    /// The 1-based line of its `fn`.
    pub line: usize,
    /// The function name.
    pub name: String,
    /// Its `ignore` attribute, if any.
    pub ignore: Option<Ignore>,
    /// The F-n its `/// exercises:` doc lines name.
    pub exercises: Vec<u32>,
}

impl TestFn {
    /// The F-n the test owns by its name `f<n>_<slug>`, if it is named so.
    #[must_use]
    pub fn owns(&self) -> Option<u32> {
        let rest = self.name.strip_prefix('f')?;
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let n: u32 = rest[..digits].parse().ok()?;
        let slug = rest[digits..].strip_prefix('_')?;
        (!slug.is_empty() && rest[..digits] == n.to_string()).then_some(n)
    }

    fn at(&self) -> String {
        format!("{}:{} `{}`", self.file, self.line, self.name)
    }
}

/// The inner text of an attribute, `#[x = 1]` giving `x = 1`.
fn attr_inner(attr: &str) -> &str {
    attr.trim()
        .strip_prefix("#[")
        .and_then(|a| a.strip_suffix(']'))
        .unwrap_or("")
        .trim()
}

fn attr_path(attr: &str) -> &str {
    attr_inner(attr)
        .split(['(', '='])
        .next()
        .unwrap_or("")
        .trim()
}

/// The function name an item line declares, if it declares one.
fn fn_name(line: &str) -> Option<&str> {
    let mut rest = line.trim_start();
    loop {
        if let Some(r) = rest.strip_prefix("pub(") {
            rest = r.split_once(')')?.1.trim_start();
        } else if let Some(r) = ["pub ", "async ", "const ", "unsafe "]
            .iter()
            .find_map(|p| rest.strip_prefix(p))
        {
            rest = r.trim_start();
        } else {
            break;
        }
    }
    let rest = rest.strip_prefix("fn ")?.trim_start();
    let len = rest
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(rest.len());
    (len > 0).then(|| &rest[..len])
}

/// Every test function in one Rust source file, read line by line: the attributes and doc
/// lines directly above a `fn` (comments and blank lines between them allowed) belong to it.
#[must_use]
pub fn tests_in(file: &str, text: &str) -> Vec<TestFn> {
    let mut out = Vec::new();
    let mut attrs: Vec<String> = Vec::new();
    let mut docs: Vec<&str> = Vec::new();
    let mut open: Option<(String, i64)> = None;
    let depth = |s: &str| {
        let count = |c| i64::try_from(s.matches(c).count()).unwrap_or(i64::MAX);
        count('[') - count(']')
    };
    for (n, raw) in text.lines().enumerate() {
        let t = raw.trim();
        if let Some((mut buf, d)) = open.take() {
            buf.push(' ');
            buf.push_str(t);
            let d = d + depth(t);
            if d <= 0 {
                attrs.push(buf);
            } else {
                open = Some((buf, d));
            }
            continue;
        }
        if t.starts_with("#[") {
            let d = depth(t);
            if d <= 0 {
                attrs.push(t.to_owned());
            } else {
                open = Some((t.to_owned(), d));
            }
            continue;
        }
        if let Some(doc) = t.strip_prefix("///") {
            docs.push(doc.trim());
            continue;
        }
        if t.is_empty() || t.starts_with("//") {
            continue;
        }
        if let Some(name) = fn_name(t)
            && attrs.iter().any(|a| {
                let p = attr_path(a);
                p == "test" || p.ends_with("::test")
            })
        {
            let ignore = attrs
                .iter()
                .find(|a| attr_path(a) == "ignore")
                .map(|a| awaited(a).map_or_else(|| Ignore::Other(a.clone()), Ignore::Awaiting));
            let exercises = docs
                .iter()
                .filter_map(|d| d.strip_prefix("exercises:"))
                .flat_map(|d| ids(d, "F-"))
                .collect();
            out.push(TestFn {
                file: file.to_owned(),
                line: n + 1,
                name: name.to_owned(),
                ignore,
                exercises,
            });
        }
        attrs.clear();
        docs.clear();
    }
    out
}

/// A fixture directory `crates/<crate>/tests/g_<group>/` and its Rust sources.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FixtureDir {
    /// The directory, relative to the repository root, without a trailing `/`.
    pub path: String,
    /// Every `.rs` file under it: path relative to the repository root, and contents.
    pub files: BTreeMap<String, String>,
}

impl FixtureDir {
    /// The directory's own name, such as `g_commit`.
    #[must_use]
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// Every test function in the directory.
    #[must_use]
    pub fn tests(&self) -> Vec<TestFn> {
        self.files
            .iter()
            .flat_map(|(f, t)| tests_in(f, t))
            .collect()
    }

    /// The fixture task the directory's `mod.rs` names, if it names one.
    #[must_use]
    pub fn fixture_task(&self) -> Option<u64> {
        self.files.get(&self.mod_rs()).and_then(|t| fixture_task(t))
    }

    fn mod_rs(&self) -> String {
        format!("{}/mod.rs", self.path)
    }
}

fn io_error(path: &Path, e: &std::io::Error) -> Error {
    Error::Parse(format!("reading {}: {e}", path.display()))
}

/// The entries of `dir` sorted by name, or none when it does not exist.
fn entries(dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| io_error(dir, &e))? {
        out.push(entry.map_err(|e| io_error(dir, &e))?.path());
    }
    out.sort();
    Ok(out)
}

/// Every file under `dir` with extension `ext`, recursively, as (path relative to `root`,
/// contents).
fn read_tree(root: &Path, dir: &Path, ext: &str, out: &mut BTreeMap<String, String>) -> Result<()> {
    for path in entries(dir)? {
        if path.is_dir() {
            read_tree(root, &path, ext, out)?;
        } else if path.extension().is_some_and(|e| e == ext) {
            let text = std::fs::read_to_string(&path).map_err(|e| io_error(&path, &e))?;
            out.insert(relative(root, &path), text);
        }
    }
    Ok(())
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Every fixture directory `crates/*/tests/g_*/` under the repository `root`.
///
/// # Errors
/// Fails if a directory cannot be listed or a source cannot be read as UTF-8.
pub fn fixture_dirs(root: &Path) -> Result<Vec<FixtureDir>> {
    let mut out = Vec::new();
    for krate in entries(&root.join("crates"))? {
        for dir in entries(&krate.join("tests"))? {
            let is_group = dir
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("g_"));
            if dir.is_dir() && is_group {
                let mut files = BTreeMap::new();
                read_tree(root, &dir, "rs", &mut files)?;
                out.push(FixtureDir {
                    path: relative(root, &dir),
                    files,
                });
            }
        }
    }
    Ok(out)
}

/// What stage 2 found: violations, and one line per active group and per `exercises` line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// The violations, sorted.
    pub violations: Vec<Violation>,
    /// Informational lines, in order.
    pub notes: Vec<String>,
}

/// Stage 2 over the fixture directories `dirs`; with `closed`, also the closure of that group.
#[must_use]
pub fn stage2(d: &Design, dirs: &[FixtureDir], closed: Option<&str>) -> Report {
    let mut r = Report::default();
    for dir in dirs {
        if !d.groups.iter().any(|g| g.dir_name() == dir.name()) {
            r.violations.push(violation(
                &dir.path,
                None,
                Rule::OwningTest,
                format!("`{}` names no M0 §3 fixture group", dir.name()),
            ));
        }
        if dir.fixture_task().is_none() {
            r.violations.push(violation(
                &dir.mod_rs(),
                Some(1),
                Rule::Ignore,
                "does not start with `//! fixture-task: #N`".to_owned(),
            ));
        }
    }
    for g in &d.groups {
        let mine: Vec<&FixtureDir> = dirs.iter().filter(|x| x.name() == g.dir_name()).collect();
        if mine.is_empty() {
            continue;
        }
        let tasks: BTreeSet<u64> = mine.iter().filter_map(|x| x.fixture_task()).collect();
        if tasks.len() > 1 {
            let named: Vec<String> = mine
                .iter()
                .filter_map(|x| Some(format!("{} #{}", x.path, x.fixture_task()?)))
                .collect();
            r.violations.push(violation(
                &mine[0].mod_rs(),
                Some(1),
                Rule::Ignore,
                format!(
                    "{}: its directories name different fixture tasks: {}",
                    g.name,
                    named.join(", ")
                ),
            ));
        }
        let tests: Vec<TestFn> = mine.iter().flat_map(|x| x.tests()).collect();
        let place = mine[0].path.as_str();
        for &f in &g.properties {
            let owners: Vec<&TestFn> = tests.iter().filter(|t| t.owns() == Some(f)).collect();
            match owners.as_slice() {
                [_] => {}
                [] => r.violations.push(violation(
                    place,
                    None,
                    Rule::OwningTest,
                    format!("{}: F-{f} has no owning test `f{f}_<slug>`", g.name),
                )),
                [first, rest @ ..] => {
                    for t in rest {
                        r.violations.push(violation(
                            &t.file,
                            Some(t.line),
                            Rule::OwningTest,
                            format!(
                                "{}: `{}` is a second owning test of F-{f}, after {}",
                                g.name,
                                t.name,
                                first.at()
                            ),
                        ));
                    }
                }
            }
        }
        let mut ignored = 0;
        for t in &tests {
            if let Some(f) = t.owns().filter(|f| !g.properties.contains(f)) {
                let whose = d.group_of(f).map_or_else(
                    || "which FORMAL §4 does not define or no group lists".to_owned(),
                    |o| {
                        format!(
                            "which {} owns; rename it and mark it `/// exercises: F-{f}`",
                            o.name
                        )
                    },
                );
                r.violations.push(violation(
                    &t.file,
                    Some(t.line),
                    Rule::OwningTest,
                    format!("`{}` is in {} but names F-{f}, {whose}", t.name, g.name),
                ));
            }
            match &t.ignore {
                Some(Ignore::Other(attr)) => r.violations.push(violation(
                    &t.file,
                    Some(t.line),
                    Rule::Ignore,
                    format!(
                        "`{}` is ignored by `{attr}`, not by `#[ignore = \"awaiting #N\"]`",
                        t.name
                    ),
                )),
                Some(Ignore::Awaiting(_)) => ignored += 1,
                None => {}
            }
            for &f in &t.exercises {
                match d.group_of(f) {
                    Some(o) => r.notes.push(format!(
                        "exercises: {} exercises F-{f} (owned by {})",
                        t.at(),
                        o.name
                    )),
                    None => r.violations.push(violation(
                        &t.file,
                        Some(t.line),
                        Rule::OwningTest,
                        format!("`{}` exercises F-{f}, which no fixture group lists", t.name),
                    )),
                }
            }
        }
        let owning = tests.iter().filter(|t| t.owns().is_some()).count();
        let paths: Vec<&str> = mine.iter().map(|x| x.path.as_str()).collect();
        r.notes.push(format!(
            "stage 2: {}: {owning} owning test(s), {ignored} ignored, in {}",
            g.name,
            paths.join(", ")
        ));
    }
    if let Some(name) = closed {
        close(d, dirs, name, &mut r.violations);
    }
    r.violations.sort();
    r
}

fn close(d: &Design, dirs: &[FixtureDir], name: &str, out: &mut Vec<Violation>) {
    let m0 = design_file(M0);
    let Some(g) = d.group(name) else {
        out.push(violation(
            &m0,
            None,
            Rule::Closed,
            format!("`{name}` is not an M0 §3 fixture group"),
        ));
        return;
    };
    let mine: Vec<&FixtureDir> = dirs.iter().filter(|x| x.name() == g.dir_name()).collect();
    if mine.is_empty() {
        out.push(violation(
            &m0,
            Some(g.line),
            Rule::Closed,
            format!(
                "{name} is not closed: no fixture directory `crates/*/tests/{}/`",
                g.dir_name()
            ),
        ));
    }
    for t in mine.iter().flat_map(|x| x.tests()) {
        if t.ignore.is_some() && t.owns().is_some_and(|f| g.properties.contains(&f)) {
            out.push(violation(
                &t.file,
                Some(t.line),
                Rule::Closed,
                format!("{name} is not closed: `{}` is still ignored", t.name),
            ));
        }
    }
}

/// Every test in `dirs` still marked `#[ignore = "awaiting #<issue>"]`.
#[must_use]
pub fn awaiting(dirs: &[FixtureDir], issue: u64) -> Vec<TestFn> {
    dirs.iter()
        .flat_map(FixtureDir::tests)
        .filter(|t| t.ignore == Some(Ignore::Awaiting(issue)))
        .collect()
}

/// One file of a unified diff, with its changed lines.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileChange {
    /// The path, the new one unless the file was deleted.
    pub path: String,
    /// Added lines: 1-based line in the new file, text.
    pub added: Vec<(usize, String)>,
    /// Removed lines: 1-based line in the old file, text.
    pub removed: Vec<(usize, String)>,
    /// Whether git reported a binary change.
    pub binary: bool,
}

/// Parses `git diff` output (any context size, renames off) into per-file changes.
#[must_use]
pub fn parse_diff(diff: &str) -> Vec<FileChange> {
    let mut out: Vec<FileChange> = Vec::new();
    let mut header = false;
    let (mut old, mut new) = (0usize, 0usize);
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let path = rest.rsplit_once(" b/").map_or(rest, |(_, p)| p);
            out.push(FileChange {
                path: path.to_owned(),
                ..FileChange::default()
            });
            header = true;
            continue;
        }
        let Some(file) = out.last_mut() else {
            continue;
        };
        if header {
            if let Some(p) = line.strip_prefix("+++ b/") {
                p.clone_into(&mut file.path);
            } else if line.starts_with("Binary files ") || line == "GIT binary patch" {
                file.binary = true;
            }
            if !line.starts_with("@@ ") {
                continue;
            }
        }
        if let Some(range) = line.strip_prefix("@@ ") {
            header = false;
            let start = |sign: char| {
                range
                    .split_whitespace()
                    .find_map(|r| r.strip_prefix(sign))
                    .and_then(|r| r.split(',').next())
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0)
            };
            (old, new) = (start('-'), start('+'));
        } else if let Some(t) = line.strip_prefix('+') {
            file.added.push((new, t.to_owned()));
            new += 1;
        } else if let Some(t) = line.strip_prefix('-') {
            file.removed.push((old, t.to_owned()));
            old += 1;
        } else if line.starts_with(' ') {
            old += 1;
            new += 1;
        }
    }
    out
}

/// The fixture directory `crates/<crate>/tests/g_<group>` a path lies under, if any.
#[must_use]
pub fn fixture_dir_of(path: &str) -> Option<String> {
    let parts: Vec<&str> = path.split('/').collect();
    match parts.as_slice() {
        ["crates", _, "tests", g, _, ..] if g.starts_with("g_") => Some(parts[..4].join("/")),
        _ => None,
    }
}

/// The diff rule over a unified diff: `closes` is the issue the pull request closes, and
/// `fixture_task` gives the fixture task of the group a fixture directory belongs to.
#[must_use]
pub fn diff_rule(
    diff: &str,
    closes: Option<u64>,
    fixture_task: impl Fn(&str) -> Option<u64>,
) -> Vec<Violation> {
    let mut out = Vec::new();
    let who = closes.map_or_else(
        || "a pull request closing no issue".to_owned(),
        |n| format!("#{n}"),
    );
    for file in parse_diff(diff) {
        let Some(dir) = fixture_dir_of(&file.path) else {
            continue;
        };
        if closes.is_some() && closes == fixture_task(&dir) {
            continue;
        }
        let why = format!("{who} is not the fixture task of {dir}");
        if file.binary {
            out.push(violation(
                &file.path,
                None,
                Rule::Diff,
                format!("changes a binary file; {why}"),
            ));
        }
        for (n, text) in &file.added {
            out.push(violation(
                &file.path,
                Some(*n),
                Rule::Diff,
                format!("adds `{}`; {why}", text.trim()),
            ));
        }
        for (n, text) in &file.removed {
            if closes.is_none() || awaited(text) != closes {
                let may = closes.map_or_else(
                    || "may change nothing here".to_owned(),
                    |c| format!("may only delete `{}` lines here", awaiting_line(c)),
                );
                out.push(violation(
                    &file.path,
                    Some(*n),
                    Rule::Diff,
                    format!("deletes `{}`; {who} {may}", text.trim()),
                ));
            }
        }
    }
    out.sort();
    out
}

/// The issue a pull request body closes: the number after its first `Closes #`, any case.
#[must_use]
pub fn closes_issue(body: &str) -> Option<u64> {
    let lower = body.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find("closes #") {
        let at = from + i;
        let start = at + "closes #".len();
        let bounded = lower[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        let digits = lower[start..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        if bounded && let Ok(n) = lower[start..start + digits].parse() {
            return Some(n);
        }
        from = start;
    }
    None
}

/// The diff rule for the pull request in the event payload `event`, in the repository `root`.
///
/// Returns `None` when the payload is not a pull request event.
///
/// # Errors
/// Fails if the payload is not JSON or git fails.
pub fn pull_request(root: &Path, event: &str) -> Result<Option<Vec<Violation>>> {
    let value: serde_json::Value =
        serde_json::from_str(event).map_err(|e| Error::Parse(format!("event payload: {e}")))?;
    let pr = &value["pull_request"];
    if pr.is_null() {
        return Ok(None);
    }
    let sha = |side: &str| {
        pr[side]["sha"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| Error::Parse(format!("event payload without pull_request.{side}.sha")))
    };
    let (base, head) = (sha("base")?, sha("head")?);
    let closes = closes_issue(pr["body"].as_str().unwrap_or(""));
    let merge_base = git::merge_base(root, &base, &head)?;
    let diff = Cmd::new("git")
        .args(["diff", "--no-color", "--no-ext-diff", "--no-renames", "-U0"])
        .args([merge_base.as_str(), head.as_str()])
        .current_dir(root)
        .output()?;
    let trees = [
        (merge_base.as_str(), tree(root, &merge_base)?),
        (head.as_str(), tree(root, &head)?),
    ];
    let show = |rev: &str, path: &str| {
        Cmd::new("git")
            .args(["show", &format!("{rev}:{path}")])
            .current_dir(root)
            .output()
            .ok()
    };
    Ok(Some(diff_rule(&diff, closes, |dir| {
        let name = dir.rsplit('/').next().unwrap_or(dir);
        trees
            .iter()
            .find_map(|(rev, paths)| group_task(paths, name, |p| show(rev, p)))
            .flatten()
    })))
}

/// Every file path under `crates/` at `rev`.
fn tree(root: &Path, rev: &str) -> Result<Vec<String>> {
    let out = Cmd::new("git")
        .args(["ls-tree", "-r", "--name-only", rev, "--", "crates"])
        .current_dir(root)
        .output()?;
    Ok(out.lines().map(str::to_owned).collect())
}

/// The fixture task of the group whose directories are named `name` (such as `g_dispatch`),
/// among the file `paths` of one revision, with `read` giving a file's contents: `None` when
/// the group has no directory among `paths`, and `Some(None)` when its directories do not all
/// name one and the same fixture task.
fn group_task(
    paths: &[String],
    name: &str,
    read: impl Fn(&str) -> Option<String>,
) -> Option<Option<u64>> {
    let dirs: BTreeSet<String> = paths
        .iter()
        .filter_map(|p| fixture_dir_of(p))
        .filter(|d| d.rsplit('/').next() == Some(name))
        .collect();
    if dirs.is_empty() {
        return None;
    }
    let tasks: BTreeSet<Option<u64>> = dirs
        .iter()
        .map(|d| {
            read(&format!("{d}/mod.rs"))
                .as_deref()
                .and_then(fixture_task)
        })
        .collect();
    Some(match tasks.into_iter().collect::<Vec<_>>().as_slice() {
        [Some(n)] => Some(*n),
        _ => None,
    })
}

/// A Quint module under [`MODEL_DIR`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// The file, relative to the repository root.
    pub path: String,
    /// Its contents.
    pub text: String,
}

/// Every Quint module `formal/*.qnt` directly under the repository `root`.
///
/// # Errors
/// Fails if the directory cannot be listed or a module cannot be read as UTF-8.
pub fn modules(root: &Path) -> Result<Vec<Module>> {
    let mut out = Vec::new();
    for path in entries(&root.join(MODEL_DIR))? {
        if path.is_file() && path.extension().is_some_and(|e| e == "qnt") {
            let text = std::fs::read_to_string(&path).map_err(|e| io_error(&path, &e))?;
            out.push(Module {
                path: relative(root, &path),
                text,
            });
        }
    }
    Ok(out)
}

/// The F-n a module's first line `// owns: F-…` claims, or `None` without that line.
#[must_use]
pub fn owned_by(module: &str) -> Option<Vec<u32>> {
    let list = module.lines().next()?.trim().strip_prefix("// owns:")?;
    Some(ids(list, "F-"))
}

/// Every `F<n>_<Name>` invariant a module declares: F-n, name, 1-based line.
#[must_use]
pub fn model_invariants(module: &str) -> Vec<(u32, String, usize)> {
    let mut out = Vec::new();
    for (i, line) in module.lines().enumerate() {
        let t = line.trim_start();
        let t = t.strip_prefix("pure ").unwrap_or(t);
        let Some(rest) = ["val ", "def ", "temporal "]
            .iter()
            .find_map(|k| t.strip_prefix(k))
        else {
            continue;
        };
        let rest = rest.trim_start();
        let len = rest
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(rest.len());
        let name = &rest[..len];
        let Some(tail) = name.strip_prefix('F') else {
            continue;
        };
        let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
        let has_name = tail[digits..]
            .strip_prefix('_')
            .is_some_and(|n| !n.is_empty());
        if let (true, Ok(n)) = (has_name, tail[..digits].parse()) {
            out.push((n, name.to_owned(), i + 1));
        }
    }
    out
}

/// Stage 3 over the Quint modules `modules`.
#[must_use]
pub fn stage3(d: &Design, modules: &[Module]) -> Vec<Violation> {
    let mut out = Vec::new();
    let mut owner: BTreeMap<u32, &str> = BTreeMap::new();
    for m in modules {
        let Some(owned) = owned_by(&m.text) else {
            continue;
        };
        let declared = model_invariants(&m.text);
        for &f in &owned {
            if !d.known(f) {
                out.push(violation(
                    &m.path,
                    Some(1),
                    Rule::Model,
                    format!("owns F-{f}, which FORMAL §4 does not define"),
                ));
            }
            if let Some(other) = owner.insert(f, &m.path) {
                out.push(violation(
                    &m.path,
                    Some(1),
                    Rule::Model,
                    format!("owns F-{f}, which {other} already owns"),
                ));
            }
            let mine: Vec<&(u32, String, usize)> =
                declared.iter().filter(|(n, _, _)| *n == f).collect();
            match mine.as_slice() {
                [_] => {}
                [] => out.push(violation(
                    &m.path,
                    Some(1),
                    Rule::Model,
                    format!("owns F-{f} but declares no invariant `F{f}_<Name>`"),
                )),
                [_, rest @ ..] => {
                    for (_, name, line) in rest {
                        out.push(violation(
                            &m.path,
                            Some(*line),
                            Rule::Model,
                            format!("`{name}` is a second invariant for F-{f}"),
                        ));
                    }
                }
            }
        }
    }
    for m in modules {
        let owned = owned_by(&m.text).unwrap_or_default();
        for (f, name, line) in model_invariants(&m.text) {
            if !owned.contains(&f) {
                out.push(violation(
                    &m.path,
                    Some(line),
                    Rule::Model,
                    format!("declares `{name}`, but its `// owns:` line does not name F-{f}"),
                ));
            }
        }
    }
    out.sort();
    out
}

/// What `run` was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Lint { closed: Option<String> },
    Awaiting(u64),
    Diff,
}

const USAGE: &str = "usage: trace_lint [--closed G-X | --awaiting #N | --diff]";

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Mode> {
    let args: Vec<String> = args.into_iter().collect();
    let usage = || Error::Parse(USAGE.to_owned());
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => Ok(Mode::Lint { closed: None }),
        ["--closed", g] => Ok(Mode::Lint {
            closed: Some((*g).to_owned()),
        }),
        ["--awaiting", n] => n
            .trim_start_matches('#')
            .parse()
            .map(Mode::Awaiting)
            .map_err(|_| usage()),
        ["--diff"] => Ok(Mode::Diff),
        _ => Err(usage()),
    }
}

fn report(violations: &[Violation], what: &str) -> ExitCode {
    for v in violations {
        println!("{v}");
    }
    if violations.is_empty() {
        println!("{what}: consistent");
        ExitCode::SUCCESS
    } else {
        println!("{what}: {} violation(s)", violations.len());
        ExitCode::FAILURE
    }
}

/// Runs the lint in the repository at the current directory, as `just trace-lint` does:
/// no argument runs stages 1 to 3, `--closed G-X` adds the closure of G-X, `--awaiting #N`
/// lists the tests awaiting #N, and `--diff` applies the diff rule to the pull request in
/// `GITHUB_EVENT_PATH` (and passes, saying so, when there is none).
///
/// # Errors
/// Fails on unknown arguments, an unreadable design set, fixture directory, model or event
/// payload, or a git failure.
pub fn run(args: impl IntoIterator<Item = String>) -> Result<ExitCode> {
    let root = Path::new(".");
    match parse_args(args)? {
        Mode::Awaiting(n) => {
            let tests = awaiting(&fixture_dirs(root)?, n);
            for t in &tests {
                println!("{}", t.at());
            }
            println!("{} test(s) awaiting #{n}", tests.len());
            Ok(ExitCode::SUCCESS)
        }
        Mode::Diff => {
            let Some(path) = std::env::var_os("GITHUB_EVENT_PATH") else {
                println!("diff rule: GITHUB_EVENT_PATH is unset; no pull request to check");
                return Ok(ExitCode::SUCCESS);
            };
            let event =
                std::fs::read_to_string(&path).map_err(|e| io_error(Path::new(&path), &e))?;
            match pull_request(root, &event)? {
                Some(v) => Ok(report(&v, "diff rule")),
                None => {
                    println!("diff rule: the event is not a pull request; nothing to check");
                    Ok(ExitCode::SUCCESS)
                }
            }
        }
        Mode::Lint { closed } => {
            let design = Design::load(&root.join(DESIGN_DIR))?;
            let mut v = stage1(&design);
            let code = stage2(&design, &fixture_dirs(root)?, closed.as_deref());
            for note in &code.notes {
                println!("{note}");
            }
            v.extend(code.violations);
            v.extend(stage3(&design, &modules(root)?));
            Ok(report(&v, "trace lint"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESIGN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/design");

    fn read(name: &str) -> String {
        std::fs::read_to_string(Path::new(DESIGN).join(name)).unwrap()
    }

    fn imported() -> Design {
        Design::load(Path::new(DESIGN)).unwrap()
    }

    /// The imported set with `from` replaced by `to` in the document `doc`.
    fn edited(doc: &str, from: &str, to: &str) -> Design {
        let mut docs = [read(THESIS), read(FORMAL), read(M0)];
        let i = [THESIS, FORMAL, M0].iter().position(|d| *d == doc).unwrap();
        assert!(docs[i].contains(from), "{from}");
        docs[i] = docs[i].replacen(from, to, 1);
        Design::parse(&docs[0], &docs[1], &docs[2]).unwrap()
    }

    fn messages(v: &[Violation]) -> Vec<String> {
        v.iter().map(ToString::to_string).collect()
    }

    const DISPATCH: &str = "crates/k/tests/g_dispatch";

    /// A G-DISPATCH fixture directory whose `mod.rs` names #228 and holds `tests`.
    fn dispatch(tests: &str) -> FixtureDir {
        let mut files = BTreeMap::new();
        files.insert(
            format!("{DISPATCH}/mod.rs"),
            "//! fixture-task: #228\nmod cases;\n".to_owned(),
        );
        files.insert(format!("{DISPATCH}/cases.rs"), tests.to_owned());
        FixtureDir {
            path: DISPATCH.to_owned(),
            files,
        }
    }

    /// One owning test per G-DISPATCH F-n, `f7_…` written as `f7`, all awaiting #9.
    fn complete(f7: &str) -> String {
        let mut s = f7.to_owned();
        for f in [8, 9, 10, 13, 14] {
            s.push_str(&format!(
                "#[test]\n#[ignore = \"awaiting #9\"]\nfn f{f}_case() {{}}\n\n"
            ));
        }
        s
    }

    const F7: &str = "#[test]\n#[ignore = \"awaiting #9\"]\nfn f7_hold_cut() {\n    run();\n}\n\n";

    #[test]
    fn stage1_passes_on_the_imported_set() {
        let d = imported();
        assert_eq!(stage1(&d), []);
        assert_eq!(d.invariants.len(), 10);
        assert_eq!(d.properties.len(), 37);
        assert_eq!(d.groups.len(), 9);
        assert_eq!(d.variants.len(), 15);
        let f37 = d.properties.iter().find(|p| p.id == 37).unwrap();
        assert_eq!(f37.invariant, Some(10));
        let f33 = d.properties.iter().find(|p| p.id == 33).unwrap();
        assert_eq!(f33.invariant, None);
        assert_eq!(d.group_of(9).unwrap().name, "G-DISPATCH");
        assert_eq!(d.group_of(17).unwrap().dir_name(), "g_custody_m0");
        let takeover = d
            .variants
            .iter()
            .find(|v| v.guard.starts_with("takeover"))
            .unwrap();
        assert_eq!(takeover.properties, [4, 11]);
    }

    #[test]
    fn f_n_removed_from_a_group_fails() {
        let d = edited(M0, "**G-COMMIT** (F-1 … F-6)", "**G-COMMIT** (F-1 … F-5)");
        let v = stage1(&d);
        assert_eq!(v.len(), 1, "{v:#?}");
        assert_eq!(v[0].rule, Rule::Group);
        assert_eq!(v[0].file, "docs/design/AUTOBOT-FORMAL-SURFACE.md");
        assert_eq!(
            v[0].message,
            "F-6 is listed by no fixture group; it needs exactly one"
        );
    }

    #[test]
    fn f_n_listed_by_two_groups_fails() {
        let d = edited(M0, "**G-RESTORE** (F-15)", "**G-RESTORE** (F-15, F-16)");
        assert_eq!(
            messages(&stage1(&d)),
            [format!(
                "docs/design/AUTOBOT-FORMAL-SURFACE.md:{}: [group] F-16 is listed by \
                 G-CUSTODY-M0, G-RESTORE; it needs exactly one",
                d.properties.iter().find(|p| p.id == 16).unwrap().line
            )]
        );
    }

    #[test]
    fn variant_naming_an_unknown_f_n_fails() {
        let d = edited(
            FORMAL,
            "| send without a `send_attempt` marker | F-18 |",
            "| send without a `send_attempt` marker | F-99 |",
        );
        let v = stage1(&d);
        assert_eq!(v.len(), 1, "{v:#?}");
        assert_eq!(v[0].rule, Rule::Variant);
        assert_eq!(
            v[0].message,
            "the variant `send without a `send_attempt` marker` names F-99, which FORMAL §4 does \
             not define"
        );
    }

    #[test]
    fn invariant_without_an_f_n_fails() {
        let d = edited(
            FORMAL,
            "**I-10 Economics** — F-37",
            "**Cross-cutting** — F-37",
        );
        let v = stage1(&d);
        assert_eq!(messages(&v).len(), 1, "{v:#?}");
        assert_eq!(v[0].message, "I-10 has no F-n in FORMAL §4");
        assert_eq!(v[0].file, "docs/design/AUTOBOT-THESIS.md");
    }

    #[test]
    fn ids_expand_ranges_and_respect_word_boundaries() {
        assert_eq!(ids("F-7 … F-10, F-13", "F-"), [7, 8, 9, 10, 13]);
        assert_eq!(ids("F-1..F-3 or GF-4, F-", "F-"), [1, 2, 3]);
        assert_eq!(ids("I-1 and I-10", "I-"), [1, 10]);
    }

    #[test]
    fn complete_group_passes_and_is_reported() {
        let r = stage2(&imported(), &[dispatch(&complete(F7))], None);
        assert_eq!(r.violations, []);
        assert_eq!(
            r.notes,
            ["stage 2: G-DISPATCH: 6 owning test(s), 6 ignored, in crates/k/tests/g_dispatch"]
        );
    }

    #[test]
    fn two_owning_tests_fail() {
        let two = format!("{F7}#[test]\nfn f7_other() {{}}\n");
        let r = stage2(&imported(), &[dispatch(&complete(&two))], None);
        assert_eq!(
            messages(&r.violations),
            [
                "crates/k/tests/g_dispatch/cases.rs:8: [owning-test] G-DISPATCH: `f7_other` is a \
                 second owning test of F-7, after crates/k/tests/g_dispatch/cases.rs:3 \
                 `f7_hold_cut`"
            ]
        );
    }

    #[test]
    fn zero_owning_tests_in_an_existing_directory_fail() {
        let r = stage2(&imported(), &[dispatch(&complete(""))], None);
        assert_eq!(
            messages(&r.violations),
            [
                "crates/k/tests/g_dispatch: [owning-test] G-DISPATCH: F-7 has no owning test `f7_<slug>`"
            ]
        );
        // Without the directory, the group is not checked at all.
        assert_eq!(stage2(&imported(), &[], None), Report::default());
    }

    #[test]
    fn ignored_owning_test_fails_closure() {
        let d = imported();
        let dirs = [dispatch(&complete(F7))];
        let v = stage2(&d, &dirs, Some("G-DISPATCH")).violations;
        assert_eq!(v.len(), 6, "{v:#?}");
        assert!(v.iter().all(|v| v.rule == Rule::Closed));
        assert_eq!(
            v[0].to_string(),
            "crates/k/tests/g_dispatch/cases.rs:3: [closed] G-DISPATCH is not closed: \
             `f7_hold_cut` is still ignored"
        );
        // With every ignore line deleted the group closes.
        let done = complete(F7).replace("#[ignore = \"awaiting #9\"]\n", "");
        assert_eq!(
            stage2(&d, &[dispatch(&done)], Some("G-DISPATCH")).violations,
            []
        );
        // A group without a fixture directory is not closed.
        let v = stage2(&d, &[], Some("G-RESTORE")).violations;
        assert_eq!(v.len(), 1);
        assert!(
            v[0].message.contains("no fixture directory"),
            "{}",
            v[0].message
        );
    }

    #[test]
    fn ignore_without_awaiting_fails() {
        for attr in [
            "#[ignore]",
            "#[ignore = \"flaky\"]",
            "#[ignore=\"awaiting #9\"]",
        ] {
            let f7 = F7.replace("#[ignore = \"awaiting #9\"]", attr);
            let r = stage2(&imported(), &[dispatch(&complete(&f7))], None);
            assert_eq!(
                messages(&r.violations),
                [format!(
                    "crates/k/tests/g_dispatch/cases.rs:3: [ignore] `f7_hold_cut` is ignored by \
                     `{attr}`, not by `#[ignore = \"awaiting #N\"]`"
                )],
                "{attr}"
            );
        }
    }

    #[test]
    fn exercises_lines_are_reported_but_misnamed_owners_fail() {
        let helper = "/// exercises: F-11\n#[test]\nfn hold_beside_takeover() {}\n\n";
        let r = stage2(
            &imported(),
            &[dispatch(&complete(&format!("{F7}{helper}")))],
            None,
        );
        assert_eq!(r.violations, []);
        assert_eq!(
            r.notes[0],
            "exercises: crates/k/tests/g_dispatch/cases.rs:9 `hold_beside_takeover` exercises \
             F-11 (owned by G-MANAGER)"
        );
        let misnamed = "#[test]\nfn f11_takeover() {}\n";
        let r = stage2(
            &imported(),
            &[dispatch(&complete(&format!("{F7}{misnamed}")))],
            None,
        );
        assert_eq!(r.violations.len(), 1, "{r:#?}");
        assert_eq!(
            r.violations[0].message,
            "`f11_takeover` is in G-DISPATCH but names F-11, which G-MANAGER owns; rename it and \
             mark it `/// exercises: F-11`"
        );
    }

    #[test]
    fn fixture_directory_needs_a_group_and_a_fixture_task() {
        let mut dir = dispatch(&complete(F7));
        dir.files
            .insert(format!("{DISPATCH}/mod.rs"), "mod cases;\n".to_owned());
        let mut stray = FixtureDir {
            path: "crates/k/tests/g_nothing".to_owned(),
            ..FixtureDir::default()
        };
        stray.files.insert(
            "crates/k/tests/g_nothing/mod.rs".to_owned(),
            "//! fixture-task: #1\n".to_owned(),
        );
        let v = messages(&stage2(&imported(), &[dir, stray], None).violations);
        assert_eq!(
            v,
            [
                "crates/k/tests/g_dispatch/mod.rs:1: [ignore] does not start with \
                 `//! fixture-task: #N`",
                "crates/k/tests/g_nothing: [owning-test] `g_nothing` names no M0 §3 fixture group",
            ]
        );
    }

    #[test]
    fn directories_of_one_group_must_name_one_fixture_task() {
        let mut other = dispatch("");
        other.path = "crates/other/tests/g_dispatch".to_owned();
        other.files = BTreeMap::from([(
            "crates/other/tests/g_dispatch/mod.rs".to_owned(),
            "//! fixture-task: #99\n".to_owned(),
        )]);
        let v = messages(
            &stage2(&imported(), &[dispatch(&complete(F7)), other.clone()], None).violations,
        );
        assert_eq!(
            v,
            [
                "crates/k/tests/g_dispatch/mod.rs:1: [ignore] G-DISPATCH: its directories name \
                 different fixture tasks: crates/k/tests/g_dispatch #228, \
                 crates/other/tests/g_dispatch #99"
            ]
        );
        // A second directory naming the same fixture task is fine.
        other.files.insert(
            "crates/other/tests/g_dispatch/mod.rs".to_owned(),
            "//! fixture-task: #228\n".to_owned(),
        );
        assert_eq!(
            stage2(&imported(), &[dispatch(&complete(F7)), other], None).violations,
            []
        );
    }

    #[test]
    fn group_task_reads_every_directory_of_the_group() {
        let paths: Vec<String> = [
            "crates/a/tests/g_dispatch/mod.rs",
            "crates/a/tests/g_dispatch/cases.rs",
            "crates/b/tests/g_dispatch/mod.rs",
            "crates/b/tests/g_commit/mod.rs",
            "crates/b/src/g_dispatch/mod.rs",
        ]
        .map(str::to_owned)
        .into();
        let read = |b: &'static str| {
            move |p: &str| match p {
                "crates/a/tests/g_dispatch/mod.rs" => Some("//! fixture-task: #228\n".to_owned()),
                "crates/b/tests/g_dispatch/mod.rs" => Some(format!("//! fixture-task: #{b}\n")),
                _ => None,
            }
        };
        assert_eq!(
            group_task(&paths, "g_dispatch", read("228")),
            Some(Some(228))
        );
        assert_eq!(group_task(&paths, "g_dispatch", read("99")), Some(None));
        assert_eq!(group_task(&paths, "g_commit", read("228")), Some(None));
        assert_eq!(group_task(&paths, "g_restore", read("228")), None);
    }

    #[test]
    fn test_parser_reads_attributes_docs_and_signatures() {
        let src = "mod m {\n    /// exercises: F-1 … F-2\n    // note\n    #[tokio::test]\n    \
                   #[ignore = \"awaiting #4\"]\n    pub(crate) async fn f3_a() {}\n    fn \
                   f4_helper() {}\n    #[test]\n    #[cfg_attr(\n        unix,\n        \
                   allow(unused)\n    )]\n    fn f5_b() {}\n}\n";
        let t = tests_in("x.rs", src);
        assert_eq!(t.len(), 2, "{t:#?}");
        assert_eq!(
            (t[0].name.as_str(), t[0].line, t[0].owns()),
            ("f3_a", 6, Some(3))
        );
        assert_eq!(t[0].ignore, Some(Ignore::Awaiting(4)));
        assert_eq!(t[0].exercises, [1, 2]);
        assert_eq!((t[1].name.as_str(), t[1].ignore.clone()), ("f5_b", None));
        let named = |n: &str| TestFn {
            name: n.to_owned(),
            ..t[0].clone()
        };
        assert_eq!(named("f07_x").owns(), None);
        assert_eq!(named("f7_").owns(), None);
        assert_eq!(named("f7").owns(), None);
    }

    #[test]
    fn awaiting_lists_tests_of_one_issue() {
        let mut f7 = F7.replace("#9", "#12");
        f7.push_str(&complete(""));
        let names: Vec<String> = awaiting(&[dispatch(&f7)], 12)
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(names, ["f7_hold_cut"]);
        assert_eq!(awaiting(&[dispatch(&f7)], 9).len(), 5);
    }

    /// A `-U0` diff of `crates/k/tests/g_dispatch/cases.rs` with the given hunk body.
    fn diff(hunk: &str) -> String {
        format!(
            "diff --git a/{DISPATCH}/cases.rs b/{DISPATCH}/cases.rs\nindex 1..2 100644\n--- \
             a/{DISPATCH}/cases.rs\n+++ b/{DISPATCH}/cases.rs\n{hunk}"
        )
    }

    fn task(dir: &str) -> Option<u64> {
        (dir == DISPATCH).then_some(228)
    }

    #[test]
    fn diff_rule_accepts_deleting_the_closed_issues_marker() {
        let d = diff("@@ -2 +1,0 @@\n-#[ignore = \"awaiting #9\"]\n");
        assert_eq!(diff_rule(&d, Some(9), task), []);
    }

    #[test]
    fn diff_rule_rejects_deleting_another_issues_marker() {
        let d = diff("@@ -2 +1,0 @@\n-#[ignore = \"awaiting #10\"]\n");
        assert_eq!(
            messages(&diff_rule(&d, Some(9), task)),
            [format!(
                "{DISPATCH}/cases.rs:2: [diff] deletes `#[ignore = \"awaiting #10\"]`; #9 may \
                 only delete `#[ignore = \"awaiting #9\"]` lines here"
            )]
        );
    }

    #[test]
    fn diff_rule_rejects_editing_a_test_body_outside_the_fixture_task() {
        let d = diff("@@ -4 +4 @@\n-    run();\n+    run_differently();\n");
        let v = diff_rule(&d, Some(9), task);
        assert_eq!(
            messages(&v),
            [
                format!(
                    "{DISPATCH}/cases.rs:4: [diff] adds `run_differently();`; #9 is not the \
                     fixture task of {DISPATCH}"
                ),
                format!(
                    "{DISPATCH}/cases.rs:4: [diff] deletes `run();`; #9 may only delete \
                     `#[ignore = \"awaiting #9\"]` lines here"
                ),
            ]
        );
        // The fixture task itself may change anything in its directory.
        assert_eq!(diff_rule(&d, Some(228), task), []);
        // Without `Closes #N` nothing may change, not even an ignore line.
        let del = diff("@@ -2 +1,0 @@\n-#[ignore = \"awaiting #9\"]\n");
        assert_eq!(diff_rule(&del, None, task).len(), 1);
        // Paths outside fixture directories are not the rule's business.
        let other = d.replace(DISPATCH, "crates/k/src");
        assert_eq!(diff_rule(&other, Some(9), task), []);
    }

    #[test]
    fn diff_parser_tracks_lines_and_header_lookalikes() {
        let d = diff("@@ -3,2 +3,0 @@\n--- not a header\n-x\n@@ -9,0 +8 @@\n+y\n");
        let files = parse_diff(&d);
        assert_eq!(files.len(), 1);
        assert_eq!(
            files[0].removed,
            [(3, "-- not a header".to_owned()), (4, "x".to_owned())]
        );
        assert_eq!(files[0].added, [(8, "y".to_owned())]);
    }

    #[test]
    fn closes_issue_takes_the_first_closing_reference() {
        assert_eq!(closes_issue("Closes #61\n\nSee #7. Closes #8"), Some(61));
        assert_eq!(closes_issue("fixes things; closes #5"), Some(5));
        assert_eq!(closes_issue("encloses #5"), None);
        assert_eq!(closes_issue("Closes #x"), None);
    }

    fn sh(dir: &Path, args: &[&str]) -> String {
        Cmd::new("git")
            .args(args.iter().copied())
            .current_dir(dir)
            .output()
            .unwrap()
    }

    #[test]
    fn pull_request_diff_rule_reads_the_repository() {
        let dir = std::env::temp_dir().join(format!("devtools-trace-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let fixture = dir.join(DISPATCH);
        std::fs::create_dir_all(&fixture).unwrap();
        sh(&dir, &["init", "-q", "-b", "main"]);
        sh(&dir, &["config", "user.email", "t@example.com"]);
        sh(&dir, &["config", "user.name", "t"]);
        std::fs::write(
            fixture.join("mod.rs"),
            "//! fixture-task: #228\nmod cases;\n",
        )
        .unwrap();
        std::fs::write(fixture.join("cases.rs"), F7).unwrap();
        sh(&dir, &["add", "."]);
        sh(&dir, &["commit", "-qm", "base"]);
        let base = sh(&dir, &["rev-parse", "HEAD"]).trim().to_owned();
        // A head on top of `base` that writes each (path, text) of `files`.
        let commit = |files: &[(&str, &str)]| {
            sh(&dir, &["checkout", "-q", &base]);
            for (path, text) in files {
                let path = dir.join(path);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, text).unwrap();
            }
            sh(&dir, &["add", "."]);
            sh(&dir, &["commit", "-qm", "edit"]);
            sh(&dir, &["rev-parse", "HEAD"]).trim().to_owned()
        };
        let cases = format!("{DISPATCH}/cases.rs");
        let edit = |text: &str| commit(&[(&cases, text)]);
        let event = |head: &str, body: &str| {
            serde_json::json!({
                "pull_request": {"base": {"sha": base}, "head": {"sha": head}, "body": body}
            })
            .to_string()
        };
        let unignored = edit(&F7.replace("#[ignore = \"awaiting #9\"]\n", ""));
        let check =
            |head: &str, body: &str| pull_request(&dir, &event(head, body)).unwrap().unwrap();
        assert_eq!(check(&unignored, "Closes #9"), []);
        assert_eq!(check(&unignored, "Closes #10").len(), 1);
        let rewritten = edit(&F7.replace("run();", "skip();"));
        assert_eq!(check(&rewritten, "Closes #9").len(), 2);
        assert_eq!(check(&rewritten, "Closes #228"), []);
        // Renaming the fixture task in the head does not make the pull request the fixture
        // task: the merge base's `mod.rs` decides.
        let mod_rs = format!("{DISPATCH}/mod.rs");
        let renamed = commit(&[
            (&mod_rs, "//! fixture-task: #9\nmod cases;\n"),
            (&cases, &F7.replace("run();", "skip();")),
        ]);
        let v = check(&renamed, "Closes #9");
        assert_eq!(v.len(), 4, "{v:#?}");
        assert!(v.iter().any(|v| v.file == mod_rs), "{v:#?}");
        // Neither does a new directory for a group that already has one.
        let second = "crates/other/tests/g_dispatch";
        let (second_mod, second_cases) = (format!("{second}/mod.rs"), format!("{second}/cases.rs"));
        let added = commit(&[
            (&second_mod, "//! fixture-task: #99\nmod cases;\n"),
            (&second_cases, "#[test]\nfn f7_other() {}\n"),
        ]);
        let v = check(&added, "Closes #99");
        assert_eq!(v.len(), 4, "{v:#?}");
        assert!(v.iter().all(|v| v.file.starts_with(second)), "{v:#?}");
        assert!(
            v[0].message
                .ends_with(&format!("#99 is not the fixture task of {second}")),
            "{v:#?}"
        );
        assert_eq!(check(&added, "Closes #228"), []);
        // A group with no directory at the merge base takes its fixture task from the head.
        let restore = "crates/k/tests/g_restore";
        let new_group = commit(&[
            (&format!("{restore}/mod.rs"), "//! fixture-task: #50\n"),
            (
                &format!("{restore}/cases.rs"),
                "#[test]\nfn f15_case() {}\n",
            ),
        ]);
        assert_eq!(check(&new_group, "Closes #50"), []);
        assert_eq!(check(&new_group, "Closes #51").len(), 3);
        assert_eq!(pull_request(&dir, "{\"ref\": \"main\"}").unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn loaders_find_fixture_directories_and_top_level_modules() {
        let dir = std::env::temp_dir().join(format!("devtools-trace-load-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let nested = dir.join(DISPATCH).join("more");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(dir.join("crates/k/tests/common")).unwrap();
        std::fs::create_dir_all(dir.join("formal/variants")).unwrap();
        std::fs::write(
            dir.join(DISPATCH).join("mod.rs"),
            "//! fixture-task: #228\n",
        )
        .unwrap();
        std::fs::write(nested.join("deep.rs"), F7).unwrap();
        std::fs::write(nested.join("notes.txt"), "x").unwrap();
        std::fs::write(dir.join("crates/k/tests/common/mod.rs"), F7).unwrap();
        std::fs::write(dir.join("formal/commit.qnt"), "// owns: F-1\n").unwrap();
        std::fs::write(dir.join("formal/variants/commit.qnt"), "// owns: F-1\n").unwrap();
        let dirs = fixture_dirs(&dir).unwrap();
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].path, DISPATCH);
        assert_eq!(
            dirs[0].files.keys().collect::<Vec<_>>(),
            [
                &format!("{DISPATCH}/mod.rs"),
                &format!("{DISPATCH}/more/deep.rs")
            ]
        );
        assert_eq!(dirs[0].tests()[0].file, format!("{DISPATCH}/more/deep.rs"));
        let paths: Vec<String> = modules(&dir).unwrap().into_iter().map(|m| m.path).collect();
        assert_eq!(paths, ["formal/commit.qnt"]);
        // A repository without `crates/` or `formal/` has nothing to check.
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(fixture_dirs(&dir).unwrap(), []);
        assert_eq!(modules(&dir).unwrap(), []);
    }

    #[test]
    fn stage3_checks_owned_invariants_only() {
        let d = imported();
        let module = |path: &str, text: &str| Module {
            path: path.to_owned(),
            text: text.to_owned(),
        };
        let commit = "// owns: F-1 … F-3\nmodule commit {\n  val F1_Idempotency = true\n  \
                      val F2_ReceiptDurability = true\n  pure def F3_ReceiptBarrier = true\n}\n";
        assert_eq!(stage3(&d, &[module("formal/commit.qnt", commit)]), []);
        // A module without an `owns` line, such as `types`, is not an owner.
        assert_eq!(
            stage3(&d, &[module("formal/types.qnt", "module types {}\n")]),
            []
        );
        let broken = commit
            .replace("  val F2_ReceiptDurability = true\n", "")
            .replace("}\n", "  val F1_Again = true\n  val F7_Linear = true\n}\n");
        let second = module(
            "formal/other.qnt",
            "// owns: F-3, F-99\nmodule other {\n  val F3_X = true\n}\n",
        );
        assert_eq!(
            messages(&stage3(&d, &[module("formal/commit.qnt", &broken), second])),
            [
                "formal/commit.qnt:1: [model] owns F-2 but declares no invariant `F2_<Name>`",
                "formal/commit.qnt:5: [model] `F1_Again` is a second invariant for F-1",
                "formal/commit.qnt:6: [model] declares `F7_Linear`, but its `// owns:` line does \
                 not name F-7",
                "formal/other.qnt:1: [model] owns F-3, which formal/commit.qnt already owns",
                "formal/other.qnt:1: [model] owns F-99 but declares no invariant `F99_<Name>`",
                "formal/other.qnt:1: [model] owns F-99, which FORMAL §4 does not define",
            ]
        );
    }

    #[test]
    fn arguments_select_the_mode() {
        let parse = |a: &[&str]| parse_args(a.iter().map(|s| (*s).to_owned()));
        assert_eq!(parse(&[]).unwrap(), Mode::Lint { closed: None });
        assert_eq!(
            parse(&["--closed", "G-COMMIT"]).unwrap(),
            Mode::Lint {
                closed: Some("G-COMMIT".to_owned())
            }
        );
        assert_eq!(parse(&["--awaiting", "#12"]).unwrap(), Mode::Awaiting(12));
        assert_eq!(parse(&["--diff"]).unwrap(), Mode::Diff);
        assert!(parse(&["--awaiting", "x"]).is_err());
        assert!(parse(&["--bogus"]).is_err());
    }
}
