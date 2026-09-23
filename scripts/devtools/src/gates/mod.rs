//! Gate records, behind `just gate-status` and `just gate-evidence <gate>`.
//!
//! A gate record is the manifest `docs/gates/<gate>/record.json` ([`Record`]) with the
//! evidence artifacts it lists beside it. A gate is one row of the gate table of
//! `docs/design/AUTOBOT-M0-AND-GATES.md` §4, named by its gate id (`G-QUAL` for
//! `M0-Q / G-QUAL`); a gate without a record directory is [`GateState::NotRun`]. Records are
//! repository files, not a kind: there is no Gate custom resource in M0 (M0 §5).
//!
//! - [`status`] reads every record and recomputes the current [`Digests`]. A record in
//!   [`GateState::Passed`] whose digests differ from the current ones is reported as
//!   [`GateState::Invalidated`], naming each differing [`Binding`]; any other record is
//!   reported in its recorded state. It writes nothing, and it does not check signatures.
//! - [`verify`] checks one record: every listed artifact exists with its digest, and an
//!   SSH-signed annotated tag named `gate/<gate>/<anything>` points at the record commit and
//!   passes `git verify-tag`. The record commit is the last commit that changed the gate
//!   directory, and the directory must have no uncommitted or untracked change. `git
//!   verify-tag` checks the signature against the verifier's own `gpg.ssh.allowedSignersFile`;
//!   this module does no cryptography of its own.
//!
//! Choices this module makes where the design is open:
//!
//! - Each binding is computed from fixed repository paths ([`Binding::sources`]): the
//!   installation from the deployment manifests, the software from the workspace crates,
//!   manifests, lock file and toolchain file, the policy from the charter and the dependency
//!   policy, and the profile as the kernel's [`Profile`] digest of [`PROFILE`]. A file binding's
//!   digest is [`files_digest`] over the tracked files under its paths, read from the working
//!   tree.
//! - Only a `PASSED` record is reported `INVALIDATED` when a digest changes; a record may also
//!   carry `INVALIDATED` itself. `NOT_RUN` is never written in a record.
//! - `just gate-status` fails only on a record it cannot read or a record directory that names
//!   no gate; an invalidated gate is printed, not a failure.
//! - The gate states are those of M0 §4; whether KERNEL §10 must print them is the open design
//!   finding #319, and #226 concerns the sentence of M0 §4 stating the current gate state,
//!   which this module derives from the records instead.

mod record;

pub use record::{Artifact, Binding, Digests, GateState, RECORD_FILE, RECORD_VERSION, Record};

use crate::markdown;
use crate::process::Cmd;
use crate::{Error, Result};
use autobot_kernel::profile::Profile;
use autobot_kernel::types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

/// The directory holding one directory per recorded gate, relative to the repository root.
pub const GATES_DIR: &str = "docs/gates";
/// The design document whose §4 table lists the gates, relative to the repository root.
pub const M0_DESIGN: &str = "docs/design/AUTOBOT-M0-AND-GATES.md";
/// The heading of the section holding the gate table.
pub const GATE_SECTION: &str = "4. Milestones and gates";
/// The profile the profile binding is computed from, relative to the repository root.
pub const PROFILE: &str = "profiles/m0.toml";
/// The prefix of a gate record tag, followed by `<gate>/`.
pub const TAG_PREFIX: &str = "gate/";
/// The armor line that starts an SSH signature in a tag object.
const SSH_SIGNATURE: &str = "-----BEGIN SSH SIGNATURE-----";

impl Binding {
    /// The repository paths the binding's current digest is computed from.
    #[must_use]
    pub fn sources(self) -> &'static [&'static str] {
        match self {
            Self::Installation => &["deploy"],
            Self::Software => &["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "crates"],
            Self::Policy => &["CHARTER.md", "deny.toml"],
            Self::Profile => &[PROFILE],
        }
    }
}

/// The SHA-256 of `bytes`.
#[must_use]
pub fn sha256(bytes: &[u8]) -> Digest {
    Digest::from_bytes(Sha256::digest(bytes).into())
}

