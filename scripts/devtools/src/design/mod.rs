//! The design set under `docs/design`: its consistency check and the KERNEL §10 parser.

pub mod check;
pub mod lifecycle;

/// Whether `line` opens or closes a fenced code block.
pub(crate) fn is_fence(line: &str) -> bool {
    line.trim_start().starts_with("```")
}
