//! The rows of a `judge` report comment and how each compares with the review, as the parent
//! module docs describe it.

use crate::markdown::tables;

/// The answer of one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// "complies".
    Complies,
    /// "violates".
    Violates,
    /// "unsure".
    Unsure,
    /// Anything else: not asked, not answered, no answer or malformed.
    None,
}

/// One row of the report table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The charter entry id.
    pub entry: String,
    /// The answer.
    pub answer: Answer,
    /// Whether the row's Result is `blocks`.
    pub blocks: bool,
}

/// The rows of the first table in `report` whose header has `Entry`, `Answer` and `Result`
/// columns; empty when there is none.
#[must_use]
pub fn rows(report: &str) -> Vec<Row> {
    for table in tables(report) {
        let Some((header, rows)) = table.split_first() else {
            continue;
        };
        let column = |name: &str| header.iter().position(|cell| cell == name);
        let (Some(entry), Some(answer), Some(result)) =
            (column("Entry"), column("Answer"), column("Result"))
        else {
            continue;
        };
        return rows
            .iter()
            .filter_map(|row| {
                let answer = match row.get(answer)?.as_str() {
                    "complies" => Answer::Complies,
                    "violates" => Answer::Violates,
                    "unsure" => Answer::Unsure,
                    _ => Answer::None,
                };
                Some(Row {
                    entry: row.get(entry)?.clone(),
                    answer,
                    blocks: row.get(result).is_some_and(|r| r == "blocks"),
                })
            })
            .collect();
    }
    Vec::new()
}

/// Whether `text` names the entry `id` as a whole word: the characters around it are neither
/// ASCII alphanumerics nor hyphens.
#[must_use]
pub fn names(text: &str, id: &str) -> bool {
    if id.is_empty() {
        return false;
    }
    let word = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-');
    text.match_indices(id).any(|(at, _)| {
        !word(text[..at].chars().next_back()) && !word(text[at + id.len()..].chars().next())
    })
}

/// How a judge answer compares with the review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Call {
    /// "violates", and review found the entry.
    Hit,
    /// "violates", and review did not find the entry.
    FalseAlarm,
    /// "complies" or "unsure", and review found the entry.
    Miss,
    /// "complies" or "unsure", and review did not find the entry.
    Agree,
    /// The service did not answer.
    Unanswered,
}

/// The call for `row` when review `found` its entry or not.
#[must_use]
pub fn call(row: &Row, found: bool) -> Call {
    match (row.answer, found) {
        (Answer::Violates, true) => Call::Hit,
        (Answer::Violates, false) => Call::FalseAlarm,
        (Answer::Complies | Answer::Unsure, true) => Call::Miss,
        (Answer::Complies | Answer::Unsure, false) => Call::Agree,
        (Answer::None, _) => Call::Unanswered,
    }
}
