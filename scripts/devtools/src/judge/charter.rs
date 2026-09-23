//! The entries of a charter and their enforcement modes.
//!
//! An entry table is a pipe table whose header names an `Id` and a `Mode` column; its second
//! column is the entry's statement, and its Mode cell (backticks optional) the entry's mode
//! ([`modes`]). A row whose mode is `judged` is a judged entry ([`judged_entries`]). Its table
//! must also have a `Threshold` column, whose cell holds the decimal confidence, from 0 to 1,
//! at or above which a "violates" answer blocks; a judged row without one is an error.

use crate::markdown::tables;
use crate::{Error, Result};
use std::collections::BTreeSet;

/// The Mode cell of a judged entry, without backticks.
pub const JUDGED: &str = "judged";

/// One `judged` charter entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// The stable id, such as `L-1`.
    pub id: String,
    /// The statement the entry makes, as written in the charter.
    pub statement: String,
    /// The confidence at or above which a "violates" answer blocks, in `0..=1`.
    pub threshold: f64,
}

/// One row of an entry table: its id, statement, mode without backticks and Threshold cell
/// (empty when the table has no Threshold column).
struct Row {
    id: String,
    statement: String,
    mode: String,
    threshold: String,
}

/// Every row of every entry table of `charter`, in document order.
fn rows(charter: &str) -> Vec<Row> {
    let mut out = Vec::new();
    for table in tables(charter) {
        let Some((header, rows)) = table.split_first() else {
            continue;
        };
        let column = |name: &str| header.iter().position(|cell| cell == name);
        let (Some(id_col), Some(mode_col)) = (column("Id"), column("Mode")) else {
            continue;
        };
        let threshold_col = column("Threshold");
        for row in rows {
            let cell = |i: usize| row.get(i).map_or("", String::as_str).to_owned();
            out.push(Row {
                id: cell(id_col),
                statement: cell(1),
                mode: cell(mode_col).trim_matches('`').to_owned(),
                threshold: threshold_col.map_or_else(String::new, cell),
            });
        }
    }
    out
}

/// Every entry of `charter` as its id and mode (`mechanical`, `review`, `judged`,
/// `advisory`), in document order.
#[must_use]
pub fn modes(charter: &str) -> Vec<(String, String)> {
    rows(charter).into_iter().map(|r| (r.id, r.mode)).collect()
}

