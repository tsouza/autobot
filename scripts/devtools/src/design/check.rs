//! The design-set consistency check behind `just check-design`.
//!
//! A design set is a directory of Markdown files: the core documents `AUTOBOT-*.md` at its
//! top, the extension files under `extensions/`, and `*.background.md` companions. Main files
//! are all Markdown files that are not background files. The check applies six rules:
//!
//! 1. [`Rule::StateVocabulary`]: every ALL-CAPS token of three or more characters in a main
//!    file is a KERNEL §10 lifecycle state or in [`VOCABULARY`].
//! 2. [`Rule::GlossaryLayer`]: the GLOSSARY CORE section names no term defined under
//!    EXTENSION or EXTERNAL, other than a role that CORE itself defines under the same name
//!    (a CORE entry whose definition begins `The role of`); a lowercase plain part after ` / `
//!    in a label qualifies the first part and is no term.
//! 3. [`Rule::CoreDependsOnExtension`]: no core document's `Depends on:` line names
//!    `extensions/` or an extension file.
//! 4. [`Rule::NumberOutsideProfile`]: no main file other than [`M0`] holds a number of two or
//!    more digits, except `10`, `11`, and tokens prefixed `F-`, `I-` or `§`.
//! 5. [`Rule::Background`]: a main file with a background file ends with the line
//!    ``Background: `<name>.background.md`.``, a pointer names an existing file, and every
//!    background file has a main file and clears both floors: at least [`BACKGROUND_MIN_LINES`]
//!    lines and at least one twentieth of its main file's lines.
//! 6. [`Rule::Link`]: every Markdown link resolves to a file inside the set.

use crate::design::{self, RuleName, is_word_char, lifecycle, line_of, offset_in};
use crate::{Error, Result, markdown};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};
use std::process::ExitCode;

/// The KERNEL document, whose §10 defines the lifecycle states.
pub const KERNEL: &str = "AUTOBOT-KERNEL.md";
/// The GLOSSARY document, whose sections define the layers.
pub const GLOSSARY: &str = "AUTOBOT-GLOSSARY.md";
/// The M0 document, the single home of numeric limits.
pub const M0: &str = "AUTOBOT-M0-AND-GATES.md";
/// The minimum length of a background file, in lines.
pub const BACKGROUND_MIN_LINES: usize = 50;
/// A background file has at least `1 / BACKGROUND_MIN_RATIO` of its main file's lines.
pub const BACKGROUND_MIN_RATIO: usize = 20;

