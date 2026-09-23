//! Globs over `/`-separated repository paths.
//!
//! A glob is split into segments at `/`. A `**` segment matches any number of whole segments,
//! none included. Inside any other segment `*` matches any run of characters and `?` exactly
//! one, and every other character matches itself. A glob ending in `/` names everything under
//! that directory, as if it ended in `/**`, and a leading `./` or `/` is ignored. A glob matches
//! a path only as a whole: `scripts/*.rs` matches `scripts/judge.rs` and not
//! `scripts/devtools/src/lib.rs`.

/// The glob `token` in the form [`matches()`] reads, or `None` when it is not a glob: it is
/// empty, holds whitespace, has no literal character (such as `*` or `**/*`), or starts with a
/// `**` segment (such as `**/*.rs`); each would allow every path, or every path of a kind.
#[must_use]
pub fn parse(token: &str) -> Option<String> {
    let token = token.trim();
    let token = token.strip_prefix("./").unwrap_or(token);
    let token = token.trim_start_matches('/');
    let literal = token.chars().any(|c| !matches!(c, '*' | '?' | '/'));
    let universal = token == "**" || token.starts_with("**/");
    if token.is_empty() || !literal || universal || token.chars().any(char::is_whitespace) {
        return None;
    }
    Some(if token.ends_with('/') {
        format!("{token}**")
    } else {
        token.to_owned()
    })
}

/// Whether `glob`, as [`parse`] returns it, matches the whole of `path`.
#[must_use]
pub fn matches(glob: &str, path: &str) -> bool {
    let glob: Vec<Vec<char>> = glob.split('/').map(|s| s.chars().collect()).collect();
    let path: Vec<Vec<char>> = path.split('/').map(|s| s.chars().collect()).collect();
    segments(&glob, &path)
}

fn segments(glob: &[Vec<char>], path: &[Vec<char>]) -> bool {
    match glob.split_first() {
        None => path.is_empty(),
        Some((seg, rest)) if seg.as_slice() == ['*', '*'] => {
            (0..=path.len()).any(|skip| segments(rest, &path[skip..]))
        }
        Some((seg, rest)) => path
            .split_first()
            .is_some_and(|(name, tail)| segment(seg, name) && segments(rest, tail)),
    }
}

fn segment(pattern: &[char], name: &[char]) -> bool {
    match pattern.split_first() {
        None => name.is_empty(),
        Some(('*', rest)) => (0..=name.len()).any(|skip| segment(rest, &name[skip..])),
        Some(('?', rest)) => !name.is_empty() && segment(rest, &name[1..]),
        Some((c, rest)) => name.first() == Some(c) && segment(rest, &name[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allows(glob: &str, path: &str) -> bool {
        matches(&parse(glob).unwrap(), path)
    }

    #[test]
    fn double_star_spans_whole_segments_and_star_stays_inside_one() {
        assert!(allows(".github/workflows/**", ".github/workflows/ci.yml"));
        assert!(allows(".github/workflows/**", ".github/workflows/a/b.yml"));
        assert!(!allows(
            ".github/workflows/**",
            ".github/rulesets/main.json"
        ));
        assert!(allows("scripts/*.rs", "scripts/judge.rs"));
        assert!(!allows("scripts/*.rs", "scripts/devtools/src/lib.rs"));
        assert!(!allows("scripts/*.rs", "scripts/README.md"));
        assert!(allows("crates/**/tests/*.rs", "crates/tests/a.rs"));
        assert!(allows("crates/**/tests/*.rs", "crates/k/x/tests/a.rs"));
        assert!(allows(
            "docs/design/AUTOBOT-M0-AND-GATES*.md",
            "docs/design/AUTOBOT-M0-AND-GATES.background.md"
        ));
        assert!(allows("a/?.rs", "a/b.rs"));
        assert!(!allows("a/?.rs", "a/bc.rs"));
    }

    #[test]
    fn a_plain_path_matches_only_itself_and_a_trailing_slash_means_everything_under() {
        assert!(allows("CHARTER.md", "CHARTER.md"));
        assert!(!allows("CHARTER.md", "docs/CHARTER.md"));
        assert!(!allows("docs", "docs/a.md"));
        assert!(allows("docs/", "docs/a.md"));
        assert!(allows("./Justfile", "Justfile"));
        assert!(allows("/Justfile", "Justfile"));
        assert!(!allows(
            "scripts/devtools/src/judge.rs",
            "scripts/devtools/src/judge/x.rs"
        ));
    }

    #[test]
    fn tokens_that_are_not_globs_are_refused() {
        for token in [
            "",
            "  ",
            "*",
            "**",
            "**/*",
            "**/*.rs",
            "**/tests/*.md",
            "*/",
            "a b",
            "CONTRIBUTING.md (a section)",
        ] {
            assert_eq!(parse(token), None, "{token:?}");
        }
    }
}
