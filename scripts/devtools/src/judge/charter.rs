//! The `judged` entries of a charter.
//!
//! An entry table is a pipe table whose header names an `Id` and a `Mode` column; its second
//! column is the entry's statement. A row whose Mode cell is `judged` (backticks optional) is a
//! judged entry. Its table must also have a `Threshold` column, whose cell holds the decimal
//! confidence, from 0 to 1, at or above which a "violates" answer blocks; a judged row without
//! one is an error.

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

/// Every `judged` entry of `charter`, in document order.
///
/// # Errors
/// Fails when a judged row has no Threshold cell or a threshold that is not a decimal in
/// `0..=1`, or when two judged entries share an id.
pub fn judged_entries(charter: &str) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    let mut ids = BTreeSet::new();
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
            let cell = |i: usize| row.get(i).map_or("", String::as_str);
            if cell(mode_col).trim_matches('`') != JUDGED {
                continue;
            }
            let id = cell(id_col).to_owned();
            let raw = threshold_col.map_or("", cell);
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
                statement: cell(1).to_owned(),
                threshold,
            });
        }
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
}
