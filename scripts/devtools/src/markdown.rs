//! Reading sections and tables out of Markdown documents.

/// The body of the first section whose heading text starts with `heading`, at any level.
///
/// The body runs until the next heading of the same or a higher level; it excludes the
/// heading line itself. Headings inside fenced code blocks are ignored.
#[must_use]
pub fn section<'a>(doc: &'a str, heading: &str) -> Option<&'a str> {
    let mut start = None;
    let mut level = 0;
    let mut offset = 0;
    let mut fenced = false;
    for line in doc.split_inclusive('\n') {
        let trimmed = line.trim_end();
        if trimmed.starts_with("```") {
            fenced = !fenced;
        } else if !fenced && let Some((l, text)) = heading_of(trimmed) {
            match start {
                None if text.starts_with(heading) => {
                    start = Some(offset + line.len());
                    level = l;
                }
                Some(s) if l <= level => return Some(&doc[s..offset]),
                _ => {}
            }
        }
        offset += line.len();
    }
    start.map(|s| &doc[s..])
}

/// Splits a heading line into its level and text, or `None` if the line is not a heading.
#[must_use]
pub fn heading_of(line: &str) -> Option<(usize, &str)> {
    let level = line.bytes().take_while(|&b| b == b'#').count();
    if (1..=6).contains(&level) && line[level..].starts_with(' ') {
        Some((level, line[level..].trim()))
    } else {
        None
    }
}

/// Every pipe table in `text`, each as rows of trimmed cells, without the separator row.
#[must_use]
pub fn tables(text: &str) -> Vec<Vec<Vec<String>>> {
    let mut out = Vec::new();
    let mut current: Vec<Vec<String>> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('|') && line.ends_with('|') && line.len() > 1 {
            let cells: Vec<String> = line[1..line.len() - 1]
                .split('|')
                .map(|c| c.trim().to_owned())
                .collect();
            let separator = cells
                .iter()
                .all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':')));
            if !separator {
                current.push(cells);
            }
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}