/// ALL-CAPS tokens a main file may use besides the §10 states.
pub const VOCABULARY: &[&str] = &[
    // Document names and the authority tags glossary entries cite.
    "AUTOBOT",
    "THESIS",
    "GLOSSARY",
    "TRUST",
    "MODEL",
    "KERNEL",
    "ROLES",
    "AND",
    "RUNTIME",
    "ONBOARDING",
    "ONBOARD",
    "CONVERGENCE",
    "FORMAL",
    "SURFACE",
    "GATES",
    // The root charter file a charter is projected into.
    "CHARTER",
    // Glossary layers.
    "CORE",
    "EXTENSION",
    "EXTERNAL",
    // Gate and fixture-group names (`G-…`).
    "ADAPT",
    "COMMIT",
    "CUSTODY",
    "DISPATCH",
    "EFFECT",
    "EVALUATION",
    "EVIDENCE",
    "FENCE",
    "FORGE",
    "INTAKE",
    "MANAGER",
    "OBS",
    "OPS",
    "PORTABILITY",
    "PROD",
    "QUAL",
    "RECORD",
    "RESTORE",
    "SCOPE",
    // Acronyms and protocol words.
    "API",
    "CAS",
    "CLI",
    "CQRS",
    "CRD",
    "FIFO",
    "GET",
    "JSON",
    "KMS",
    "MCP",
    "OCI",
    "RBAC",
    "RPO",
    "RTO",
    "SDK",
    "SIG",
    "SLO",
    "TLA",
    "TLC",
    "TTL",
    "UID",
    "URL",
    "WAL",
    // `Intervention` actions.
    "HOLD",
    "RESUME",
    "PAUSE",
    "QUIESCE",
    "SUPERSEDE",
    "FAIL",
    "CANCEL",
    "KILL_SWITCH",
    "ADJUDICATE_OPERATION",
    "ADJUDICATE_CONFLICT",
    // Enumerated schema values that are not lifecycle states.
    "DOMAIN",
    "CONTROL",
    "ATTEMPT",
    "ALLOW",
    "DENY",
    "DETECTED_AT_CHECKPOINT",
    "OUTCOME_MISSING",
    "USAGE_MISSING",
    "UNQUALIFIED",
    // Consequence classes.
    "REVERSIBLE",
    "COMPATIBILITY_RISK",
    "SECURITY_OR_DATA_INTEGRITY",
    // Review classification.
    "DESIGN_DEFECT",
    "DESIGN_GAP",
    "PLANNED_ARTIFACT",
    "IMPLEMENTATION_FAIL",
    "OUT_OF_SCOPE",
    // Trust labels of proposal fields (ONBOARD §1).
    "UNTRUSTED_REPOSITORY_CONTENT",
    "UNTRUSTED_ISSUE_OR_PR_TEXT",
    "INTERVIEW_ANSWER",
    // Further trust labels and fallback kinds of the decision-policy extension.
    "CANONICAL_AUTOBOT_FACT",
    "AUTHENTICATED_PROVIDER_OBSERVATION",
    "UNTRUSTED_CI_OUTPUT",
    "DERIVED_STATISTIC",
    "DERIVED_FALLBACK_DECISION",
    "ABSTAIN",
    "DETERMINISTIC_RULE",
    "STRONGER_REASONER",
    // `DriftAssessment` values.
    "ON_TRACK",
    "DRIFT_RISK",
    "DRIFTED",
    "STUCK",
    "COMPROMISED",
    "INSUFFICIENT_EVIDENCE",
    // Extension-gate states named but not printed in a core document.
    "BACKFILLING",
    // Retired names, listed in the glossary so they are not reintroduced.
    "REMOTE_SENT",
    "REVISION_PENDING",
    "ANALYZING",
    "NEEDS_INPUT",
];

/// The rule a violation breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rule {
    /// An ALL-CAPS token that is neither a §10 state nor in the vocabulary.
    StateVocabulary,
    /// The GLOSSARY CORE section names an EXTENSION or EXTERNAL term.
    GlossaryLayer,
    /// A core document depends on an extension.
    CoreDependsOnExtension,
    /// A number outside the M0 profile.
    NumberOutsideProfile,
    /// A missing or wrong background pointer, or a background file below a floor.
    Background,
    /// A link that does not resolve to a file inside the set.
    Link,
}

impl RuleName for Rule {
    fn name(self) -> &'static str {
        match self {
            Self::StateVocabulary => "state-vocabulary",
            Self::GlossaryLayer => "glossary-layer",
            Self::CoreDependsOnExtension => "core-depends-on-extension",
            Self::NumberOutsideProfile => "number-outside-m0",
            Self::Background => "background",
            Self::Link => "link",
        }
    }
}

/// One broken rule at one place; its file is relative to the set's directory.
pub type Violation = design::Violation<Rule>;

/// The files of a design set: Markdown contents, and the paths of every file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DesignSet {
    docs: BTreeMap<String, String>,
    paths: BTreeSet<String>,
}

impl DesignSet {
    /// Reads every file under `dir` outside directories named `target`; paths are relative to
    /// it, with `/` separators.
    ///
    /// # Errors
    /// Fails if a directory cannot be listed or a Markdown file cannot be read as UTF-8.
    pub fn load(dir: &Path) -> Result<Self> {
        let mut set = Self::default();
        for (rel, path) in design::walk(dir, dir)? {
            if rel.ends_with(".md") {
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| Error::Parse(format!("reading {}: {e}", path.display())))?;
                set.docs.insert(rel.clone(), text);
            }
            set.paths.insert(rel);
        }
        Ok(set)
    }

    /// Adds or replaces a Markdown file.
    pub fn insert(&mut self, path: &str, text: &str) {
        self.paths.insert(path.to_owned());
        self.docs.insert(path.to_owned(), text.to_owned());
    }

    /// Removes a file.
    pub fn remove(&mut self, path: &str) {
        self.paths.remove(path);
        self.docs.remove(path);
    }

    fn doc(&self, path: &str) -> Result<&str> {
        self.docs
            .get(path)
            .map(String::as_str)
            .ok_or_else(|| Error::Parse(format!("the design set has no {path}")))
    }

    fn main_files(&self) -> impl Iterator<Item = (&str, &str)> {
        self.docs
            .iter()
            .filter(|(p, _)| !p.ends_with(".background.md"))
            .map(|(p, t)| (p.as_str(), t.as_str()))
    }

    fn core_files(&self) -> impl Iterator<Item = (&str, &str)> {
        self.main_files()
            .filter(|(p, _)| p.starts_with("AUTOBOT-") && !p.contains('/'))
    }
}

