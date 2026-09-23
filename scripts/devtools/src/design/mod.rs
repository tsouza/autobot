//! The design set under `docs/design`: its consistency check, the retired-term check, the traceability lint and the KERNEL §10 parser.

pub mod check;
pub mod lifecycle;
pub mod retired;
pub mod trace;

/// Tracks whether the current line is inside a fenced code block.
///
/// A fence opens with three or more backticks or tildes (at most three spaces of indent) and
/// closes with a line of the same character, at least as long, and nothing else. These are the
/// semantics of the private fence tracker in `crate::markdown`, which `markdown::section`
/// follows, so the check and the section reader agree on what is fenced.
#[derive(Debug, Default)]
struct Fence {
    open: Option<(u8, usize)>,
}

impl Fence {
    /// Whether a fenced code block is open after the lines fed so far.
    fn is_open(&self) -> bool {
        self.open.is_some()
    }

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

#[cfg(test)]
mod tests {
    use super::Fence;

    fn fenced(doc: &str) -> Vec<bool> {
        let mut fence = Fence::default();
        doc.lines().map(|l| fence.step(l)).collect()
    }

    #[test]
    fn fence_closes_only_on_a_run_of_the_same_character_at_least_as_long() {
        let doc = "a\n````md\n```\nx\n```\n~~~\n````\nb\n~~~\ny\n```\n~~~~\nc\n";
        assert_eq!(
            fenced(doc),
            [
                false, true, true, true, true, true, true, false, true, true, true, true, false
            ]
        );
    }

    #[test]
    fn fence_ignores_deep_indent_and_backtick_info_strings() {
        let doc = "    ```\na\n```x`y\nb\n   ```\nc\n   ```\nd\n";
        assert_eq!(
            fenced(doc),
            [false, false, false, false, true, true, true, false]
        );
    }

    #[test]
    fn fence_reports_whether_a_block_is_open() {
        let mut fence = Fence::default();
        assert!(!fence.is_open());
        fence.step("~~~");
        assert!(fence.is_open());
        fence.step("~~");
        assert!(fence.is_open());
        fence.step("~~~~");
        assert!(!fence.is_open());
    }
}