/// Every `judged` entry of `charter`, in document order.
///
/// # Errors
/// Fails when a judged row has no Threshold cell or a threshold that is not a decimal in
/// `0..=1`, or when two judged entries share an id.
pub fn judged_entries(charter: &str) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    let mut ids = BTreeSet::new();
    for row in rows(charter).into_iter().filter(|r| r.mode == JUDGED) {
        let (id, raw) = (row.id, row.threshold);
        let threshold = raw
            .parse::<f64>()
            .ok()
            .filter(|t| (0.0..=1.0).contains(t))
            .ok_or_else(|| {
                Error::Parse(format!(
                    "charter entry {id}: threshold `{raw}` is not a decimal from 0 to 1"
                ))
            })?;
        if !ids.insert(id.clone()) {
            return Err(Error::Parse(format!("charter entry {id} is judged twice")));
        }
        entries.push(Entry {
            id,
            statement: row.statement,
            threshold,
        });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
| Id | Law | Mode | Threshold |
|---|---|---|---|
| L-1 | No comparisons. | `judged` | 0.8 |
| L-2 | Checked by code. | `mechanical` | |

| Id | Rule | Mode | Threshold |
|---|---|---|---|
| R-1 | One task, one pull request. | judged | 1 |

| Mode | Meaning |
|---|---|
| `judged` | A typed question. |
";

    #[test]
    fn reads_judged_rows_of_every_entry_table_and_nothing_else() {
        let entries = judged_entries(SAMPLE).unwrap();
        assert_eq!(
            entries,
            [
                Entry {
                    id: "L-1".into(),
                    statement: "No comparisons.".into(),
                    threshold: 0.8,
                },
                Entry {
                    id: "R-1".into(),
                    statement: "One task, one pull request.".into(),
                    threshold: 1.0,
                },
            ]
        );
    }

    #[test]
    fn rejects_a_missing_or_out_of_range_threshold() {
        for (table, raw) in [
            (
                "| Id | Law | Mode | Threshold |\n|---|---|---|---|\n| L-1 | x | `judged` | |\n",
                "",
            ),
            (
                "| Id | Law | Mode | Threshold |\n|---|---|---|---|\n| L-1 | x | `judged` | 1.5 |\n",
                "1.5",
            ),
            (
                "| Id | Law | Mode | Threshold |\n|---|---|---|---|\n| L-1 | x | `judged` | high |\n",
                "high",
            ),
            (
                "| Id | Law | Mode |\n|---|---|---|\n| L-1 | x | `judged` |\n",
                "",
            ),
        ] {
            let err = judged_entries(table).unwrap_err().to_string();
            assert!(
                err.contains(&format!("L-1: threshold `{raw}`")),
                "{table}: {err}"
            );
        }
    }

    #[test]
    fn rejects_a_duplicate_id() {
        let doc = "| Id | Law | Mode | Threshold |\n|---|---|---|---|\n| L-1 | a | `judged` | 0.5 |\n| L-1 | b | `judged` | 0.5 |\n";
        let err = judged_entries(doc).unwrap_err().to_string();
        assert!(err.contains("L-1 is judged twice"), "{err}");
    }

    #[test]
    fn finds_the_judged_entries_of_the_committed_charter() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../CHARTER.md");
        let charter = std::fs::read_to_string(path).unwrap();
        let entries = judged_entries(&charter).unwrap();
        // An independent count: every table row whose cells include `judged`, outside the
        // mode table (whose first cell is the mode itself).
        let judged_rows: Vec<&str> = charter
            .lines()
            .filter(|l| l.contains("| `judged` |") && !l.starts_with("| `judged`"))
            .collect();
        assert!(!entries.is_empty(), "the charter has no judged entry");
        assert_eq!(entries.len(), judged_rows.len(), "{judged_rows:?}");
        for (entry, row) in entries.iter().zip(&judged_rows) {
            assert!(row.starts_with(&format!("| {} |", entry.id)), "{row}");
            assert!(row.contains(&entry.statement), "{row}");
            let last = row.trim().trim_end_matches('|').rsplit('|').next();
            let threshold = last.map(|c| c.trim().parse::<f64>());
            assert_eq!(threshold, Some(Ok(entry.threshold)), "{row}");
        }
    }

    #[test]
    fn modes_reads_every_entry_row_and_skips_the_mode_table() {
        let got = modes(SAMPLE);
        let got: Vec<(&str, &str)> = got.iter().map(|(i, m)| (i.as_str(), m.as_str())).collect();
        assert_eq!(
            got,
            [("L-1", "judged"), ("L-2", "mechanical"), ("R-1", "judged")]
        );
    }

    #[test]
    fn the_committed_charter_checks_scope_by_code_and_judges_one_task_per_pull_request() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../CHARTER.md");
        let charter = std::fs::read_to_string(path).unwrap();
        let mode = |id: &str| {
            modes(&charter)
                .into_iter()
                .find_map(|(i, m)| (i == id).then_some(m))
        };
        assert_eq!(mode("R-1").as_deref(), Some("mechanical"));
        assert_eq!(mode("R-2").as_deref(), Some(JUDGED));
        assert_eq!(mode("L-2").as_deref(), Some("review"));
        let r2 = judged_entries(&charter)
            .unwrap()
            .into_iter()
            .find(|e| e.id == "R-2")
            .unwrap();
        assert_eq!(r2.statement, "A pull request delivers exactly one task.");
        assert_eq!(r2.threshold, 0.8);
    }
}