/// Checks `dir`, prints every violation or `design set: consistent`, and returns the exit code.
///
/// # Errors
/// Fails if the set cannot be read, or KERNEL or GLOSSARY is missing or §10 does not parse.
pub fn run(dir: &Path) -> Result<ExitCode> {
    let violations = check(&DesignSet::load(dir)?)?;
    if violations.is_empty() {
        println!("design set: consistent");
        return Ok(ExitCode::SUCCESS);
    }
    for v in &violations {
        println!("{v}");
    }
    println!("design set: {} violation(s)", violations.len());
    Ok(ExitCode::FAILURE)
}

/// Every violation in `set`, sorted by file and line.
///
/// # Errors
/// Fails if KERNEL or GLOSSARY is missing or KERNEL §10 does not parse.
pub fn check(set: &DesignSet) -> Result<Vec<Violation>> {
    let states = lifecycle::all_states(&lifecycle::parse(set.doc(KERNEL)?)?);
    let mut out = Vec::new();
    state_vocabulary(set, &states, &mut out);
    glossary_layer(set.doc(GLOSSARY)?, &mut out);
    core_depends_on_extension(set, &mut out);
    numbers_outside_profile(set, &mut out);
    background(set, &mut out);
    links(set, &mut out);
    out.sort();
    Ok(out)
}

fn state_vocabulary(set: &DesignSet, states: &BTreeSet<String>, out: &mut Vec<Violation>) {
    for (file, text) in set.main_files() {
        for (n, line) in text.lines().enumerate() {
            let mut seen = BTreeSet::new();
            for word in line.split(|c: char| !is_word_char(c)) {
                if word.len() >= 3
                    && lifecycle::is_state(word)
                    && !states.contains(word)
                    && !VOCABULARY.contains(&word)
                    && seen.insert(word)
                {
                    out.push(Violation::new(
                        file,
                        Some(n + 1),
                        Rule::StateVocabulary,
                        format!("`{word}` is neither a KERNEL §10 state nor in the vocabulary"),
                    ));
                }
            }
        }
    }
}

/// The terms a glossary section defines, from its entry labels split at ` / `.
///
/// The first part of a label is always a term. A later part is a term when it is a code span
/// or starts with a capital; a lowercase plain word or phrase there only qualifies the first
/// part (`Capability matrix / qualification`), so with `qualifiers` false it is left out.
fn glossary_terms(section: &str, qualifiers: bool) -> Vec<String> {
    let mut terms = Vec::new();
    for line in section.lines() {
        let Some(rest) = line.trim_start().strip_prefix("- **") else {
            continue;
        };
        let Some((label, _)) = rest.split_once("**") else {
            continue;
        };
        let mut plain = String::new();
        let mut depth = 0usize;
        for c in label.chars() {
            match c {
                '(' => depth += 1,
                ')' => depth = depth.saturating_sub(1),
                _ if depth == 0 => plain.push(c),
                _ => {}
            }
        }
        for (i, part) in plain.split(" / ").map(str::trim).enumerate() {
            let named = part.starts_with(|c: char| c == '`' || c.is_uppercase());
            let term = part.replace('`', "");
            if !term.is_empty() && (i == 0 || named || qualifiers) {
                terms.push(term);
            }
        }
    }
    terms
}

