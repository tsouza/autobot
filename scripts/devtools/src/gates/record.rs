//! The gate record manifest: `docs/gates/<gate>/record.json`.

use crate::{Error, Result};
use autobot_kernel::types::Digest;
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::fmt;

/// The record schema version [`Record::parse`] accepts.
pub const RECORD_VERSION: u64 = 1;

/// The file name of a gate record inside its gate directory.
pub const RECORD_FILE: &str = "record.json";

/// A state of a gate (M0 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GateState {
    /// No evidence has been collected: the state of a gate without a record.
    NotRun,
    /// Evidence is being collected.
    Running,
    /// The evidence passed.
    Passed,
    /// The evidence failed.
    Failed,
    /// The evidence no longer applies.
    Invalidated,
}

impl GateState {
    /// Every state, in the order M0 §4 lists them.
    pub const ALL: [Self; 5] = [
        Self::NotRun,
        Self::Running,
        Self::Passed,
        Self::Failed,
        Self::Invalidated,
    ];

    /// The state's name as M0 §4 writes it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::NotRun => "NOT_RUN",
            Self::Running => "RUNNING",
            Self::Passed => "PASSED",
            Self::Failed => "FAILED",
            Self::Invalidated => "INVALIDATED",
        }
    }
}

impl fmt::Display for GateState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One of the four things gate evidence is bound to (M0 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Binding {
    /// The installation: the deployment manifests.
    Installation,
    /// The software: the workspace crates, their lock file and the toolchain.
    Software,
    /// The policy: the repository charter and the dependency policy.
    Policy,
    /// The profile: the kernel's digest of the M0 profile.
    Profile,
}

impl Binding {
    /// Every binding, in the order M0 §4 lists them.
    pub const ALL: [Self; 4] = [
        Self::Installation,
        Self::Software,
        Self::Policy,
        Self::Profile,
    ];

    /// The binding's key in a record's `digests` object.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Installation => "installation",
            Self::Software => "software",
            Self::Policy => "policy",
            Self::Profile => "profile",
        }
    }
}

impl fmt::Display for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// One digest per [`Binding`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Digests {
    /// The installation digest.
    pub installation: Digest,
    /// The software digest.
    pub software: Digest,
    /// The policy digest.
    pub policy: Digest,
    /// The profile digest.
    pub profile: Digest,
}

impl Digests {
    /// The digest of `binding`.
    #[must_use]
    pub fn get(&self, binding: Binding) -> Digest {
        match binding {
            Binding::Installation => self.installation,
            Binding::Software => self.software,
            Binding::Policy => self.policy,
            Binding::Profile => self.profile,
        }
    }

    /// The bindings whose digest in `self` differs from the one in `other`, in [`Binding::ALL`]
    /// order.
    #[must_use]
    pub fn differing(&self, other: &Self) -> Vec<Binding> {
        Binding::ALL
            .into_iter()
            .filter(|&b| self.get(b) != other.get(b))
            .collect()
    }
}

/// One evidence artifact a record lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    /// The artifact's path, relative to the gate directory.
    pub path: String,
    /// The SHA-256 of the artifact's bytes.
    pub digest: Digest,
}

/// A parsed, checked gate record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// The gate id, such as `G-QUAL`; equal to the name of the record's directory.
    pub gate: String,
    /// The recorded state; never [`GateState::NotRun`].
    pub state: GateState,
    /// The digests the evidence is bound to.
    pub digests: Digests,
    /// The evidence artifacts; at least one when the state is [`GateState::Passed`].
    pub artifacts: Vec<Artifact>,
}

