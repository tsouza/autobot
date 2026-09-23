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

/// Every pipe table outside fenced code in `text`, as rows of trimmed cells without the delimiter
/// row.
///
/// This is the subset of GitHub Flavored Markdown tables that the design documents use, not a
/// full implementation: a table is a line containing `|` followed by a delimiter row with the
/// same number of cells, and it runs until a blank line or a line without `|`. Outer pipes are
/// optional. Cells split on every `|` not escaped as `\\|` (including inside code spans), and
/// `\\|` yields a literal `|`. Rows are returned as written, without padding or truncation.
#[must_use]
pub fn tables(text: &str) -> Vec<Vec<Vec<String>>> {
    let mut out = Vec::new();
    let mut fence = Fence::default();
    let lines: Vec<(bool, &str)> = text.lines().map(|l| (fence.step(l), l)).collect();
    let mut i = 0;
    while i + 1 < lines.len() {
        let (fenced, line) = lines[i];
        let (next_fenced, next) = lines[i + 1];
        let header = split_row(line);
        let is_table = !fenced
            && !next_fenced
            && line.contains('|')
            && is_delimiter(next)
            && split_row(next).len() == header.len();
        if !is_table {
            i += 1;
            continue;
        }
        let mut table = vec![header];
        i += 2;
        while i < lines.len() {
            let (fenced, line) = lines[i];
            if fenced || line.trim().is_empty() || !line.contains('|') {
                break;
            }
            table.push(split_row(line));
            i += 1;
        }
        out.push(table);
    }
    out
}

/// Whether `line` is a table delimiter row: cells of dashes with optional edge colons.
fn is_delimiter(line: &str) -> bool {
    line.contains('-')
        && split_row(line).iter().all(|c| {
            let c = c.trim_start_matches(':').trim_end_matches(':');
            !c.is_empty() && c.bytes().all(|b| b == b'-')
        })
}

/// Splits one table row into trimmed cells, dropping optional outer pipes.
fn split_row(line: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut chars = line.trim().chars().peekable();
    if chars.peek() == Some(&'|') {
        chars.next();
    }
    let mut trailing_pipe = false;
    while let Some(ch) = chars.next() {
        trailing_pipe = false;
        match ch {
            '\\' if chars.peek() == Some(&'|') => {
                cell.push('|');
                chars.next();
            }
            '|' => {
                cells.push(std::mem::take(&mut cell).trim().to_owned());
                trailing_pipe = true;
            }
            _ => cell.push(ch),
        }
    }
    if !trailing_pipe || !cell.trim().is_empty() {
        cells.push(cell.trim().to_owned());
    }
    cells
}