/// Byte offsets of `needle` in `hay` as a whole phrase, ignoring ASCII case.
fn phrase_matches(hay: &str, needle: &str) -> Vec<usize> {
    let hay_l = hay.to_ascii_lowercase();
    let needle_l = needle.to_ascii_lowercase();
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(i) = hay_l[from..].find(&needle_l) {
        let at = from + i;
        let end = at + needle_l.len();
        let before = hay[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !is_word_char(c));
        let after = hay[end..].chars().next().is_none_or(|c| !is_word_char(c));
        if before && after {
            found.push(at);
        }
        from = end;
    }
    found
}

/// Whether a glossary entry line defines a role: its definition begins `The role of`.
fn is_role(line: &str) -> bool {
    line.trim_start()
        .strip_prefix("- **")
        .and_then(|rest| rest.split_once("**"))
        .is_some_and(|(_, def)| {
            def.trim_start_matches([' ', '—'])
                .starts_with("The role of")
        })
}

fn glossary_layer(glossary: &str, out: &mut Vec<Violation>) {
    let section = |name: &str| markdown::section(glossary, name);
    let (Some(core), Some(ext), Some(external)) =
        (section("CORE"), section("EXTENSION"), section("EXTERNAL"))
    else {
        out.push(Violation::new(
            GLOSSARY,
            None,
            Rule::GlossaryLayer,
            "the glossary needs CORE, EXTENSION and EXTERNAL sections".to_owned(),
        ));
        return;
    };
    let core_offset = offset_in(glossary, core);
    let roles: String = core
        .lines()
        .filter(|l| is_role(l))
        .flat_map(|l| [l, "\n"])
        .collect();
    let core_roles: BTreeSet<String> = glossary_terms(&roles, true)
        .iter()
        .map(|t| t.to_ascii_lowercase())
        .collect();
    let mut outside: BTreeMap<String, &str> = BTreeMap::new();
    for (layer, text) in [("EXTENSION", ext), ("EXTERNAL", external)] {
        for term in glossary_terms(text, false) {
            if !core_roles.contains(&term.to_ascii_lowercase()) {
                outside.entry(term).or_insert(layer);
            }
        }
    }
    for (term, layer) in outside {
        for at in phrase_matches(core, &term) {
            out.push(Violation::new(
                GLOSSARY,
                Some(line_of(glossary, core_offset + at)),
                Rule::GlossaryLayer,
                format!("the CORE section names the {layer} term `{term}`"),
            ));
        }
    }
}

fn core_depends_on_extension(set: &DesignSet, out: &mut Vec<Violation>) {
    let extensions: Vec<&str> = set
        .docs
        .keys()
        .filter_map(|p| p.strip_prefix("extensions/"))
        .filter_map(|p| p.strip_suffix(".md"))
        .collect();
    for (file, text) in set.core_files() {
        for (n, line) in text.lines().enumerate() {
            let Some(deps) = line.strip_prefix("Depends on:") else {
                continue;
            };
            let named = if deps.contains("extensions/") {
                Some("extensions/")
            } else {
                extensions
                    .iter()
                    .copied()
                    .find(|e| !phrase_matches(deps, e).is_empty())
            };
            if let Some(e) = named {
                out.push(Violation::new(
                    file,
                    Some(n + 1),
                    Rule::CoreDependsOnExtension,
                    format!("a core document depends on the extension `{e}`"),
                ));
            }
        }
    }
}

fn is_token_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '.' | '_' | '§')
}

fn numbers_outside_profile(set: &DesignSet, out: &mut Vec<Violation>) {
    for (file, text) in set.main_files().filter(|(p, _)| *p != M0) {
        for (n, line) in text.lines().enumerate() {
            let chars: Vec<(usize, char)> = line.char_indices().collect();
            let mut i = 0;
            while i < chars.len() {
                if !chars[i].1.is_ascii_digit() {
                    i += 1;
                    continue;
                }
                let start = i;
                while i < chars.len() && chars[i].1.is_ascii_digit() {
                    i += 1;
                }
                let begin = chars[start].0;
                let end = chars.get(i).map_or(line.len(), |c| c.0);
                let digits = &line[begin..end];
                if digits.len() < 2 || digits == "10" || digits == "11" {
                    continue;
                }
                let token_start = line[..begin]
                    .char_indices()
                    .rev()
                    .take_while(|(_, c)| is_token_char(*c))
                    .last()
                    .map_or(begin, |(p, _)| p);
                let token_end = line[end..]
                    .char_indices()
                    .find(|(_, c)| !is_token_char(*c))
                    .map_or(line.len(), |(p, _)| end + p);
                let token = line[token_start..token_end].trim_end_matches('.');
                if ["F-", "I-", "§"].iter().any(|p| token.starts_with(p)) {
                    continue;
                }
                out.push(Violation::new(
                    file,
                    Some(n + 1),
                    Rule::NumberOutsideProfile,
                    format!("the number `{token}` belongs in the M0 profile ({M0})"),
                ));
            }
        }
    }
}

