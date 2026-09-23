use autobot_devtools::markdown::{heading_of, section, tables};

const DOC: &str = "# Title\n\nintro\n\n## 1. First\n\nbody one\n\n### 1.1 Sub\n\nsub body\n\n```text\n## not a heading\n```\n\n## 2. Second\n\n| A | B |\n|---|---|\n| x | y |\n\ntail\n";

#[test]
fn section_stops_at_same_level_heading_and_skips_fences() {
    let body = section(DOC, "1. First").unwrap();
    assert!(body.contains("body one"));
    assert!(body.contains("### 1.1 Sub"));
    assert!(body.contains("## not a heading"));
    assert!(!body.contains("## 2. Second"));
}

#[test]
fn section_runs_to_end_and_misses_cleanly() {
    assert!(section(DOC, "2. Second").unwrap().contains("tail"));
    assert!(section(DOC, "3. Missing").is_none());
}

#[test]
fn heading_requires_space_and_level_range() {
    assert_eq!(heading_of("## 4. Manager"), Some((2, "4. Manager")));
    assert_eq!(heading_of("##no-space"), None);
    assert_eq!(heading_of("####### seven"), None);
}

#[test]
fn tables_drop_separator_rows() {
    let t = tables(section(DOC, "2. Second").unwrap());
    assert_eq!(
        t,
        vec![vec![
            vec!["A".to_owned(), "B".to_owned()],
            vec!["x".to_owned(), "y".to_owned()]
        ]]
    );
}

#[test]
fn tilde_and_longer_fences_hide_headings() {
    let doc = "## A\n\n~~~\n## hidden\n~~~\n\n````\n```\n## also hidden\n````\n\n## B\n";
    let a = section(doc, "A").unwrap();
    assert!(a.contains("## hidden"));
    assert!(a.contains("## also hidden"));
    assert!(!a.contains("## B"));
}

#[test]
fn tables_skip_fenced_blocks() {
    let t =
        tables("```\n| a | b |\n|---|---|\n| 1 | 2 |\n```\n\n| k | v |\n|---|---|\n| 1 | 2 |\n");
    assert_eq!(t.len(), 1);
    assert_eq!(t[0][0], vec!["k".to_owned(), "v".to_owned()]);
}

#[test]
fn cells_split_on_every_unescaped_pipe_like_github() {
    // GitHub splits `a|b` inside a code span, and renders `\|` as `|` even inside code.
    let t = tables("| k | v |\n|---|---|\n| `a|b` | c |\n| `x\\|y` | z |\n");
    assert_eq!(
        t[0][1],
        vec!["`a".to_owned(), "b`".to_owned(), "c".to_owned()]
    );
    assert_eq!(t[0][2], vec!["`x|y`".to_owned(), "z".to_owned()]);
}

#[test]
fn outer_pipes_are_optional() {
    let t = tables("a | b\n--|:-:\n| 1 | 2\n3 | 4 |\n");
    assert_eq!(
        t[0],
        vec![
            vec!["a".to_owned(), "b".to_owned()],
            vec!["1".to_owned(), "2".to_owned()],
            vec!["3".to_owned(), "4".to_owned()],
        ]
    );
}

#[test]
fn pipe_lines_without_a_delimiter_row_are_not_a_table() {
    assert!(tables("| a | b |\n| 1 | 2 |\n").is_empty());
    // A delimiter row with a different cell count does not start a table either.
    assert!(tables("| a | b |\n|---|\n").is_empty());
}

#[test]
fn only_the_second_row_is_a_delimiter() {
    let t = tables("| a | b |\n|---|---|\n| - | - |\n");
    assert_eq!(
        t[0],
        vec![
            vec!["a".to_owned(), "b".to_owned()],
            vec!["-".to_owned(), "-".to_owned()]
        ]
    );
}

#[test]
fn a_blank_line_ends_the_table() {
    let t = tables("| a |\n|---|\n| 1 |\n\n| 2 |\n");
    assert_eq!(t, vec![vec![vec!["a".to_owned()], vec!["1".to_owned()]]]);
}
