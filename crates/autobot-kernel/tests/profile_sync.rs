//! Keeps `profiles/m0.toml` equal to its single design home, the table of
//! `docs/design/AUTOBOT-M0-AND-GATES.md` §2.
//!
//! Each row of the table must contain the phrases [`claims`] renders from the profile for that
//! area, each running to the end of a clause ([`find_clause`]); the table must have exactly the
//! areas [`claims`] names; and every number in a row, meaning every digit run, even one joined to
//! letters, outside the names in [`NAMES`], and the words one to ten, must lie inside one of those
//! phrases. A failure means the two disagree; the design is the authority, so the fix is a
//! `design` finding that decides which one is wrong, not an edit of either side to match.

use autobot_devtools::markdown::{section, tables};
use autobot_kernel::profile::{
    ControlWork, Profile, ProfileValues, SandboxEgress, SandboxImage, SandboxModelApi,
    SandboxNetwork, SandboxOs,
};
use std::collections::BTreeSet;

const DESIGN: &str = include_str!("../../../docs/design/AUTOBOT-M0-AND-GATES.md");
const M0: &str = include_str!("../../../profiles/m0.toml");
const SECTION: &str = "2. M0 profile";

/// Words in the table that contain digits but are names, not numbers.
const NAMES: [&str; 2] = ["M0", "S3"];

/// Characters that end a clause of a table cell. A comma does not: it also separates list items,
/// so a list rendered with fewer items would end at one.
const SEPARATORS: [char; 3] = [';', '.', '—'];

/// Number words the table may use.
const WORDS: [&str; 10] = [
    "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
];

/// `n` followed by `singular` or `plural`.
fn count(n: impl Into<u64>, singular: &str, plural: &str) -> String {
    let n = n.into();
    format!("{n} {}", if n == 1 { singular } else { plural })
}

/// `n` as a word, as the table spells out one to ten; any other number in digits.
fn word(n: impl Into<u32>) -> String {
    let n: u32 = n.into();
    usize::try_from(n)
        .ok()
        .and_then(|i| i.checked_sub(1))
        .and_then(|i| WORDS.get(i))
        .map_or_else(|| n.to_string(), |w| (*w).to_owned())
}

/// Items joined as an English list with `conjunction`: `a`, `a and b`, `a, b and c`.
fn english(items: &[&str], conjunction: &str) -> String {
    match items.split_last() {
        None => String::new(),
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => format!("{} {conjunction} {last}", rest.join(", ")),
    }
}