fn background(set: &DesignSet, out: &mut Vec<Violation>) {
    for (file, text) in set.main_files() {
        let stem = file.strip_suffix(".md").unwrap_or(file);
        let bg = format!("{stem}.background.md");
        let name = bg.rsplit('/').next().unwrap_or(&bg);
        let last = text.lines().rev().find(|l| !l.trim().is_empty());
        let pointer = format!("Background: `{name}`.");
        let points = last.is_some_and(|l| l.starts_with("Background:"));
        match set.docs.get(&bg) {
            Some(bg_text) => {
                if last != Some(pointer.as_str()) {
                    out.push(Violation::new(
                        file,
                        None,
                        Rule::Background,
                        format!("does not end with the pointer line `{pointer}`"),
                    ));
                }
                let (main_lines, bg_lines) = (text.lines().count(), bg_text.lines().count());
                if bg_lines < BACKGROUND_MIN_LINES || bg_lines * BACKGROUND_MIN_RATIO < main_lines {
                    out.push(Violation::new(
                        &bg,
                        None,
                        Rule::Background,
                        format!(
                            "{bg_lines} lines against {main_lines} in {file}: below the floor of \
                             {BACKGROUND_MIN_LINES} lines and 1/{BACKGROUND_MIN_RATIO} of the main file"
                        ),
                    ));
                }
            }
            None if points => out.push(Violation::new(
                file,
                None,
                Rule::Background,
                format!("ends with a background pointer, but {bg} does not exist"),
            )),
            None => {}
        }
    }
    for file in set.docs.keys() {
        if let Some(stem) = file.strip_suffix(".background.md")
            && !set.docs.contains_key(&format!("{stem}.md"))
        {
            out.push(Violation::new(
                file,
                None,
                Rule::Background,
                format!("has no main file {stem}.md"),
            ));
        }
    }
}

/// Link targets on one line of Markdown outside code: inline links, autolinks and
/// reference definitions.
fn link_targets(line: &str) -> Vec<String> {
    let plain = strip_code_spans(line);
    let mut targets = Vec::new();
    let trimmed = plain.trim_start();
    if trimmed.starts_with('[')
        && let Some((label, target)) = trimmed.split_once("]:")
        && !label.contains("](")
    {
        targets.push(target.trim().to_owned());
    }
    let mut rest = plain.as_str();
    while let Some(i) = rest.find("](") {
        let after = &rest[i + 2..];
        let end = after.find(')').unwrap_or(after.len());
        targets.push(after[..end].trim().to_owned());
        rest = &after[end..];
    }
    let mut rest = plain.as_str();
    while let Some(i) = rest.find('<') {
        let after = &rest[i + 1..];
        let end = after.find('>').unwrap_or(after.len());
        let inner = &after[..end];
        if inner.contains("://") || inner.starts_with("mailto:") {
            targets.push(inner.to_owned());
        }
        rest = &after[end..];
    }
    targets
}

/// `line` without its CommonMark code spans: a backtick run opens a span that the next
/// run of the same length closes, and a run with no such closer is literal text.
fn strip_code_spans(line: &str) -> String {
    let bytes = line.as_bytes();
    let run_at = |i: usize| bytes[i..].iter().take_while(|&&b| b == b'`').count();
    let mut plain = String::new();
    let mut i = 0;
    let mut copied = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let open = run_at(i);
        let mut j = i + open;
        let mut close = None;
        while j < bytes.len() {
            if bytes[j] == b'`' {
                let run = run_at(j);
                if run == open {
                    close = Some(j + run);
                    break;
                }
                j += run;
            } else {
                j += 1;
            }
        }
        match close {
            Some(end) => {
                plain.push_str(&line[copied..i]);
                copied = end;
                i = end;
            }
            None => i += open,
        }
    }
    plain.push_str(&line[copied..]);
    plain
}