impl Record {
    /// Parses and checks the record manifest `json`.
    ///
    /// The manifest is one JSON object with exactly the keys `version` ([`RECORD_VERSION`]),
    /// `gate`, `state` (a [`GateState`] name other than `NOT_RUN`), `digests` (an object with
    /// exactly one `sha256:<hex>` digest per [`Binding::key`]) and `artifacts` (an array of
    /// objects with exactly `path` and `digest`).
    ///
    /// # Errors
    /// [`Error::Parse`] if `json` is not such an object; if an artifact path is empty, absolute,
    /// names `.` or `..`, repeats, or is [`RECORD_FILE`]; or if a `PASSED` record lists no
    /// artifact.
    pub fn parse(json: &str) -> Result<Self> {
        let value: Value = serde_json::from_str(json).map_err(|e| invalid(&e.to_string()))?;
        let top = object(
            &value,
            "the record",
            &["version", "gate", "state", "digests", "artifacts"],
        )?;
        let version = top["version"].as_u64();
        if version != Some(RECORD_VERSION) {
            return Err(invalid(&format!(
                "version is {}, not {RECORD_VERSION}",
                top["version"]
            )));
        }
        let gate = string(&top["gate"], "gate")?.to_owned();
        let state_name = string(&top["state"], "state")?;
        let state = GateState::ALL
            .into_iter()
            .find(|s| s.name() == state_name)
            .ok_or_else(|| invalid(&format!("state `{state_name}` is not a gate state")))?;
        if state == GateState::NotRun {
            return Err(invalid(
                "state NOT_RUN is the state of a gate without a record",
            ));
        }
        let keys = Binding::ALL.map(Binding::key);
        let digests = object(&top["digests"], "digests", &keys)?;
        let at = |b: Binding| digest(&digests[b.key()], &format!("digests.{b}"));
        let digests = Digests {
            installation: at(Binding::Installation)?,
            software: at(Binding::Software)?,
            policy: at(Binding::Policy)?,
            profile: at(Binding::Profile)?,
        };
        let list = top["artifacts"]
            .as_array()
            .ok_or_else(|| invalid("artifacts is not an array"))?;
        let mut seen = BTreeSet::new();
        let mut artifacts = Vec::with_capacity(list.len());
        for item in list {
            let fields = object(item, "an artifact", &["path", "digest"])?;
            let path = string(&fields["path"], "artifact path")?;
            check_artifact_path(path)?;
            if !seen.insert(path) {
                return Err(invalid(&format!("artifact `{path}` is listed twice")));
            }
            artifacts.push(Artifact {
                path: path.to_owned(),
                digest: digest(&fields["digest"], &format!("digest of `{path}`"))?,
            });
        }
        if state == GateState::Passed && artifacts.is_empty() {
            return Err(invalid("a PASSED record lists no artifact"));
        }
        Ok(Self {
            gate,
            state,
            digests,
            artifacts,
        })
    }
}

fn invalid(msg: &str) -> Error {
    Error::Parse(format!("gate record: {msg}"))
}

/// The members of `value`, which must be an object with exactly the keys `keys`.
fn object<'a>(value: &'a Value, what: &str, keys: &[&str]) -> Result<&'a Map<String, Value>> {
    let map = value
        .as_object()
        .ok_or_else(|| invalid(&format!("{what} is not an object")))?;
    if let Some(extra) = map.keys().find(|k| !keys.contains(&k.as_str())) {
        return Err(invalid(&format!("{what} has the unknown key `{extra}`")));
    }
    if let Some(missing) = keys.iter().find(|k| !map.contains_key(**k)) {
        return Err(invalid(&format!("{what} has no `{missing}`")));
    }
    Ok(map)
}

fn string<'a>(value: &'a Value, what: &str) -> Result<&'a str> {
    value
        .as_str()
        .ok_or_else(|| invalid(&format!("{what} is not a string")))
}

fn digest(value: &Value, what: &str) -> Result<Digest> {
    let text = string(value, what)?;
    text.parse().map_err(|_| {
        invalid(&format!(
            "{what} `{text}` is not `sha256:` and 64 lowercase hex digits"
        ))
    })
}