/// The phrases each area row of the table must contain, rendered from the profile.
fn claims(v: &ProfileValues) -> Vec<(&'static str, Vec<String>)> {
    let o = &v.objects;
    let a = &v.api;
    let s = &v.sandbox;
    let d = &v.evidence.defect_maturity_days;
    let reserved: Vec<&str> = a
        .reserved_control
        .iter()
        .map(|c| match c {
            ControlWork::Hold => "hold",
            ControlWork::Fence => "fence",
            ControlWork::ReceiptRepair => "receipt repair",
            _ => "an unknown control class",
        })
        .collect();
    let reserved = if reserved.is_empty() {
        "no reserved control capacity".to_owned()
    } else {
        format!(
            "reserved control capacity for {}",
            english(&reserved, "and")
        )
    };
    let slow = if d.compatibility_risk == d.security_or_data_integrity {
        format!(
            "({} for `SECURITY_OR_DATA_INTEGRITY` and `COMPATIBILITY_RISK`)",
            d.security_or_data_integrity
        )
    } else {
        format!(
            "({} for `SECURITY_OR_DATA_INTEGRITY`, {} for `COMPATIBILITY_RISK`)",
            d.security_or_data_integrity, d.compatibility_risk
        )
    };
    let mounts = [
        ("host mount", s.host_mounts),
        ("privileged container", s.privileged),
        ("device", s.devices),
    ];
    let denied: Vec<&str> = mounts.iter().filter(|c| !c.1).map(|c| c.0).collect();
    let os = match s.os {
        SandboxOs::Linux => "Linux",
        _ => "an unknown OS",
    };
    let image = match s.image {
        SandboxImage::PinnedOci => "pinned OCI image",
        _ => "an unknown image policy",
    };
    let network = match s.network {
        SandboxNetwork::DefaultDeny => "default-deny network",
        _ => "an unknown network default",
    };
    let egress = match s.egress {
        SandboxEgress::BrokerOnly => "broker-only egress",
        _ => "an unknown egress route",
    };
    let closed = if denied.is_empty() {
        "host mounts, privileged containers and devices allowed".to_owned()
    } else {
        format!("no {}", english(&denied, "or"))
    };
    let model_api = match s.model_api {
        SandboxModelApi::MeteredProxy => {
            "optional live model API only through a metered proxy that cannot reach production endpoints"
        }
        SandboxModelApi::Disabled => "no live model API",
        _ => "an unknown model API access",
    };
    vec![
        (
            "Replay window",
            vec![format!(
                "{}, plus {} clock and transport margin",
                count(v.replay.window_days.get(), "day", "days"),
                count(v.replay.margin_days, "day", "days"),
            )],
        ),
        (
            "Control-receipt ring",
            vec![format!(
                "{}, ≤ {} KiB each",
                count(
                    v.control_ring.entries.get(),
                    "unpublished entry",
                    "unpublished entries"
                ),
                v.control_ring.entry_max_kib,
            )],
        ),
        (
            "Dispatch ledger",
            vec![count(v.dispatch_ledger.entries.get(), "entry", "entries")],
        ),
        (
            "Registers",
            vec![format!(
                "{}, {} per context",
                count(v.registers.plans.get(), "plan", "plans"),
                count(
                    v.registers.integration_bases.get(),
                    "integration base",
                    "integration bases"
                ),
            )],
        ),
        (
            "Scale",
            vec![
                format!(
                    "{}, {}, {}, {}",
                    count(v.scale.contexts.get(), "context", "contexts"),
                    count(v.scale.repositories.get(), "repository", "repositories"),
                    count(v.scale.tasks.get(), "task", "tasks"),
                    count(
                        v.scale.active_task_runs.get(),
                        "active TaskRun",
                        "active TaskRuns"
                    ),
                ),
                format!(
                    "{} simulated Managers and {} simulated installation identities to exercise races",
                    word(v.scale.simulated_managers),
                    word(v.scale.simulated_installation_identities),
                ),
            ],
        ),
        (
            "Objects",
            vec![
                format!("status ≤ {} KiB", o.status_max_kib),
                format!("pending slot ≤ {} KiB", o.pending_slot_max_kib),
                format!(
                    "≤ {} effect intents per command",
                    o.effect_intents_per_command
                ),
                format!(
                    "late-event buffer {} per projected aggregate",
                    o.late_event_buffer
                ),
            ],
        ),
        (
            "API budget",
            vec![
                format!(
                    "{} requests/s, burst {}, per operator process",
                    a.requests_per_second, a.burst
                ),
                format!(
                    "queue of {} keys, FIFO within priority, {reserved}",
                    a.queue_keys
                ),
            ],
        ),
        (
            "Checkpoint cadence",
            vec![
                format!(
                    "at most {} s of active work",
                    v.checkpoint.max_active_work_secs
                ),
                format!(
                    "an indivisible write capped at {} s",
                    v.checkpoint.indivisible_write_max_secs
                ),
            ],
        ),
        (
            "RPO / RTO",
            vec![
                format!(
                    "at most the uncheckpointed {} s plus {} bounded in-flight operation, with \
                     `last_verified_checkpoint_age` and `at_risk_interval` exposed",
                    v.checkpoint.max_active_work_secs,
                    word(v.recovery.in_flight_operations),
                ),
                format!(
                    "restore of a {} MiB fixture within {} minutes with healthy dependencies",
                    v.recovery.restore_fixture_mib, v.recovery.restore_within_minutes,
                ),
            ],
        ),
        (
            "Artifact fixture",
            vec![format!(
                "≤ {} GiB per workspace",
                v.artifacts.workspace_max_gib
            )],
        ),
        (
            "Evidence",
            vec![
                format!(
                    "TTL {} h, shortened by repository policy",
                    v.evidence.ttl_hours
                ),
                format!(
                    "no-test exception expiry {}",
                    count(v.evidence.no_test_expiry_days.get(), "day", "days")
                ),
                format!("defect maturity {} days {slow}", d.reversible),
            ],
        ),
        (
            "Liveness bounds",
            vec!["the rest of FORMAL §6 are fixture constants chosen per fixture".to_owned()],
        ),
        (
            "Sandbox",
            vec![
                format!(
                    "{os}, {image}, {network}, {egress}, {} writable mount, {closed}",
                    word(s.writable_mounts)
                ),
                model_api.to_owned(),
            ],
        ),
    ]
}