/// Where a link target from `file` points inside the set, or why it leaves it.
fn resolve(file: &str, target: &str) -> std::result::Result<Option<String>, &'static str> {
    let target = target.trim_start_matches('<');
    let target = target.split_whitespace().next().unwrap_or("");
    let target = target.trim_end_matches('>');
    let path = target.split('#').next().unwrap_or("");
    if path.is_empty() {
        return Ok(None);
    }
    if path
        .split('/')
        .next()
        .is_some_and(|first| first.contains(':'))
        || path.starts_with('/')
    {
        return Err("leaves the design set");
    }
    let mut parts: Vec<&str> = file.split('/').collect();
    parts.pop();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(c) => parts.push(c.to_str().ok_or("is not UTF-8")?),
            Component::ParentDir => {
                parts.pop().ok_or("leaves the design set")?;
            }
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => return Err("leaves the design set"),
        }
    }
    Ok(Some(parts.join("/")))
}

fn links(set: &DesignSet, out: &mut Vec<Violation>) {
    for (file, text) in &set.docs {
        let mut fence = markdown::Fence::default();
        for (n, line) in text.lines().enumerate() {
            if fence.step(line) {
                continue;
            }
            for target in link_targets(line) {
                let problem = match resolve(file, &target) {
                    Ok(Some(path)) if !set.paths.contains(&path) => Some("resolves to no file"),
                    Ok(_) => None,
                    Err(why) => Some(why),
                };
                if let Some(why) = problem {
                    out.push(Violation::new(
                        file,
                        Some(n + 1),
                        Rule::Link,
                        format!("the link `{target}` {why}"),
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/design");

    fn base() -> DesignSet {
        DesignSet::load(&Path::new(FIXTURES).join("base")).unwrap()
    }

    /// The base set with the files of fixture `name` laid over it.
    fn seeded(name: &str) -> DesignSet {
        let mut set = base();
        let overlay = DesignSet::load(&Path::new(FIXTURES).join(name)).unwrap();
        for (path, text) in &overlay.docs {
            set.insert(path, text);
        }
        set
    }

    fn only(set: &DesignSet) -> Violation {
        let mut v = check(set).unwrap();
        assert_eq!(v.len(), 1, "{v:#?}");
        v.remove(0)
    }

    #[test]
    fn base_fixture_is_consistent() {
        assert_eq!(check(&base()).unwrap(), []);
    }

    #[test]
    fn unknown_all_caps_token_fails() {
        let v = only(&seeded("state-vocabulary"));
        assert_eq!(v.rule, Rule::StateVocabulary);
        assert_eq!(
            v.to_string(),
            "AUTOBOT-THESIS.md:6: [state-vocabulary] `LAUNCHED` is neither a KERNEL §10 state \
             nor in the vocabulary"
        );
    }

    #[test]
    fn core_glossary_naming_an_extension_term_fails() {
        let v = only(&seeded("glossary-layer"));
        assert_eq!(v.rule, Rule::GlossaryLayer);
        assert_eq!(v.line, Some(9));
        assert!(
            v.message.contains("EXTENSION term `RoutingTable`"),
            "{}",
            v.message
        );
    }

    #[test]
    fn core_depending_on_an_extension_fails() {
        let v = only(&seeded("core-depends-on-extension"));
        assert_eq!(v.rule, Rule::CoreDependsOnExtension);
        assert_eq!(v.file, "AUTOBOT-THESIS.md");
        assert!(v.message.contains("`extensions/`"), "{}", v.message);
    }

    #[test]
    fn number_outside_m0_fails() {
        let v = only(&seeded("number-outside-m0"));
        assert_eq!(v.rule, Rule::NumberOutsideProfile);
        assert_eq!(v.file, "extensions/sample.md");
        assert!(v.message.starts_with("the number `30s`"), "{}", v.message);
    }

    #[test]
    fn missing_background_pointer_fails() {
        let v = only(&seeded("background-pointer"));
        assert_eq!(v.rule, Rule::Background);
        assert_eq!(v.file, "AUTOBOT-KERNEL.md");
        assert!(
            v.message.contains("does not end with the pointer line"),
            "{}",
            v.message
        );
    }

    #[test]
    fn background_below_the_floor_fails() {
        let v = only(&seeded("background-floor"));
        assert_eq!(v.rule, Rule::Background);
        assert_eq!(v.file, "AUTOBOT-KERNEL.background.md");
        assert!(v.message.starts_with("49 lines"), "{}", v.message);
    }

    #[test]
    fn link_outside_the_set_fails() {
        let v = only(&seeded("link"));
        assert_eq!(v.rule, Rule::Link);
        assert_eq!(v.message, "the link `../../guide.md` leaves the design set");
    }

    #[test]
    fn number_exemptions_hold() {
        let mut set = base();
        set.insert(
            "extensions/sample.md",
            "# Sample\n\nSee KERNEL §10.2, §3.11, F-12, I-10, 10 and 11.\n",
        );
        set.insert(
            "AUTOBOT-KERNEL.background.md",
            &"Measured 250 ms.\n".repeat(50),
        );
        assert_eq!(check(&set).unwrap(), []);
        // The same numbers outside the exempt forms are flagged one by one.
        set.insert(
            "extensions/sample.md",
            "# Sample\n\nSee 3.12, G-12, 250, sha256, p99 and T300.\n",
        );
        let tokens: Vec<String> = check(&set)
            .unwrap()
            .into_iter()
            .filter(|v| v.rule == Rule::NumberOutsideProfile)
            .map(|v| v.message)
            .collect();
        assert_eq!(tokens.len(), 6, "{tokens:?}");
        for want in ["`3.12`", "`G-12`", "`250`", "`sha256`", "`p99`", "`T300`"] {
            assert!(
                tokens.iter().any(|t| t.contains(want)),
                "{want}: {tokens:?}"
            );
        }
    }

    #[test]
    fn core_role_named_like_an_external_service_is_exempt_only_while_core_defines_it() {
        let glossary = base()
            .doc(GLOSSARY)
            .unwrap()
            .replace("- **Forge** — The role", "- **Host** — The role");
        let mut set = base();
        set.insert(GLOSSARY, &glossary);
        let v = only(&set);
        assert_eq!(v.rule, Rule::GlossaryLayer);
        assert_eq!(
            v.message,
            "the CORE section names the EXTERNAL term `Forge`"
        );
    }

    #[test]
    fn core_entry_that_is_no_role_does_not_exempt_an_extension_term() {
        let glossary = base().doc(GLOSSARY).unwrap().replace(
            "- **Forge** — The role",
            "- **`RoutingTable`** — Copied into core.\n- **Forge** — The role",
        );
        let mut set = base();
        set.insert(GLOSSARY, &glossary);
        let v = only(&set);
        assert_eq!(v.rule, Rule::GlossaryLayer);
        assert_eq!(v.line, Some(10));
        assert_eq!(
            v.message,
            "the CORE section names the EXTENSION term `RoutingTable`"
        );
    }

    #[test]
    fn lowercase_label_qualifier_is_not_a_term_but_named_aliases_are() {
        let glossary = base().doc(GLOSSARY).unwrap().replace(
            "- **`RoutingTable` / route hint** — The table the routing extension keeps.",
            "- **Capability matrix / qualification / `RouteHint` / Matrix Row** — Per-provider \
             status.",
        );
        let with_core = |core_line: &str| {
            let mut set = base();
            set.insert(
                GLOSSARY,
                &glossary.replace(
                    "- **Forge** — The role",
                    &format!("{core_line}\n- **Forge** — The role"),
                ),
            );
            check(&set).unwrap()
        };
        assert_eq!(with_core("- **Status** — Has a qualification status."), []);
        for (line, term) in [
            (
                "- **Status** — Mirrors the capability matrix.",
                "Capability matrix",
            ),
            ("- **Status** — Carries a RouteHint.", "RouteHint"),
            ("- **Status** — Keeps a matrix row.", "Matrix Row"),
        ] {
            let got = with_core(line);
            assert_eq!(got.len(), 1, "{line}: {got:?}");
            assert_eq!(
                got[0].message,
                format!("the CORE section names the EXTENSION term `{term}`")
            );
        }
    }

    #[test]
    fn all_caps_token_with_a_digit_is_checked() {
        let mut set = base();
        set.insert("extensions/sample.md", "# Sample\n\nUses SHA2_X.\n");
        let v = only(&set);
        assert_eq!(v.rule, Rule::StateVocabulary);
        assert_eq!(
            v.message,
            "`SHA2_X` is neither a KERNEL §10 state nor in the vocabulary"
        );
    }

    #[test]
    fn background_ratio_floor_applies() {
        let mut set = base();
        let kernel = set.doc(KERNEL).unwrap().to_owned();
        let long = kernel.replacen("## 11.", &format!("{}## 11.", "Filler.\n".repeat(1000)), 1);
        set.insert(KERNEL, &long);
        let v = only(&set);
        assert_eq!(v.file, "AUTOBOT-KERNEL.background.md");
        assert!(v.message.contains("50 lines against 1018"), "{}", v.message);
    }

    #[test]
    fn pointer_without_background_and_orphan_background_fail() {
        let mut set = base();
        set.remove("AUTOBOT-KERNEL.background.md");
        let v = only(&set);
        assert_eq!(v.file, KERNEL);
        assert!(v.message.contains("does not exist"), "{}", v.message);
        let mut set = base();
        set.insert("AUTOBOT-THESIS.background.md", &"Why.\n".repeat(60));
        set.remove("AUTOBOT-THESIS.md");
        let v = only(&set);
        assert_eq!(v.message, "has no main file AUTOBOT-THESIS.md");
    }

    #[test]
    fn links_resolve_inside_the_set_only() {
        let mut set = base();
        for (target, want) in [
            ("../AUTOBOT-KERNEL.md#10", None),
            ("#top", None),
            ("missing.md", Some("resolves to no file")),
            ("https://example.com", Some("leaves the design set")),
            ("../../x.md", Some("leaves the design set")),
        ] {
            set.insert(
                "extensions/sample.md",
                &format!("# Sample\n\n[k]({target}).\n"),
            );
            let got = check(&set).unwrap();
            match want {
                None => assert_eq!(got, [], "{target}"),
                Some(w) => {
                    assert_eq!(got.len(), 1, "{target}: {got:?}");
                    assert_eq!(got[0].message, format!("the link `{target}` {w}"));
                }
            }
        }
        // A link inside a code span or a fenced block is not a link.
        set.insert(
            "extensions/sample.md",
            "# S\n\n`[k](x.md)`\n\n```\n[k](y.md)\n```\n",
        );
        assert_eq!(check(&set).unwrap(), []);
        // Tilde fences, and a longer fence around an inner three-backtick pair, hide links too.
        set.insert(
            "extensions/sample.md",
            "# S\n\n~~~\n[x](../../outside.md)\n~~~\n\n````\n```\n[y](nowhere.md)\n```\n````\n\n\
             [z](gone.md)\n",
        );
        let got = check(&set).unwrap();
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].message, "the link `gone.md` resolves to no file");
        // A code span closes only on a backtick run of its own length, and an unmatched
        // run is literal, so neither hides the link after it.
        for (line, target) in [
            ("``a`b`` [hid](nowhere.md)", "nowhere.md"),
            ("it's a `stray backtick [x](gone.md)", "gone.md"),
        ] {
            set.insert("extensions/sample.md", &format!("# S\n\n{line}\n"));
            let got = check(&set).unwrap();
            assert_eq!(got.len(), 1, "{line}: {got:?}");
            assert_eq!(
                got[0].message,
                format!("the link `{target}` resolves to no file")
            );
        }
        // A double-backtick span that holds a single backtick still hides its link.
        set.insert("extensions/sample.md", "# S\n\n`` a ` [k](x.md) ``\n");
        assert_eq!(check(&set).unwrap(), []);
    }

    #[test]
    fn missing_kernel_is_an_error() {
        let mut set = base();
        set.remove(KERNEL);
        assert!(check(&set).is_err());
    }
}
