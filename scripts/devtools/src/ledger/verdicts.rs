//! The blocking findings of a FAIL verdict and their class slugs, as the parent module docs
//! describe them.

use crate::markdown::{Fence, heading_of};

/// The class of a blocking finding that names none.
pub const UNTAGGED: &str = "untagged";

/// One blocking finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The class slugs it names, in order and without repeats; empty when it names none.
    pub classes: Vec<String>,
    /// The whole list item, continuation lines included.
    pub text: String,
}

impl Finding {
    /// Its classes, or [`UNTAGGED`] alone when it names none.
    pub fn class_names(&self) -> impl Iterator<Item = &str> {
        let untagged = self.classes.is_empty().then_some(UNTAGGED);
        self.classes.iter().map(String::as_str).chain(untagged)
    }
}

/// Whether `text` is a class slug: lowercase ASCII letters, digits and hyphens, starting with
/// a letter.
#[must_use]
pub fn is_slug(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_lowercase())
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The text after a top-level list marker (`- `, `* `, `1. ` or `1) `), or `None` when `line`
/// does not start a top-level list item.
fn item_start(line: &str) -> Option<&str> {
    if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
        return Some(rest);
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    let rest = &line[digits..];
    (digits > 0)
        .then(|| rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")))
        .flatten()
}

/// The class slugs the first line of a list item names.
#[must_use]
pub fn classes(item: &str) -> Vec<String> {
    let item = item.trim_start();
    let tagged = item
        .strip_prefix("**")
        .and_then(|after| after.find("**").map(|end| &after[end + 2..]))
        .and_then(|t| t.trim_start().strip_prefix('('))
        .and_then(|t| t.find(')').map(|end| &t[..end]));
    let slugs = |list: &str| -> Option<Vec<String>> {
        let mut out: Vec<String> = Vec::new();
        for slug in list.split(['/', ',']).map(str::trim) {
            if !is_slug(slug) {
                return None;
            }
            if !out.iter().any(|s| s == slug) {
                out.push(slug.to_owned());
            }
        }
        Some(out)
    };
    let listed = tagged.map(|inner| inner.split_once(':').map_or(inner, |(list, _)| list));
    if let Some(found) = listed.and_then(slugs) {
        return found;
    }
    item.trim_start_matches('*')
        .split_once(':')
        .and_then(|(lead, _)| is_slug(lead).then(|| vec![lead.to_owned()]))
        .unwrap_or_default()
}

/// The text of a section label: a Markdown heading, or an unindented line that is one bold span,
/// such as `**Defects:**`.
fn label(line: &str) -> Option<&str> {
    if let Some((_, text)) = heading_of(line.trim_end()) {
        return Some(text);
    }
    let inner = line.trim_end().strip_prefix("**")?.strip_suffix("**")?;
    (!inner.contains("**")).then_some(inner)
}

/// Whether a label opens the blocking findings: it starts with `Blocking` or `Defects`, in any
/// case.
fn opens_findings(label: &str) -> bool {
    let label = label.to_ascii_lowercase();
    label.starts_with("blocking") || label.starts_with("defects")
}

/// The list items among `lines`: each starts at a top-level list marker (a numbered one only,
/// when `numbered_only`) and runs until the next item.
fn items<'a>(lines: impl IntoIterator<Item = &'a str>, numbered_only: bool) -> Vec<String> {
    let mut items: Vec<String> = Vec::new();
    for line in lines {
        let start =
            item_start(line).filter(|_| !numbered_only || line.starts_with(char::is_numeric));
        match (start, items.last_mut()) {
            (Some(rest), _) => items.push(rest.to_owned()),
            (None, Some(item)) => {
                item.push('\n');
                item.push_str(line);
            }
            (None, None) => {}
        }
    }
    items
}

/// The blocking findings of a verdict body, one per list item of its first section whose label
/// (a heading, or a line that is one bold span) starts with `Blocking` or `Defects`; the
/// section runs to the next label. Text before the first item, such as `None.`, is not a
/// finding. A body with no such section lists its findings as the numbered items before its
/// first label. Labels inside fenced code do not count.
#[must_use]
pub fn blocking_findings(body: &str) -> Vec<Finding> {
    let mut fence = Fence::default();
    let lines: Vec<(Option<&str>, &str)> = body
        .lines()
        .map(|line| (if fence.step(line) { None } else { label(line) }, line))
        .collect();
    let texts = match lines
        .iter()
        .position(|(l, _)| l.is_some_and(opens_findings))
    {
        Some(start) => {
            let section = lines[start + 1..].iter().take_while(|(l, _)| l.is_none());
            items(section.map(|(_, line)| *line), false)
        }
        None => {
            let before = lines.iter().take_while(|(l, _)| l.is_none());
            items(before.map(|(_, line)| *line), true)
        }
    };
    texts
        .into_iter()
        .map(|text| Finding {
            classes: classes(text.lines().next().unwrap_or_default()),
            text,
        })
        .collect()
}