/// The digest of the tracked files under `paths` in the repository at `root`, read from the
/// working tree.
///
/// It is the SHA-256 of one `<file digest> <path>\n` line per file, sorted by path, where the
/// file digest is [`sha256`] of its bytes, or of its target for a symbolic link. It changes
/// with a file's content, name or presence, and not with untracked files.
///
/// # Errors
/// Fails if git fails, a file cannot be read, or no tracked file is under `paths`.
pub fn files_digest(root: &Path, paths: &[&str]) -> Result<Digest> {
    let listed = Cmd::new("git")
        .args(["ls-files", "-z", "--"])
        .args(paths.iter().copied())
        .current_dir(root)
        .output()?;
    let mut files: Vec<&str> = listed.split('\0').filter(|f| !f.is_empty()).collect();
    if files.is_empty() {
        return Err(Error::Parse(format!(
            "no tracked file under {}",
            paths.join(", ")
        )));
    }
    files.sort_unstable();
    let mut canonical = String::new();
    for file in files {
        let path = root.join(file);
        let read = |e: std::io::Error| Error::Parse(format!("reading {file}: {e}"));
        let meta = std::fs::symlink_metadata(&path).map_err(read)?;
        let bytes = if meta.file_type().is_symlink() {
            std::fs::read_link(&path)
                .map_err(read)?
                .into_os_string()
                .into_encoded_bytes()
        } else {
            std::fs::read(&path).map_err(read)?
        };
        canonical.push_str(&format!("{} {file}\n", sha256(&bytes)));
    }
    Ok(sha256(canonical.as_bytes()))
}

/// The current digest of every binding for the repository at `root`.
///
/// # Errors
/// Fails if a file binding fails ([`files_digest`]) or [`PROFILE`] cannot be read or parsed.
pub fn current_digests(root: &Path) -> Result<Digests> {
    let file = |b: Binding| files_digest(root, b.sources());
    let text = std::fs::read_to_string(root.join(PROFILE))
        .map_err(|e| Error::Parse(format!("reading {PROFILE}: {e}")))?;
    let profile = Profile::parse(&text).map_err(|e| Error::Parse(format!("{PROFILE}: {e}")))?;
    Ok(Digests {
        installation: file(Binding::Installation)?,
        software: file(Binding::Software)?,
        policy: file(Binding::Policy)?,
        profile: Digest::from_bytes(*profile.digest().as_bytes()),
    })
}

/// The gate ids of the §4 gate table of `m0`, the text of [`M0_DESIGN`], in table order: the
/// part of each first cell after its last ` / `, without emphasis.
///
/// # Errors
/// Fails if the section or its table is missing or the table has no gate row.
pub fn gate_ids(m0: &str) -> Result<Vec<String>> {
    let missing = |what: &str| Error::Parse(format!("{M0_DESIGN}: {what}"));
    let section = markdown::section(m0, GATE_SECTION)
        .ok_or_else(|| missing("no section `4. Milestones and gates`"))?;
    let table = markdown::tables(section)
        .into_iter()
        .next()
        .ok_or_else(|| missing("no gate table in §4"))?;
    let ids: Vec<String> = table
        .iter()
        .skip(1)
        .filter_map(|row| row.first())
        .filter_map(|cell| cell.trim_matches('*').rsplit(" / ").next())
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty())
        .collect();
    if ids.is_empty() {
        return Err(missing("the §4 gate table has no row"));
    }
    Ok(ids)
}

/// The state `record` is reported in against the `current` digests, and the bindings that
/// differ when that state is [`GateState::Invalidated`] because of them.
#[must_use]
pub fn reported_state(record: &Record, current: &Digests) -> (GateState, Vec<Binding>) {
    let differing = record.digests.differing(current);
    if record.state == GateState::Passed && !differing.is_empty() {
        (GateState::Invalidated, differing)
    } else {
        (record.state, Vec::new())
    }
}

/// One gate as [`status`] reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The gate id.
    pub gate: String,
    /// The reported state.
    pub state: GateState,
    /// The recorded state, when the gate has a record.
    pub recorded: Option<GateState>,
    /// The bindings that invalidated a `PASSED` record.
    pub differing: Vec<Binding>,
}

