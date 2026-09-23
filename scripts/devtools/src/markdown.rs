//! Reading sections and tables out of Markdown documents.

/// Tracks whether the current line is inside a fenced code block.
///
/// A fence opens with three or more backticks or tildes (at most three spaces of indent) and
/// closes with a line of the same character, at least as long, and nothing else.
#[derive(Debug, Default)]
struct Fence {
    open: Option<(u8, usize)>,
}

impl Fence {
    /// Feeds one line; returns `true` if the line is fence markup or fenced content.
    fn step(&mut self, line: &str) -> bool {
        let body = line.trim_end();
        let indent = body.len() - body.trim_start_matches(' ').len();
        let body = body.trim_start_matches(' ');
        let run = |c: u8| body.bytes().take_while(|&b| b == c).count();
        match self.open {
            Some((c, len)) => {
                if indent <= 3 && run(c) >= len && body.bytes().all(|b| b == c) {
                    self.open = None;
                }
                true
            }
            None => {
                if indent > 3 {
                    return false;
                }
                for c in *b"`~" {
                    let len = run(c);
                    if len >= 3 && !(c == b'`' && body[len..].contains('`')) {
                        self.open = Some((c, len));
                        return true;
                    }
                }
                false
            }
        }
    }
}

/// The body of the first section whose heading text starts with `heading`, at any level.
///
/// The body runs until the next heading of the same or a higher level; it excludes the
/// heading line itself. Headings inside fenced code blocks are ignored.
#[must_use]
pub fn section<'a>(doc: &'a str, heading: &str) -> Option<&'a str> {
    let mut start = None;
    let mut level = 0;
    let mut offset = 0;
    let mut fence = Fence::default();
    for line in doc.split_inclusive('\n') {
        if !fence.step(line)
            && let Some((l, text)) = heading_of(line.trim_end())
        {
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

/// Every pipe table outside fenced code in `text`, each as rows of trimmed cells, without the
/// delimiter row (the second row of a table, made of dashes and colons).
///
/// Cells are split on unescaped `|` outside code spans; `\\|` yields a literal `|`. A code span
/// is a backtick run closed by a later run of the same length; an unmatched run is literal text.
#[must_use]
pub fn tables(text: &str) -> Vec<Vec<Vec<String>>> {
    let mut out = Vec::new();
    let mut current: Vec<Vec<String>> = Vec::new();
    let mut delimiter_seen = false;
    let mut fence = Fence::default();
    for line in text.lines() {
        let fenced = fence.step(line);
        let line = line.trim();
        if !fenced && line.starts_with('|') && line.ends_with('|') && line.len() > 1 {
            let cells = split_cells(&line[1..line.len() - 1]);
            let delimiter = current.len() == 1
                && !delimiter_seen
                && cells
                    .iter()
                    .all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':')));
            if delimiter {
                delimiter_seen = true;
            } else {
                current.push(cells);
            }
        } else {
            delimiter_seen = false;
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Splits the inside of a table row into trimmed cells.
fn split_cells(row: &str) -> Vec<String> {
    let bytes = row.as_bytes();
    let run_at = |i: usize| bytes[i..].iter().take_while(|&&b| b == b'`').count();
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if bytes.get(i + 1) == Some(&b'|') => {
                cell.push('|');
                i += 2;
            }
            b'`' => {
                let len = run_at(i);
                // Find a closing run of exactly the same length.
                let mut j = i + len;
                let mut close = None;
                while j < bytes.len() {
                    if bytes[j] == b'`' {
                        let l = run_at(j);
                        if l == len {
                            close = Some(j);
                            break;
                        }
                        j += l;
                    } else {
                        j += 1;
                    }
                }
                let end = close.map_or(i + len, |c| c + len);
                cell.push_str(&row[i..end]);
                i = end;
            }
            b'|' => {
                cells.push(std::mem::take(&mut cell).trim().to_owned());
                i += 1;
            }
            _ => {
                let ch = row[i..].chars().next().unwrap_or_default();
                cell.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    cells.push(cell.trim().to_owned());
    cells
}
