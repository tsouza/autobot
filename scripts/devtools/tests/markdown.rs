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