/// The current digests and one report per gate of the repository at `root`, in table order.
///
/// # Errors
/// Fails if the design, a record or a binding cannot be read, a record is malformed or names
/// another gate than its directory, or a directory under [`GATES_DIR`] is not a gate id.
pub fn status(root: &Path) -> Result<(Digests, Vec<Report>)> {
    let gates = read_gate_ids(root)?;
    let records = read_records(root, &gates)?;
    let current = current_digests(root)?;
    let reports = gates
        .into_iter()
        .map(|gate| match records.get(&gate) {
            None => Report {
                gate,
                state: GateState::NotRun,
                recorded: None,
                differing: Vec::new(),
            },
            Some(record) => {
                let (state, differing) = reported_state(record, &current);
                Report {
                    gate,
                    state,
                    recorded: Some(record.state),
                    differing,
                }
            }
        })
        .collect();
    Ok((current, reports))
}

/// Prints the current digests and each gate's reported state; `just gate-status`.
///
/// # Errors
/// Fails as [`status`] does.
pub fn run_status(root: &Path) -> Result<ExitCode> {
    let (current, reports) = status(root)?;
    println!("gate-status: current digests");
    for b in Binding::ALL {
        println!("  {:<12} {}", b.key(), current.get(b));
    }
    for r in &reports {
        match (r.recorded, r.differing.is_empty()) {
            (Some(recorded), false) => {
                let names: Vec<&str> = r.differing.iter().map(|b| b.key()).collect();
                println!(
                    "{} {} (recorded {recorded}; {} differ)",
                    r.gate,
                    r.state,
                    names.join(", ")
                );
            }
            _ => println!("{} {}", r.gate, r.state),
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// The outcome of [`verify`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verification {
    /// The tag whose signature verified, if one did.
    pub tag: Option<String>,
    /// Everything that failed; empty when the record verified.
    pub problems: Vec<String>,
}

impl Verification {
    /// Whether the record verified: a tag verified and nothing failed.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.tag.is_some() && self.problems.is_empty()
    }
}

/// Verifies the record of `gate` in the repository at `root`: its artifacts and its signed tag.
///
/// # Errors
/// Fails if `gate` is not a gate of the §4 table, the record cannot be read or is malformed or
/// names another gate, or git fails.
pub fn verify(root: &Path, gate: &str) -> Result<Verification> {
    if !read_gate_ids(root)?.iter().any(|g| g == gate) {
        return Err(Error::Parse(format!(
            "`{gate}` is not a gate of {M0_DESIGN} §4"
        )));
    }
    let dir = format!("{GATES_DIR}/{gate}");
    let record = read_record(root, gate)?;
    let mut problems = Vec::new();
    for a in &record.artifacts {
        match std::fs::read(root.join(&dir).join(&a.path)) {
            Err(e) => problems.push(format!("artifact `{}`: {e}", a.path)),
            Ok(bytes) => {
                let actual = sha256(&bytes);
                if actual != a.digest {
                    problems.push(format!(
                        "artifact `{}` has digest {actual}, the record lists {}",
                        a.path, a.digest
                    ));
                }
            }
        }
    }
    let tag = signed_tag(root, gate, &dir, &mut problems)?;
    Ok(Verification { tag, problems })
}

/// Verifies the record of `gate` and prints the outcome; `just gate-evidence <gate>`.
///
/// # Errors
/// Fails as [`verify`] does.
pub fn run_evidence(root: &Path, gate: &str) -> Result<ExitCode> {
    let v = verify(root, gate)?;
    for p in &v.problems {
        println!("{p}");
    }
    match (&v.tag, v.passed()) {
        (Some(tag), true) => {
            println!("gate-evidence: {gate} verified by tag {tag}");
            Ok(ExitCode::SUCCESS)
        }
        _ => {
            println!("gate-evidence: {gate} does not verify");
            Ok(ExitCode::FAILURE)
        }
    }
}

/// The first tag `gate/<gate>/*` on the record commit whose SSH signature `git verify-tag`
/// accepts; each tag that fails, or the lack of a record commit or tag, is pushed to `problems`.
fn signed_tag(
    root: &Path,
    gate: &str,
    dir: &str,
    problems: &mut Vec<String>,
) -> Result<Option<String>> {
    let git = |args: &[&str]| {
        Cmd::new("git")
            .args(args.iter().copied())
            .current_dir(root)
            .output()
    };
    let dirty = git(&["status", "--porcelain", "--untracked-files=all", "--", dir])?;
    if !dirty.trim().is_empty() {
        problems.push(format!(
            "{dir} has uncommitted changes, so no commit is its record commit"
        ));
        return Ok(None);
    }
    let commit = git(&["log", "-1", "--format=%H", "--", dir])?
        .trim()
        .to_owned();
    if commit.is_empty() {
        problems.push(format!("{dir} is not committed"));
        return Ok(None);
    }
    let pattern = format!("{TAG_PREFIX}{gate}/*");
    let tags = git(&["tag", "--points-at", &commit, "--list", &pattern])?;
    let tags: Vec<&str> = tags
        .lines()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect();
    if tags.is_empty() {
        problems.push(format!(
            "no tag {pattern} points at the record commit {commit}"
        ));
        return Ok(None);
    }
    let mut failures = Vec::new();
    for tag in tags {
        let reference = format!("refs/tags/{tag}");
        if git(&["cat-file", "-t", &reference])?.trim() != "tag" {
            failures.push(format!("tag {tag} is not an annotated tag"));
            continue;
        }
        if !git(&["cat-file", "tag", &reference])?.contains(SSH_SIGNATURE) {
            failures.push(format!("tag {tag} has no SSH signature"));
            continue;
        }
        match git(&["verify-tag", &reference]) {
            Ok(_) => return Ok(Some(tag.to_owned())),
            Err(e) => failures.push(format!("tag {tag} does not verify: {e}")),
        }
    }
    problems.extend(failures);
    Ok(None)
}

fn read_gate_ids(root: &Path) -> Result<Vec<String>> {
    let m0 = std::fs::read_to_string(root.join(M0_DESIGN))
        .map_err(|e| Error::Parse(format!("reading {M0_DESIGN}: {e}")))?;
    gate_ids(&m0)
}

fn read_record(root: &Path, gate: &str) -> Result<Record> {
    let path = format!("{GATES_DIR}/{gate}/{RECORD_FILE}");
    let text = std::fs::read_to_string(root.join(&path))
        .map_err(|e| Error::Parse(format!("reading {path}: {e}")))?;
    let record = Record::parse(&text).map_err(|e| Error::Parse(format!("{path}: {e}")))?;
    if record.gate != gate {
        return Err(Error::Parse(format!(
            "{path}: names gate `{}`, not its directory's `{gate}`",
            record.gate
        )));
    }
    Ok(record)
}

/// The record of every gate directory under [`GATES_DIR`]; files there, such as its README,
/// are not records.
fn read_records(root: &Path, gates: &[String]) -> Result<BTreeMap<String, Record>> {
    let dir = root.join(GATES_DIR);
    let mut records = BTreeMap::new();
    if !dir.exists() {
        return Ok(records);
    }
    let io = |e: std::io::Error| Error::Parse(format!("reading {GATES_DIR}: {e}"));
    for entry in std::fs::read_dir(&dir).map_err(io)? {
        let entry = entry.map_err(io)?;
        if !entry.file_type().map_err(io)?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !gates.contains(&name) {
            return Err(Error::Parse(format!(
                "{GATES_DIR}/{name} is not a gate of {M0_DESIGN} §4"
            )));
        }
        let record = read_record(root, &name)?;
        records.insert(name, record);
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree::test_support::{TempDir, git};
    use std::path::PathBuf;

    const M0: &str = "# M0\n\n## 4. Milestones and gates — the only table\n\nEach gate is ...\n\n\
        | Gate | Depends on |\n|---|---|\n| **M0-Q / G-QUAL** | frozen design |\n\
        | **M4 / G-OBS** | M0-Q |\n| **G-FORMAL** | alongside M0 |\n\n## 5. DEFERRED\n\n\
        | Other | table |\n|---|---|\n| **G-NOT** | x |\n";

    /// A committed repository with every binding source, the design and no record.
    fn repo() -> TempDir {
        let t = TempDir::new();
        let files: [(&str, &str); 9] = [
            (M0_DESIGN, M0),
            ("deploy/kind/cluster.yaml", "kind: Cluster\n"),
            ("crates/a/src/lib.rs", "pub fn a() {}\n"),
            ("Cargo.toml", "[workspace]\n"),
            ("Cargo.lock", "version = 4\n"),
            ("rust-toolchain.toml", "[toolchain]\n"),
            ("CHARTER.md", "# Charter\n"),
            ("deny.toml", "[bans]\n"),
            (PROFILE, include_str!("../../../../profiles/m0.toml")),
        ];
        for (path, text) in files {
            write(&t.0, path, text.as_bytes());
        }
        git(&t.0, &["init", "-q", "-b", "main"]);
        git(&t.0, &["add", "-A"]);
        git(&t.0, &["commit", "-q", "-m", "init"]);
        t
    }

    fn write(root: &Path, path: &str, bytes: &[u8]) {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    fn manifest(state: &str, d: &Digests, artifacts: &[(&str, Digest)]) -> String {
        let list: Vec<String> = artifacts
            .iter()
            .map(|(p, a)| format!(r#"{{"path": "{p}", "digest": "{a}"}}"#))
            .collect();
        format!(
            r#"{{"version": 1, "gate": "G-QUAL", "state": "{state}", "digests": {{"installation": "{}", "software": "{}", "policy": "{}", "profile": "{}"}}, "artifacts": [{}]}}"#,
            d.installation,
            d.software,
            d.policy,
            d.profile,
            list.join(", ")
        )
    }

    /// Writes and commits a `PASSED` G-QUAL record bound to the current digests, with one artifact.
    fn commit_passed_record(root: &Path) {
        let evidence = b"sim: 0 violations\n";
        write(root, "docs/gates/G-QUAL/evidence.txt", evidence);
        let current = current_digests(root).unwrap();
        let text = manifest("PASSED", &current, &[("evidence.txt", sha256(evidence))]);
        write(root, "docs/gates/G-QUAL/record.json", text.as_bytes());
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "record"]);
    }

    /// Every file under `root` outside `.git`, with its bytes.
    fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for e in std::fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                if p.file_name().is_some_and(|n| n == ".git") {
                    continue;
                }
                if p.is_dir() {
                    walk(&p, out);
                } else {
                    out.insert(p.clone(), std::fs::read(&p).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(root, &mut out);
        out
    }

    fn states(reports: &[Report]) -> Vec<(&str, GateState)> {
        reports.iter().map(|r| (r.gate.as_str(), r.state)).collect()
    }

    #[test]
    fn gate_ids_reads_the_section_4_table_only() {
        assert_eq!(gate_ids(M0).unwrap(), ["G-QUAL", "G-OBS", "G-FORMAL"]);
        assert!(gate_ids("# M0\n\n## 5. DEFERRED\n").is_err());
    }

    #[test]
    fn gate_ids_of_the_design_lists_every_gate_of_the_table() {
        let m0 = include_str!("../../../../docs/design/AUTOBOT-M0-AND-GATES.md");
        let ids = gate_ids(m0).unwrap();
        assert_eq!(ids.len(), 11, "{ids:?}");
        assert_eq!(ids.first().map(String::as_str), Some("G-QUAL"));
        assert_eq!(ids.last().map(String::as_str), Some("G-PROD"));
    }

    #[test]
    fn a_passed_record_whose_profile_or_installation_digest_differs_is_invalidated() {
        let t = repo();
        let current = current_digests(&t.0).unwrap();
        let record = |d: &Digests, state| {
            Record::parse(&manifest(state, d, &[("e", current.policy)])).unwrap()
        };
        assert_eq!(
            reported_state(&record(&current, "PASSED"), &current),
            (GateState::Passed, vec![])
        );

        let mut other_profile = current;
        other_profile.profile = sha256(b"another profile");
        assert_eq!(
            reported_state(&record(&other_profile, "PASSED"), &current),
            (GateState::Invalidated, vec![Binding::Profile])
        );
        let mut other_installation = current;
        other_installation.installation = sha256(b"another installation");
        assert_eq!(
            reported_state(&record(&other_installation, "PASSED"), &current),
            (GateState::Invalidated, vec![Binding::Installation])
        );
        assert_eq!(
            reported_state(&record(&other_installation, "FAILED"), &current),
            (GateState::Failed, vec![])
        );
    }

    #[test]
    fn status_reports_not_run_then_passed_then_invalidated_as_the_repository_changes() {
        let t = repo();
        let (_, reports) = status(&t.0).unwrap();
        let not_run = [
            ("G-QUAL", GateState::NotRun),
            ("G-OBS", GateState::NotRun),
            ("G-FORMAL", GateState::NotRun),
        ];
        assert_eq!(states(&reports), not_run);

        commit_passed_record(&t.0);
        let (_, reports) = status(&t.0).unwrap();
        assert_eq!(reports[0].state, GateState::Passed);

        let profile = std::fs::read_to_string(t.0.join(PROFILE)).unwrap();
        let changed = profile.replacen("window_days = 30", "window_days = 31", 1);
        assert_ne!(profile, changed);
        write(&t.0, PROFILE, changed.as_bytes());
        let (_, reports) = status(&t.0).unwrap();
        assert_eq!(reports[0].state, GateState::Invalidated);
        assert_eq!(reports[0].recorded, Some(GateState::Passed));
        assert_eq!(reports[0].differing, [Binding::Profile]);

        write(
            &t.0,
            "deploy/kind/cluster.yaml",
            b"kind: Cluster\nnodes: 2\n",
        );
        let (_, reports) = status(&t.0).unwrap();
        assert_eq!(
            reports[0].differing,
            [Binding::Installation, Binding::Profile]
        );
        assert_eq!(&states(&reports)[1..], &not_run[1..]);
    }

    #[test]
    fn status_changes_no_file() {
        let t = repo();
        commit_passed_record(&t.0);
        write(&t.0, "CHARTER.md", b"# Charter, edited\n");
        let before = snapshot(&t.0);
        let git_before = git(&t.0, &["status", "--porcelain"]);
        let (_, reports) = status(&t.0).unwrap();
        assert_eq!(reports[0].differing, [Binding::Policy]);
        assert_eq!(snapshot(&t.0), before);
        assert_eq!(git(&t.0, &["status", "--porcelain"]), git_before);
    }

    #[test]
    fn status_refuses_a_directory_that_is_no_gate_or_a_record_of_another_gate() {
        let t = repo();
        write(&t.0, "docs/gates/README.md", b"# Gate records\n");
        std::fs::create_dir_all(t.0.join("docs/gates/G-NOT")).unwrap();
        let err = status(&t.0).unwrap_err();
        assert!(err.to_string().contains("G-NOT is not a gate"), "{err}");

        std::fs::remove_dir(t.0.join("docs/gates/G-NOT")).unwrap();
        let current = current_digests(&t.0).unwrap();
        write(
            &t.0,
            "docs/gates/G-OBS/record.json",
            manifest("RUNNING", &current, &[]).as_bytes(),
        );
        let err = status(&t.0).unwrap_err();
        assert!(err.to_string().contains("names gate `G-QUAL`"), "{err}");
    }

    #[test]
    fn files_digest_follows_tracked_content_and_names_only() {
        let t = repo();
        let d = || files_digest(&t.0, &["crates"]).unwrap();
        let first = d();
        write(&t.0, "crates/untracked.rs", b"x");
        assert_eq!(d(), first);
        write(&t.0, "crates/a/src/lib.rs", b"pub fn b() {}\n");
        let edited = d();
        assert_ne!(edited, first);
        git(&t.0, &["mv", "crates/a/src/lib.rs", "crates/a/src/main.rs"]);
        assert_ne!(d(), edited);
        assert!(files_digest(&t.0, &["missing"]).is_err());
    }

    /// A key pair under `dir` and the allowed-signers line naming the test identity.
    fn ssh_key(dir: &Path, name: &str) -> (PathBuf, String) {
        let key = dir.join(name);
        let out = std::process::Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-C", name, "-f"])
            .arg(&key)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let public = std::fs::read_to_string(key.with_extension("pub")).unwrap();
        (key, format!("test@example.invalid {}", public.trim()))
    }

    /// Trusts `signers` in the repository at `root` and signs tag `name` on HEAD with `key`.
    fn signed_tag_with(root: &Path, keys: &Path, key: &Path, signers: &str, name: &str) {
        let allowed = keys.join("allowed_signers");
        std::fs::write(&allowed, format!("{signers}\n")).unwrap();
        git(
            root,
            &[
                "config",
                "gpg.ssh.allowedSignersFile",
                allowed.to_str().unwrap(),
            ],
        );
        let signing = format!("user.signingkey={}", key.display());
        git(
            root,
            &[
                "-c",
                "gpg.format=ssh",
                "-c",
                &signing,
                "tag",
                "-s",
                name,
                "-m",
                "gate record",
            ],
        );
    }

    #[test]
    fn verify_accepts_a_record_whose_commit_has_a_verifying_ssh_signed_tag() {
        let t = repo();
        let keys = TempDir::new();
        commit_passed_record(&t.0);
        let (key, signer) = ssh_key(&keys.0, "owner");
        signed_tag_with(&t.0, &keys.0, &key, &signer, "gate/G-QUAL/1");
        let v = verify(&t.0, "G-QUAL").unwrap();
        assert_eq!(v.problems, Vec::<String>::new());
        assert_eq!(v.tag.as_deref(), Some("gate/G-QUAL/1"));
        assert!(v.passed());
    }

    #[test]
    fn verify_fails_a_record_without_a_verifying_tag() {
        let t = repo();
        commit_passed_record(&t.0);
        let v = verify(&t.0, "G-QUAL").unwrap();
        assert!(!v.passed());
        assert!(
            v.problems[0].contains("no tag gate/G-QUAL/* points at"),
            "{v:?}"
        );

        git(&t.0, &["tag", "gate/G-QUAL/light"]);
        git(
            &t.0,
            &[
                "-c",
                "tag.gpgsign=false",
                "tag",
                "-a",
                "gate/G-QUAL/plain",
                "-m",
                "unsigned",
            ],
        );
        let v = verify(&t.0, "G-QUAL").unwrap();
        assert!(!v.passed());
        assert!(
            v.problems
                .iter()
                .any(|p| p.contains("light is not an annotated tag")),
            "{v:?}"
        );
        assert!(
            v.problems
                .iter()
                .any(|p| p.contains("plain has no SSH signature")),
            "{v:?}"
        );

        let keys = TempDir::new();
        let (key, _) = ssh_key(&keys.0, "stranger");
        let (_, owner) = ssh_key(&keys.0, "owner");
        signed_tag_with(&t.0, &keys.0, &key, &owner, "gate/G-QUAL/stranger");
        let v = verify(&t.0, "G-QUAL").unwrap();
        assert!(!v.passed());
        assert!(
            v.problems
                .iter()
                .any(|p| p.contains("stranger does not verify")),
            "{v:?}"
        );
    }

    #[test]
    fn verify_fails_when_the_record_changed_after_its_signed_tag() {
        let t = repo();
        let keys = TempDir::new();
        commit_passed_record(&t.0);
        let (key, signer) = ssh_key(&keys.0, "owner");
        signed_tag_with(&t.0, &keys.0, &key, &signer, "gate/G-QUAL/1");
        write(&t.0, "docs/gates/G-QUAL/extra.txt", b"late\n");
        let v = verify(&t.0, "G-QUAL").unwrap();
        assert!(v.problems[0].contains("uncommitted changes"), "{v:?}");
        git(&t.0, &["add", "-A"]);
        git(&t.0, &["commit", "-q", "-m", "late"]);
        let v = verify(&t.0, "G-QUAL").unwrap();
        assert!(!v.passed());
        assert!(
            v.problems[0].contains("no tag gate/G-QUAL/* points at"),
            "{v:?}"
        );
    }

    #[test]
    fn verify_fails_a_missing_or_altered_artifact() {
        let t = repo();
        let keys = TempDir::new();
        commit_passed_record(&t.0);
        std::fs::write(
            t.0.join("docs/gates/G-QUAL/evidence.txt"),
            b"sim: 1 violation\n",
        )
        .unwrap();
        git(&t.0, &["commit", "-q", "-am", "alter evidence"]);
        let (key, signer) = ssh_key(&keys.0, "owner");
        signed_tag_with(&t.0, &keys.0, &key, &signer, "gate/G-QUAL/1");
        let v = verify(&t.0, "G-QUAL").unwrap();
        assert_eq!(v.tag.as_deref(), Some("gate/G-QUAL/1"));
        assert!(!v.passed());
        assert!(
            v.problems[0].contains("artifact `evidence.txt` has digest"),
            "{v:?}"
        );

        git(&t.0, &["rm", "-q", "docs/gates/G-QUAL/evidence.txt"]);
        git(&t.0, &["commit", "-q", "-m", "drop evidence"]);
        let v = verify(&t.0, "G-QUAL").unwrap();
        assert!(
            v.problems[0].starts_with("artifact `evidence.txt`: "),
            "{v:?}"
        );
    }

    #[test]
    fn verify_refuses_a_gate_outside_the_table() {
        let t = repo();
        let err = verify(&t.0, "../G-QUAL").unwrap_err();
        assert!(err.to_string().contains("is not a gate"), "{err}");
    }
}
