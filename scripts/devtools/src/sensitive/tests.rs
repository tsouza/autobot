use super::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Serves recorded GET responses by path and fails any other path with 404.
struct Recorded(BTreeMap<String, Value>);

impl Api for Recorded {
    fn request(&self, method: Method, path: &str, _body: Option<&Value>) -> Result<Value> {
        assert_eq!(method, Method::Get, "the screen only reads");
        self.0
            .get(path)
            .cloned()
            .ok_or_else(|| Error::Http(format!("Get {path}: http status: 404")))
    }
}

/// The seeded term, invented for these tests.
const TERM: &str = "quokka-[0-9]+";

/// Pull request #50 with the seeded term's match `quokka-42` (in any case) in its title, the
/// third line of its body, the second line of one commit message, a changed path, one added
/// line, one removed line and one context line; shaped like GET pulls/{n},
/// pulls/{n}/commits and pulls/{n}/files, keeping the fields read.
fn seeded() -> Recorded {
    Recorded(BTreeMap::from([
        (
            "pulls/50".to_owned(),
            json!({"title": "feat: add QUOKKA-42 support", "body": "Closes #9\n\nsee quokka-42\n"}),
        ),
        (
            "pulls/50/commits?per_page=100&page=1".to_owned(),
            json!([
                {"sha": "0123456789abcdef0123", "commit": {"message": "feat: a\n\nno term here"}},
                {"sha": "fedcba9876543210fedc", "commit": {"message": "fix: b\nquokka-42 again"}},
            ]),
        ),
        (
            "pulls/50/files?per_page=100&page=1".to_owned(),
            json!([
                {"filename": "docs/a.md", "status": "modified", "changes": 3,
                 "patch": "@@ -10,3 +10,3 @@\n context quokka-42\n-removed quokka-42\n+clean\n+added Quokka-7\n"},
                {"filename": "fixtures/quokka-1.txt", "status": "added", "changes": 1, "patch": "@@ -0,0 +1 @@\n+quokka-1"},
                {"filename": "logo.png", "status": "added", "changes": 0},
                {"filename": "big.json", "status": "modified", "changes": 90000},
            ]),
        ),
    ]))
}

#[test]
fn a_seeded_term_fails_the_check_and_is_never_printed() {
    let report = evaluate(&seeded(), 50, &terms(TERM).unwrap()).unwrap();
    assert_eq!(
        report.matches,
        [
            Location::Title,
            Location::Body(3),
            Location::Commit("fedcba987654".to_owned(), 2),
            Location::Added("docs/a.md".to_owned(), 12),
            Location::Path(2),
            Location::Added("changed file 2".to_owned(), 1),
        ]
    );
    assert_eq!(report.exit_code(), ExitCode::FAILURE);
    let text = report.to_string();
    for place in [
        "sensitive-terms: the pull request title matches a term\n",
        "sensitive-terms: line 3 of the pull request body matches a term\n",
        "sensitive-terms: line 2 of the message of commit fedcba987654 matches a term\n",
        "sensitive-terms: docs/a.md:12, an added line matches a term\n",
        "sensitive-terms: the path of changed file 2 matches a term\n",
        "sensitive-terms: changed file 2:1, an added line matches a term\n",
        "sensitive-terms: #50 matches a term in 6 place(s)\n",
        "sensitive-terms: big.json has no patch on GitHub; its content is not screened\n",
    ] {
        assert!(text.contains(place), "{place}: {text}");
    }
    // No line of the report holds the term, a match of it, or a path that matches it.
    let lower = text.to_lowercase();
    assert!(!lower.contains("quokka"), "{text}");
    assert!(!lower.contains(TERM), "{text}");
}

#[test]
fn a_pull_request_without_a_match_passes() {
    let report = evaluate(&seeded(), 50, &terms("# a comment\nwombat\n").unwrap()).unwrap();
    assert_eq!(report.matches, []);
    assert_eq!(report.exit_code(), ExitCode::SUCCESS);
    assert!(
        report
            .to_string()
            .ends_with("sensitive-terms: #50 matches no term\n"),
        "{report}"
    );
}

#[test]
fn an_empty_or_comment_only_list_holds_no_term() {
    for list in ["", "  \n", "# only a comment\n\n"] {
        assert!(terms(list).unwrap().is_empty(), "{list:?}");
    }
    assert_eq!(terms("a\n# b\nc").unwrap().len(), 2);
}

#[test]
fn an_invalid_term_is_reported_by_line_number_only() {
    let err = terms("ok\n# note\nsecret-(unclosed\n")
        .unwrap_err()
        .to_string();
    assert_eq!(
        err,
        "parse error: SENSITIVE_TERMS line 3 is not a valid regular expression"
    );
}

#[test]
fn added_lines_carry_their_new_file_line_numbers() {
    let patch = "@@ -1,2 +1,3 @@\n a\n+b\n-c\n+d\n@@ -40 +41,2 @@\n+e\n \n+f";
    assert_eq!(added(patch), [(2, "b"), (3, "d"), (41, "e"), (43, "f")]);
}

#[test]
fn a_failed_read_is_an_error() {
    let mut api = seeded();
    api.0.remove("pulls/50/commits?per_page=100&page=1");
    assert!(evaluate(&api, 50, &terms(TERM).unwrap()).is_err());
}

#[test]
fn a_list_github_cut_fails_the_check_and_says_so() {
    let commit = |i: usize| json!({"sha": format!("{i:040x}"), "commit": {"message": "x"}});
    let mut api = seeded();
    for (page, range) in [(1, 0..100), (2, 100..200), (3, 200..250)] {
        api.0.insert(
            format!("pulls/50/commits?per_page=100&page={page}"),
            range.map(commit).collect(),
        );
    }
    let report = evaluate(&api, 50, &terms("wombat").unwrap()).unwrap();
    assert_eq!(report.matches, []);
    assert_eq!(
        report.incomplete,
        ["GitHub lists at most 250 commits of #50"]
    );
    assert_eq!(report.exit_code(), ExitCode::FAILURE);
    assert!(
        report.to_string().contains(
            "sensitive-terms: GitHub lists at most 250 commits of #50; the rest is not screened\n"
        ),
        "{report}"
    );

    let file = |i: usize| json!({"filename": format!("f{i}.txt"), "status": "added", "changes": 1, "patch": "+x"});
    let mut api = seeded();
    for page in 0..30 {
        api.0.insert(
            format!("pulls/50/files?per_page=100&page={}", page + 1),
            (page * 100..(page + 1) * 100).map(file).collect(),
        );
    }
    api.0
        .insert("pulls/50/files?per_page=100&page=31".to_owned(), json!([]));
    let report = evaluate(&api, 50, &terms("wombat").unwrap()).unwrap();
    assert_eq!(
        report.incomplete,
        ["GitHub lists at most 3000 files of #50"]
    );
    assert_eq!(report.exit_code(), ExitCode::FAILURE);
}
