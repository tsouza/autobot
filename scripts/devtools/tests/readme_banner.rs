use autobot_devtools::markdown::Fence;

const README: &str = include_str!("../../../README.md");
const BANNER: &str = include_str!("../../../assets/banner.txt");

/// The content of the fenced code block the document opens with, one `\n`-terminated line per
/// line, or `None` when the first line does not open a fence or the fence never closes.
fn opening_block(doc: &str) -> Option<String> {
    let mut lines = doc.lines();
    let mut fence = Fence::default();
    if !fence.step(lines.next()?) || !fence.is_open() {
        return None;
    }
    let mut body = String::new();
    for line in lines {
        fence.step(line);
        if !fence.is_open() {
            return Some(body);
        }
        body.push_str(line);
        body.push('\n');
    }
    None
}

#[test]
fn the_readme_opens_with_the_banner() {
    assert_eq!(opening_block(README).as_deref(), Some(BANNER));
}

#[test]
fn an_edited_readme_banner_is_a_difference() {
    let edited = README.replacen('■', "#", 1);
    assert_ne!(opening_block(&edited).as_deref(), Some(BANNER));
}

#[test]
fn an_edited_banner_file_is_a_difference() {
    let shorter: String = BANNER.lines().take(2).map(|l| format!("{l}\n")).collect();
    assert_ne!(opening_block(README).as_deref(), Some(shorter.as_str()));
}

#[test]
fn a_readme_not_opening_with_a_block_has_no_banner() {
    assert_eq!(opening_block("# AutoBot\n\n```text\nx\n```\n"), None);
    assert_eq!(opening_block("```text\nx\n"), None);
    assert_eq!(opening_block("```text\nx\n```\n").as_deref(), Some("x\n"));
}