/// Byte ranges of the numbers in `text`: every run of digits, wherever it stands in a word (`60`,
/// `60s`, `x2`), except inside a name in [`NAMES`], and every number word in [`WORDS`].
fn numbers(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_alphanumeric() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
            i += 1;
        }
        let token = &text[start..i];
        if WORDS.contains(&token.to_lowercase().as_str()) {
            out.push((start, i));
        } else if !NAMES.contains(&token) {
            let mut j = start;
            while j < i {
                if bytes[j].is_ascii_digit() {
                    let run = j;
                    while j < i && bytes[j].is_ascii_digit() {
                        j += 1;
                    }
                    out.push((run, j));
                } else {
                    j += 1;
                }
            }
        }
    }
    out
}

/// Where `phrase` occurs in `cell` running to the end of a clause: it starts at the start of the
/// cell or after a character that is not a letter or digit, and it ends at the end of the cell or
/// before optional spaces and a clause separator in [`SEPARATORS`]. A phrase that stops short of
/// the end of its clause, such as a list rendered with fewer items, therefore does not match.
fn find_clause(cell: &str, phrase: &str) -> Option<usize> {
    cell.match_indices(phrase).map(|(at, _)| at).find(|&at| {
        let starts = cell[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        let rest = cell[at + phrase.len()..].trim_start_matches(' ');
        let ends = rest.is_empty() || rest.starts_with(SEPARATORS);
        starts && ends
    })
}

/// Every disagreement between `design`, a copy of the M0 design document, and `profile`.
fn disagreements(design: &str, profile: &ProfileValues) -> Vec<String> {
    let Some(body) = section(design, SECTION) else {
        return vec![format!("the design has no section `{SECTION}`")];
    };
    let Some(table) = tables(body).into_iter().next() else {
        return vec![format!("section `{SECTION}` has no table")];
    };
    let rows: Vec<(&str, &str)> = table
        .iter()
        .skip(1)
        .map(|r| {
            (
                r.first().map_or("", String::as_str),
                r.get(1).map_or("", String::as_str),
            )
        })
        .collect();
    let claims = claims(profile);
    let mut out = Vec::new();
    let named: BTreeSet<&str> = claims.iter().map(|c| c.0).collect();
    let present: BTreeSet<&str> = rows.iter().map(|r| r.0).collect();
    for area in named.difference(&present) {
        out.push(format!("the table has no `{area}` row"));
    }
    for area in present.difference(&named) {
        out.push(format!("the profile has no area for the `{area}` row"));
    }
    for (area, cell) in rows {
        let Some((_, phrases)) = claims.iter().find(|c| c.0 == area) else {
            continue;
        };
        let mut covered = Vec::new();
        for phrase in phrases {
            match find_clause(cell, phrase) {
                Some(at) => covered.push((at, at + phrase.len())),
                None => out.push(format!("{area}: the table does not say `{phrase}`")),
            }
        }
        for (start, end) in numbers(cell) {
            if !covered.iter().any(|&(s, e)| s <= start && end <= e) {
                out.push(format!(
                    "{area}: `{}` in the table is not a profile value",
                    &cell[start..end]
                ));
            }
        }
    }
    out
}

fn m0() -> ProfileValues {
    match Profile::parse(M0) {
        Ok(p) => p.values().clone(),
        Err(e) => panic!("profiles/m0.toml does not parse: {e}"),
    }
}

/// `DESIGN` with `from`, which must occur in §2, replaced by `to`.
fn edited_design(from: &str, to: &str) -> String {
    let at = DESIGN
        .find(SECTION)
        .expect("the design has the profile section");
    let offset = DESIGN[at..]
        .find(from)
        .expect("the profile section has the text");
    let mut out = DESIGN.to_owned();
    out.replace_range(at + offset..at + offset + from.len(), to);
    out
}

#[test]
fn m0_profile_matches_the_design_table() {
    let found = disagreements(DESIGN, &m0());
    assert!(
        found.is_empty(),
        "profiles/m0.toml and M0 §2 disagree; file a `finding` + `design` + `needs-decision` \
         issue under E-M0-DESIGN-CLOSURE instead of editing either side to match:\n{}",
        found.join("\n")
    );
}

#[test]
fn an_edited_value_in_the_design_is_a_disagreement() {
    let found = disagreements(&edited_design("| 64 entries |", "| 65 entries |"), &m0());
    assert_eq!(
        found,
        [
            "Dispatch ledger: the table does not say `64 entries`",
            "Dispatch ledger: `65` in the table is not a profile value",
        ]
    );
}

#[test]
fn a_value_added_to_the_design_is_a_disagreement() {
    let found = disagreements(
        &edited_design(
            "drops nothing accepted",
            "drops nothing accepted, 3 retries",
        ),
        &m0(),
    );
    assert_eq!(
        found,
        ["API budget: `3` in the table is not a profile value"]
    );
    let found = disagreements(
        &edited_design(
            "production endpoints",
            "production endpoints; two scratch mounts",
        ),
        &m0(),
    );
    assert_eq!(
        found,
        ["Sandbox: `two` in the table is not a profile value"]
    );
}

#[test]
fn an_edited_phrase_in_the_design_is_a_disagreement() {
    let found = disagreements(
        &edited_design("default-deny network", "default-allow network"),
        &m0(),
    );
    assert_eq!(
        found,
        [
            "Sandbox: the table does not say `Linux, pinned OCI image, default-deny network, broker-only egress, one writable mount, no host mount, privileged container or device`",
            "Sandbox: `one` in the table is not a profile value",
        ]
    );
}

#[test]
fn an_added_or_removed_row_is_a_disagreement() {
    let added = edited_design(
        "| Dispatch ledger |",
        "| Watch cache | 5 minutes |\n| Dispatch ledger |",
    );
    let found = disagreements(&added, &m0());
    assert_eq!(found, ["the profile has no area for the `Watch cache` row"]);
    let removed = edited_design("| Dispatch ledger | 64 entries |\n", "");
    let found = disagreements(&removed, &m0());
    assert_eq!(found, ["the table has no `Dispatch ledger` row"]);
}

#[test]
fn an_edited_profile_is_a_disagreement() {
    let edited = M0.replacen("burst = 20", "burst = 25", 1);
    let values = match Profile::parse(&edited) {
        Ok(p) => p.values().clone(),
        Err(e) => panic!("{e}"),
    };
    let found = disagreements(DESIGN, &values);
    assert_eq!(
        found,
        [
            "API budget: the table does not say `10 requests/s, burst 25, per operator process`",
            "API budget: `10` in the table is not a profile value",
            "API budget: `20` in the table is not a profile value",
        ]
    );
}

#[test]
fn a_missing_section_or_table_is_a_disagreement() {
    let renamed = DESIGN.replacen("## 2. M0 profile", "## 2. Limits", 1);
    assert_eq!(
        disagreements(&renamed, &m0()),
        ["the design has no section `2. M0 profile`"]
    );
}

#[test]
fn a_number_glued_to_letters_is_a_disagreement() {
    for added in ["60s", "5min", "2x", "x2", "v1beta2"] {
        let found = disagreements(
            &edited_design(
                "drops nothing accepted",
                &format!("drops nothing accepted, retries {added}"),
            ),
            &m0(),
        );
        let digits: Vec<&str> = added
            .split(|c: char| !c.is_ascii_digit())
            .filter(|s| !s.is_empty())
            .collect();
        let expected: Vec<String> = digits
            .iter()
            .map(|d| format!("API budget: `{d}` in the table is not a profile value"))
            .collect();
        assert_eq!(found, expected, "added `{added}`");
    }
}

#[test]
fn names_with_digits_are_not_numbers() {
    assert_eq!(numbers("an S3-compatible store during M0"), []);
    assert_eq!(numbers("S3 at 4S3"), [(6, 7), (8, 9)]);
}

#[test]
fn a_phrase_matches_only_a_whole_clause() {
    let full = "queue of 500 keys, FIFO within priority, reserved control capacity for hold, fence and receipt repair";
    let cell = format!("{full}; a full queue stops admission");
    assert_eq!(find_clause(&cell, full), Some(0));
    for short in [
        "queue of 500 keys",
        "queue of 500 keys, FIFO within priority, reserved control capacity for",
        "queue of 500 keys, FIFO within priority, reserved control capacity for hold",
        "queue of 500 keys, FIFO within priority, reserved control capacity for hold, fence",
        &full[1..],
        "queue of 50",
    ] {
        assert_eq!(find_clause(&cell, short), None, "`{short}` matched");
    }
}

#[test]
fn a_shortened_list_in_the_design_is_a_disagreement() {
    let found = disagreements(
        &edited_design("hold, fence and receipt repair", "hold and fence"),
        &m0(),
    );
    assert_eq!(
        found,
        [
            "API budget: the table does not say `queue of 500 keys, FIFO within priority, reserved control capacity for hold, fence and receipt repair`",
            "API budget: `500` in the table is not a profile value",
        ]
    );
}

#[test]
fn a_profile_with_fewer_reserved_classes_is_a_disagreement() {
    let edited = M0.replacen(
        "[\"hold\", \"fence\", \"receipt-repair\"]",
        "[\"hold\", \"fence\"]",
        1,
    );
    let values = match Profile::parse(&edited) {
        Ok(p) => p.values().clone(),
        Err(e) => panic!("{e}"),
    };
    assert_eq!(
        disagreements(DESIGN, &values),
        [
            "API budget: the table does not say `queue of 500 keys, FIFO within priority, reserved control capacity for hold and fence`",
            "API budget: `500` in the table is not a profile value",
        ]
    );
}

#[test]
fn an_empty_or_repeating_reserved_set_is_refused() {
    for set in ["[]", "[\"hold\", \"hold\", \"fence\", \"receipt-repair\"]"] {
        let edited = M0.replacen("[\"hold\", \"fence\", \"receipt-repair\"]", set, 1);
        assert!(Profile::parse(&edited).is_err(), "accepted {set}");
    }
}

#[test]
fn a_profile_listing_only_the_first_reserved_class_is_a_disagreement() {
    // The rendered phrase is then a prefix of the design's clause, ending before `, fence`.
    let edited = M0.replacen("[\"hold\", \"fence\", \"receipt-repair\"]", "[\"hold\"]", 1);
    let values = match Profile::parse(&edited) {
        Ok(p) => p.values().clone(),
        Err(e) => panic!("{e}"),
    };
    assert_eq!(
        disagreements(DESIGN, &values),
        [
            "API budget: the table does not say `queue of 500 keys, FIFO within priority, reserved control capacity for hold`",
            "API budget: `500` in the table is not a profile value",
        ]
    );
}