/// Refuses an artifact path that could name something outside the gate directory, or the record.
fn check_artifact_path(path: &str) -> Result<()> {
    let normal = !path.contains('\\')
        && path
            .split('/')
            .all(|s| !s.is_empty() && s != "." && s != "..");
    if !normal {
        return Err(invalid(&format!(
            "artifact path `{path}` is not a relative path of plain components"
        )));
    }
    if path == RECORD_FILE {
        return Err(invalid("the record cannot list itself as an artifact"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const D1: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
    const D2: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";

    fn manifest(state: &str, artifacts: &str) -> String {
        format!(
            r#"{{"version": 1, "gate": "G-QUAL", "state": "{state}",
              "digests": {{"installation": "{D1}", "software": "{D1}", "policy": "{D1}", "profile": "{D2}"}},
              "artifacts": {artifacts}}}"#
        )
    }

    #[test]
    fn parses_a_passed_record() {
        let r = Record::parse(&manifest(
            "PASSED",
            &format!(r#"[{{"path": "evidence/sim.txt", "digest": "{D2}"}}]"#),
        ))
        .unwrap();
        assert_eq!(r.gate, "G-QUAL");
        assert_eq!(r.state, GateState::Passed);
        assert_eq!(r.digests.installation.to_string(), D1);
        assert_eq!(r.digests.profile.to_string(), D2);
        assert_eq!(
            r.artifacts,
            [Artifact {
                path: "evidence/sim.txt".to_owned(),
                digest: D2.parse().unwrap()
            }]
        );
    }

    #[test]
    fn a_running_record_may_list_no_artifact_but_a_passed_one_may_not() {
        assert_eq!(
            Record::parse(&manifest("RUNNING", "[]")).unwrap().state,
            GateState::Running
        );
        let err = Record::parse(&manifest("PASSED", "[]")).unwrap_err();
        assert!(err.to_string().contains("lists no artifact"), "{err}");
    }

    #[test]
    fn refuses_not_run_and_unknown_states() {
        for (state, msg) in [
            ("NOT_RUN", "without a record"),
            ("DONE", "not a gate state"),
        ] {
            let err = Record::parse(&manifest(state, "[]")).unwrap_err();
            assert!(err.to_string().contains(msg), "{state}: {err}");
        }
    }

    #[test]
    fn refuses_unknown_missing_and_malformed_fields() {
        let good = manifest("RUNNING", "[]");
        for (text, msg) in [
            (
                good.replacen("\"version\": 1", "\"version\": 2", 1),
                "version is 2",
            ),
            (
                good.replacen("\"gate\"", "\"extra\": 0, \"gate\"", 1),
                "unknown key `extra`",
            ),
            (
                good.replacen("\"policy\": ", "\"poliCy\": ", 1),
                "unknown key `poliCy`",
            ),
            (good.replacen(D2, "sha256:22", 1), "is not `sha256:`"),
            (
                good.replacen(&format!(", \"profile\": \"{D2}\""), "", 1),
                "has no `profile`",
            ),
            ("[]".to_owned(), "the record is not an object"),
        ] {
            let err = Record::parse(&text).unwrap_err();
            assert!(err.to_string().contains(msg), "{text}: {err}");
        }
    }

    #[test]
    fn refuses_artifact_paths_outside_the_gate_directory_or_repeated() {
        for (path, msg) in [
            ("../x", "plain components"),
            ("/etc/passwd", "plain components"),
            ("a/./b", "plain components"),
            ("", "plain components"),
            ("a\\..\\b", "plain components"),
            ("record.json", "list itself"),
        ] {
            let list = format!(
                r#"[{{"path": "{}", "digest": "{D1}"}}]"#,
                path.replace('\\', "\\\\")
            );
            let err = Record::parse(&manifest("FAILED", &list)).unwrap_err();
            assert!(err.to_string().contains(msg), "{path}: {err}");
        }
        let twice =
            format!(r#"[{{"path": "a", "digest": "{D1}"}}, {{"path": "a", "digest": "{D2}"}}]"#);
        let err = Record::parse(&manifest("FAILED", &twice)).unwrap_err();
        assert!(err.to_string().contains("listed twice"), "{err}");
    }

    #[test]
    fn differing_names_each_binding_whose_digest_changed() {
        let a = Record::parse(&manifest("RUNNING", "[]")).unwrap().digests;
        assert!(a.differing(&a).is_empty());
        let mut b = a;
        b.installation = D2.parse().unwrap();
        b.profile = D1.parse().unwrap();
        assert_eq!(a.differing(&b), [Binding::Installation, Binding::Profile]);
    }
}
