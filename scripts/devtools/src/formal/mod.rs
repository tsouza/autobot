//! The formal toolchain behind the `just formal-*` recipes: Quint typechecking and simulation,
//! and Apalache symbolic bounded checking through `quint verify`.
//!
//! **Files.** Every `.qnt` file under `formal/`, outside directories named `target`, is
//! typechecked. A *check target* is either an
//! instance `formal/instances/<name>.qnt` or a top-level module `formal/<name>.qnt` that
//! declares `action init` and no `const`; parameterised modules are checked through their
//! instances. A target is named by its file stem, and its main module is the module of that
//! name, as Quint assumes by default.
//!
//! **Invariants.** The invariants of a target are the `F<n>_<Name>` invariants (as
//! [`trace::model_invariants`] reads them) and the `Inv_<Name>` declarations (invariants that
//! own no F-n, such as the smoke module's) of the target file and of every file it imports
//! `from "<path>"`, transitively.
//!
//! **Bounds.** Simulation runs `quint run` with the Rust evaluator on one thread from the fixed
//! seed [`SEED`], [`SIM_SAMPLES`] traces of at most [`SIM_STEPS`] steps. Verification runs
//! `quint verify` with Apalache to depth [`VERIFY_STEPS`], talking to an Apalache server Quint
//! starts on [`APALACHE_ENDPOINT`], away from the default port, so a server started by hand is
//! never used. `trace` writes one ITF trace per target, from the same seed, to
//! `target/formal/traces/<target>.itf.json`.
//!
//! **CI.** `ci` is the required `formal` job: it passes at once when the change touches none of
//! [`REQUIRED_PATHS`], and otherwise typechecks and simulates. `ci-verify` is the `formal-verify`
//! job: it verifies every target when the change touches [`VERIFY_PATHS`] and passes at once
//! otherwise. The change is read from the event payload at `GITHUB_EVENT_PATH`: a pull request
//! compares its head with the merge base of its base and head, a push compares `before` with
//! `after`; any other event (the nightly schedule), a push without a previous commit, or no
//! payload at all checks everything. Both jobs write the digest manifest `target/formal/digests.txt`: the
//! pinned tools, the `java` version and the SHA-256 of every model file.

pub mod tools;

use crate::design::{self, is_word_char, trace};
use crate::process::Cmd;
use crate::{Error, Result, git};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use tools::Tool;

/// The directory of the Quint model, relative to the repository root.
pub const MODEL_DIR: &str = trace::MODEL_DIR;
/// The directory of the instances, relative to [`MODEL_DIR`].
pub const INSTANCES_DIR: &str = "instances";
/// The working directory of the toolchain, relative to the repository root: tools, run
/// directory, traces and the digest manifest.
pub const WORK_DIR: &str = "target/formal";
/// The prefix of an invariant that owns no F-n.
pub const INVARIANT_PREFIX: &str = "Inv_";
/// The simulation seed.
pub const SEED: &str = "0x2a";
/// The step bound of a simulated trace.
pub const SIM_STEPS: u32 = 20;
/// The number of simulated traces per target.
pub const SIM_SAMPLES: u32 = 1000;
/// The depth Apalache checks to.
pub const VERIFY_STEPS: u32 = 10;
/// The endpoint of the Apalache server Quint starts for verification.
pub const APALACHE_ENDPOINT: &str = "localhost:18822";
/// Paths whose change makes the `formal` job typecheck and simulate.
pub const REQUIRED_PATHS: [&str; 2] = ["formal/", "scripts/devtools/src/formal/"];
/// Paths whose change makes the `formal-verify` job run Apalache.
pub const VERIFY_PATHS: [&str; 1] = ["formal/"];

/// A check target: an instance, or a top-level module that needs no instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The file stem, which is also the main module's name.
    pub name: String,
    /// The file, relative to the repository root.
    pub path: String,
    /// The invariants checked on it, sorted.
    pub invariants: Vec<String>,
}

/// Every `.qnt` file under [`MODEL_DIR`] in the repository `root`, recursively and skipping
/// directories named `target`, relative to `root` and sorted; none without [`MODEL_DIR`].
///
/// # Errors
/// Fails if a directory cannot be listed.
pub fn qnt_files(root: &Path) -> Result<Vec<String>> {
    let dir = root.join(MODEL_DIR);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    Ok(design::walk(root, &dir)?
        .into_iter()
        .filter(|(_, path)| path.extension().is_some_and(|e| e == "qnt"))
        .map(|(rel, _)| rel)
        .collect())
}

/// The invariants declared in the Quint text `module`: its `F<n>_<Name>` invariants and its
/// `val`, `def` or `temporal` declarations named `Inv_<Name>`, in order.
#[must_use]
pub fn declared_invariants(module: &str) -> Vec<String> {
    let mut out: Vec<(usize, String)> = trace::model_invariants(module)
        .into_iter()
        .map(|(_, name, line)| (line, name))
        .collect();
    for (name, line) in trace::declarations(module) {
        if name.len() > INVARIANT_PREFIX.len() && name.starts_with(INVARIANT_PREFIX) {
            out.push((line, name.to_owned()));
        }
    }
    out.sort();
    out.into_iter().map(|(_, name)| name).collect()
}

/// The paths the Quint text `module` imports or exports `from "<path>"`, in order.
#[must_use]
pub fn imports(module: &str) -> Vec<String> {
    module
        .lines()
        .map(str::trim_start)
        .filter(|l| l.starts_with("import ") || l.starts_with("export "))
        .filter_map(|l| {
            let rest = &l[l.find(" from \"")? + " from \"".len()..];
            Some(rest[..rest.find('"')?].to_owned())
        })
        .collect()
}

/// Whether the Quint text `module` can be checked without an instance: it declares
/// `action init` and no `const`.
#[must_use]
pub fn is_standalone(module: &str) -> bool {
    let mut init = false;
    for line in module.lines().map(str::trim_start) {
        if line.starts_with("const ") {
            return false;
        }
        if let Some(rest) = line.strip_prefix("action init") {
            init |= rest.starts_with(|c: char| !is_word_char(c));
        }
    }
    init
}

/// The invariants of the file `path` (relative to `root`) and of every file it imports,
/// transitively, sorted and without duplicates.
///
/// # Errors
/// Fails if the file or an imported file cannot be read.
pub fn invariants_of(root: &Path, path: &str) -> Result<Vec<String>> {
    let mut seen = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut pending = vec![root.join(path)];
    while let Some(file) = pending.pop() {
        if !seen.insert(file.clone()) {
            continue;
        }
        let text =
            std::fs::read_to_string(&file).map_err(|e| tools::io_error("reading", &file, &e))?;
        names.extend(declared_invariants(&text));
        let dir = file.parent().unwrap_or(root);
        for import in imports(&text) {
            pending.push(normalize(&dir.join(format!("{import}.qnt"))));
        }
    }
    Ok(names.into_iter().collect())
}

/// Every check target in the repository `root`: the top-level standalone modules, then the
/// instances, each sorted by path.
///
/// # Errors
/// Fails if the model directory cannot be listed or a file cannot be read.
pub fn targets(root: &Path) -> Result<Vec<Target>> {
    let instances = format!("{MODEL_DIR}/{INSTANCES_DIR}/");
    let mut out = Vec::new();
    for path in qnt_files(root)? {
        let top_level = path
            .strip_prefix(MODEL_DIR)
            .and_then(|p| p.strip_prefix('/'))
            .is_some_and(|file| !file.contains('/'));
        let is_target = if let Some(file) = path.strip_prefix(&instances) {
            !file.contains('/')
        } else if !top_level {
            false
        } else {
            let file = root.join(&path);
            is_standalone(
                &std::fs::read_to_string(&file)
                    .map_err(|e| tools::io_error("reading", &file, &e))?,
            )
        };
        if is_target {
            let name = Path::new(&path)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            out.push(Target {
                name,
                invariants: invariants_of(root, &path)?,
                path,
            });
        }
    }
    out.sort_by_key(|t| (t.path.starts_with(&instances), t.path.clone()));
    Ok(out)
}

/// The paths changed by the event in the payload `event`, or `None` when everything is to be
/// checked (see the module documentation).
///
/// # Errors
/// Fails if the payload is not JSON, a pull request payload lacks its SHAs, or git fails.
pub fn changed(root: &Path, event: &str) -> Result<Option<Vec<String>>> {
    let value: serde_json::Value =
        serde_json::from_str(event).map_err(|e| Error::Parse(format!("event payload: {e}")))?;
    let pr = &value["pull_request"];
    let (base, head) = if pr.is_null() {
        match (value["before"].as_str(), value["after"].as_str()) {
            (Some(before), Some(after)) if before.bytes().any(|b| b != b'0') => {
                (before.to_owned(), after.to_owned())
            }
            _ => return Ok(None),
        }
    } else {
        let sha = |side: &str| {
            pr[side]["sha"].as_str().map(str::to_owned).ok_or_else(|| {
                Error::Parse(format!("event payload without pull_request.{side}.sha"))
            })
        };
        (sha("base")?, sha("head")?)
    };
    git::changed_paths(root, &base, &head).map(Some)
}

/// Whether any of `paths` lies under one of the directory `prefixes` (each ending in `/`).
#[must_use]
pub fn touches(paths: &[String], prefixes: &[&str]) -> bool {
    paths
        .iter()
        .any(|p| prefixes.iter().any(|prefix| p.starts_with(prefix)))
}

/// The digest manifest: one line per pinned tool, the `java` version, and the SHA-256 of every
/// model file under the repository `root`.
///
/// # Errors
/// Fails if the model directory cannot be listed or a digest cannot be computed.
pub fn manifest(root: &Path) -> Result<String> {
    let mut out: Vec<String> = tools::ALL.iter().map(Tool::manifest_line).collect();
    let java = Cmd::new("java")
        .args(["--version"])
        .output()
        .ok()
        .and_then(|v| v.lines().next().map(str::to_owned))
        .unwrap_or_else(|| "not found".to_owned());
    out.push(format!("java {java}"));
    for path in qnt_files(root)? {
        let digest = tools::sha256(&root.join(&path))?;
        out.push(format!("model sha256:{digest} {path}"));
    }
    Ok(out.join("\n") + "\n")
}

/// The toolchain of one repository checkout.
struct Toolchain {
    /// The repository root, absolute.
    root: PathBuf,
    /// [`WORK_DIR`] under `root`.
    work: PathBuf,
}

impl Toolchain {
    fn new(root: &Path) -> Result<Self> {
        let root = root
            .canonicalize()
            .map_err(|e| tools::io_error("resolving", root, &e))?;
        let work = root.join(WORK_DIR);
        Ok(Self { root, work })
    }

    /// Installs `tools`, refusing a platform the pinned assets are not built for.
    fn install(&self, tools: &[Tool]) -> Result<()> {
        if (std::env::consts::OS, std::env::consts::ARCH) != ("linux", "x86_64") {
            return Err(Error::Parse(format!(
                "the formal tools are pinned for linux x86_64, not {} {}",
                std::env::consts::OS,
                std::env::consts::ARCH
            )));
        }
        for tool in tools {
            tool.ensure(&self.work)?;
        }
        Ok(())
    }

    /// `quint <args>` with `QUINT_HOME` set, run from the run directory, which receives the
    /// files Apalache writes to its working directory.
    fn quint(&self, args: &[String]) -> Result<Cmd> {
        let run = self.work.join("run");
        tools::create_dir(&run)?;
        let home = self.work.join(tools::QUINT_HOME);
        Ok(Cmd::new("env")
            .args([
                format!("QUINT_HOME={}", home.display()),
                tools::QUINT.install_path(&self.work).display().to_string(),
            ])
            .args(args.iter().cloned())
            .current_dir(run))
    }

    fn abs(&self, path: &str) -> String {
        self.root.join(path).display().to_string()
    }

    fn write_manifest(&self) -> Result<()> {
        let path = self.work.join("digests.txt");
        tools::create_dir(&self.work)?;
        std::fs::write(&path, manifest(&self.root)?)
            .map_err(|e| tools::io_error("writing", &path, &e))?;
        println!("formal: digests written to {WORK_DIR}/digests.txt");
        Ok(())
    }

    /// Runs `cmd` for every item, reporting each failure; `true` when none failed.
    fn each<T>(&self, items: &[T], what: &str, cmd: impl Fn(&T) -> Result<Cmd>) -> bool {
        let mut failed = 0;
        for item in items {
            if let Err(e) = cmd(item).and_then(|c| c.run()) {
                eprintln!("formal {what}: {e}");
                failed += 1;
            }
        }
        println!("formal {what}: {} checked, {failed} failed", items.len());
        failed == 0
    }

    fn typecheck(&self) -> Result<bool> {
        self.install(&[tools::QUINT])?;
        let files = qnt_files(&self.root)?;
        Ok(self.each(&files, "typecheck", |f| {
            self.quint(&["typecheck".to_owned(), self.abs(f)])
        }))
    }

    fn simulate(&self) -> Result<bool> {
        self.install(&[tools::QUINT, tools::QUINT_EVALUATOR])?;
        let targets = targets(&self.root)?;
        Ok(self.each(&targets, "sim", |t| self.quint(&self.run_args(t))))
    }

    fn trace(&self) -> Result<bool> {
        self.install(&[tools::QUINT, tools::QUINT_EVALUATOR])?;
        let dir = self.work.join("traces");
        tools::create_dir(&dir)?;
        let targets = targets(&self.root)?;
        Ok(self.each(&targets, "trace", |t| {
            let mut args = self.run_args(t);
            let out = dir.join(format!("{}.itf.json", t.name));
            args.extend(["--out-itf".to_owned(), out.display().to_string()]);
            self.quint(&args)
        }))
    }

    fn run_args(&self, t: &Target) -> Vec<String> {
        let mut args: Vec<String> = ["run", &self.abs(&t.path), "--backend", "rust"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        args.extend([
            "--n-threads".to_owned(),
            "1".to_owned(),
            "--seed".to_owned(),
            SEED.to_owned(),
            "--max-steps".to_owned(),
            SIM_STEPS.to_string(),
            "--max-samples".to_owned(),
            SIM_SAMPLES.to_string(),
        ]);
        push_invariants(&mut args, t);
        args
    }

    fn verify(&self, targets: &[Target]) -> Result<bool> {
        self.install(&[tools::QUINT, tools::APALACHE])?;
        Ok(self.each(targets, "verify", |t| {
            if t.invariants.is_empty() {
                return Err(Error::Parse(format!(
                    "{} declares no invariant to verify",
                    t.path
                )));
            }
            let mut args: Vec<String> = vec!["verify".to_owned(), self.abs(&t.path)];
            args.extend([
                "--max-steps".to_owned(),
                VERIFY_STEPS.to_string(),
                "--apalache-version".to_owned(),
                tools::APALACHE.version.to_owned(),
                "--server-endpoint".to_owned(),
                APALACHE_ENDPOINT.to_owned(),
            ]);
            push_invariants(&mut args, t);
            self.quint(&args)
        }))
    }
}

/// Appends `--invariants <names…>` for a target with invariants; it goes last, because the
/// option takes every following argument.
fn push_invariants(args: &mut Vec<String>, t: &Target) {
    if !t.invariants.is_empty() {
        args.push("--invariants".to_owned());
        args.extend(t.invariants.iter().cloned());
    }
}

/// What `run` was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Setup,
    Typecheck,
    Sim,
    Verify(Option<String>),
    Trace,
    Ci,
    CiVerify,
}

const USAGE: &str = "usage: formal setup | typecheck | sim | verify <target> | verify --all | trace | ci | ci-verify";

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Mode> {
    let args: Vec<String> = args.into_iter().collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    Ok(match args.as_slice() {
        ["setup"] => Mode::Setup,
        ["typecheck"] => Mode::Typecheck,
        ["sim"] => Mode::Sim,
        ["verify", "--all"] => Mode::Verify(None),
        ["verify", name] if !name.starts_with('-') => Mode::Verify(Some((*name).to_owned())),
        ["trace"] => Mode::Trace,
        ["ci"] => Mode::Ci,
        ["ci-verify"] => Mode::CiVerify,
        _ => return Err(Error::Parse(USAGE.to_owned())),
    })
}

/// The change of the current GitHub event, or `None` (check everything) without a payload.
fn event_change(root: &Path) -> Result<Option<Vec<String>>> {
    let Some(path) = std::env::var_os("GITHUB_EVENT_PATH") else {
        return Ok(None);
    };
    let event = std::fs::read_to_string(&path)
        .map_err(|e| tools::io_error("reading", Path::new(&path), &e))?;
    changed(root, &event)
}

fn exit(ok: bool) -> ExitCode {
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Runs the toolchain in the repository at the current directory, as the `just formal-*`
/// recipes do. The single argument selects the command: `setup` installs every pinned tool and
/// writes the digest manifest; `typecheck`, `sim` and `trace` act on the model; `verify
/// <target>` or `verify --all` runs Apalache; `ci` and `ci-verify` are the CI jobs.
///
/// # Errors
/// Fails on unknown arguments, an unknown target, a tool that cannot be installed or verified,
/// an unreadable model or event payload, or a git failure. A failing check is reported and
/// returns a failure exit code.
pub fn run(args: impl IntoIterator<Item = String>) -> Result<ExitCode> {
    let tc = Toolchain::new(Path::new("."))?;
    let ok = match parse_args(args)? {
        Mode::Setup => {
            tc.install(&tools::ALL)?;
            tc.write_manifest()?;
            true
        }
        Mode::Typecheck => tc.typecheck()?,
        Mode::Sim => tc.simulate()?,
        Mode::Trace => tc.trace()?,
        Mode::Verify(name) => {
            let all = targets(&tc.root)?;
            let chosen: Vec<Target> = match name {
                None => all,
                Some(name) => {
                    let t = all.into_iter().find(|t| t.name == name).ok_or_else(|| {
                        Error::Parse(format!("no check target `{name}` under {MODEL_DIR}/"))
                    })?;
                    vec![t]
                }
            };
            tc.verify(&chosen)?
        }
        Mode::Ci => match event_change(&tc.root)? {
            Some(paths) if !touches(&paths, &REQUIRED_PATHS) => {
                println!("formal: nothing under {REQUIRED_PATHS:?} changed; nothing to check");
                true
            }
            _ => {
                tc.install(&[tools::QUINT, tools::QUINT_EVALUATOR])?;
                tc.write_manifest()?;
                let typed = tc.typecheck()?;
                typed && tc.simulate()?
            }
        },
        Mode::CiVerify => match event_change(&tc.root)? {
            Some(paths) if !touches(&paths, &VERIFY_PATHS) => {
                println!("formal-verify: nothing under {VERIFY_PATHS:?} changed; nothing to check");
                true
            }
            _ => {
                tc.install(&[tools::QUINT, tools::APALACHE])?;
                tc.write_manifest()?;
                tc.verify(&targets(&tc.root)?)?
            }
        },
    };
    Ok(exit(ok))
}

/// `path` with `.` and `..` components resolved lexically.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree::test_support::{TempDir, commit, git};

    const SMOKE: &str = include_str!("../../../../formal/smoke.qnt");

    fn write(root: &Path, path: &str, text: &str) {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }

    #[test]
    fn invariants_are_f_n_and_inv_declarations() {
        let module = "module m {\n  val Inv_Typed = true\n  pure def F3_Barrier = true\n  \
                      val Invariant = true\n  val Inv_ = true\n  temporal Inv_Live = true\n  \
                      val F1_Idempotency = true\n  val helper = Inv_Typed\n}\n";
        assert_eq!(
            declared_invariants(module),
            ["Inv_Typed", "F3_Barrier", "Inv_Live", "F1_Idempotency"]
        );
    }

    #[test]
    fn imports_and_standalone_modules_are_recognised() {
        let instance = "module commit_mc {\n  import commit(N = 2).* from \"../commit\"\n  \
                        export types.* from \"../types\"\n  import basics.*\n}\n";
        assert_eq!(imports(instance), ["../commit", "../types"]);
        assert!(!is_standalone(instance));
        assert!(is_standalone(SMOKE));
        assert!(!is_standalone(
            "module p {\n  const N: int\n  action init = true\n}\n"
        ));
        assert!(!is_standalone("module p {\n  action initAll = true\n}\n"));
    }

    #[test]
    fn targets_follow_instances_and_imports() {
        let tmp = TempDir::new();
        let root = &tmp.0;
        write(root, "formal/smoke.qnt", SMOKE);
        write(
            root,
            "formal/commit.qnt",
            "// owns: F-1\nmodule commit {\n  import types.* from \"./types\"\n  const N: int\n  \
             action init = true\n  val F1_Idempotency = true\n}\n",
        );
        write(
            root,
            "formal/types.qnt",
            "module types {\n  val Inv_Typed = true\n}\n",
        );
        write(
            root,
            "formal/instances/commit_mc.qnt",
            "module commit_mc {\n  import commit(N = 2).* from \"../commit\"\n}\n",
        );
        write(
            root,
            "formal/variants/commit.qnt",
            "module commit {\n  action init = true\n}\n",
        );
        write(root, "formal/target/built.qnt", "module built {}\n");
        write(root, "formal/notes.md", "not a model\n");
        assert_eq!(
            qnt_files(root).unwrap(),
            [
                "formal/commit.qnt",
                "formal/instances/commit_mc.qnt",
                "formal/smoke.qnt",
                "formal/types.qnt",
                "formal/variants/commit.qnt",
            ]
        );
        let targets = targets(root).unwrap();
        let summary: Vec<(&str, &str, Vec<&str>)> = targets
            .iter()
            .map(|t| {
                (
                    t.name.as_str(),
                    t.path.as_str(),
                    t.invariants.iter().map(String::as_str).collect(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("smoke", "formal/smoke.qnt", vec!["Inv_Bounded"]),
                (
                    "commit_mc",
                    "formal/instances/commit_mc.qnt",
                    vec!["F1_Idempotency", "Inv_Typed"]
                ),
            ]
        );
    }

    #[test]
    fn a_repository_without_a_model_has_no_files() {
        let tmp = TempDir::new();
        assert_eq!(qnt_files(&tmp.0).unwrap(), Vec::<String>::new());
        assert_eq!(targets(&tmp.0).unwrap(), []);
    }

    #[test]
    fn manifest_failures_name_the_failed_operation() {
        let tmp = TempDir::new();
        write(&tmp.0, "formal/smoke.qnt", SMOKE);
        let tc = Toolchain::new(&tmp.0).unwrap();
        write(&tmp.0, "target/formal", "not a directory");
        let err = tc.write_manifest().unwrap_err().to_string();
        assert!(err.contains(": creating "), "{err}");
        std::fs::remove_file(&tc.work).unwrap();
        std::fs::create_dir_all(tc.work.join("digests.txt")).unwrap();
        let err = tc.write_manifest().unwrap_err().to_string();
        assert!(err.contains(": writing "), "{err}");
    }

    #[test]
    fn the_event_selects_the_change() {
        let tmp = TempDir::new();
        let repo = &tmp.0;
        git(repo, &["init", "-q", "-b", "main"]);
        commit(repo, "README", "a\n", "base");
        let base = git(repo, &["rev-parse", "HEAD"]);
        std::fs::create_dir_all(repo.join("formal")).unwrap();
        commit(repo, "formal/smoke.qnt", SMOKE, "model");
        let head = git(repo, &["rev-parse", "HEAD"]);
        let pr = format!(
            r#"{{"pull_request":{{"base":{{"sha":"{base}"}},"head":{{"sha":"{head}"}}}}}}"#
        );
        assert_eq!(
            changed(repo, &pr).unwrap(),
            Some(vec!["formal/smoke.qnt".to_owned()])
        );
        let push = format!(r#"{{"before":"{base}","after":"{head}"}}"#);
        assert_eq!(
            changed(repo, &push).unwrap(),
            Some(vec!["formal/smoke.qnt".to_owned()])
        );
        let first_push = format!(r#"{{"before":"{}","after":"{head}"}}"#, "0".repeat(40));
        assert_eq!(changed(repo, &first_push).unwrap(), None);
        assert_eq!(changed(repo, r#"{"schedule":"17 3 * * *"}"#).unwrap(), None);
        assert!(changed(repo, r#"{"pull_request":{"base":{}}}"#).is_err());
        assert!(changed(repo, "not json").is_err());
    }

    #[test]
    fn only_listed_directories_count_as_touched() {
        let paths = |p: &[&str]| p.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(touches(
            &paths(&["README.md", "formal/smoke.qnt"]),
            &REQUIRED_PATHS
        ));
        assert!(touches(
            &paths(&["scripts/devtools/src/formal/tools.rs"]),
            &REQUIRED_PATHS
        ));
        assert!(!touches(
            &paths(&["scripts/devtools/src/formal/tools.rs"]),
            &VERIFY_PATHS
        ));
        assert!(!touches(
            &paths(&["formalism.md", "crates/x.rs"]),
            &REQUIRED_PATHS
        ));
        assert!(!touches(&[], &REQUIRED_PATHS));
    }

    #[test]
    fn the_manifest_lists_tools_java_and_models() {
        let tmp = TempDir::new();
        write(&tmp.0, "formal/smoke.qnt", "abc");
        let manifest = manifest(&tmp.0).unwrap();
        let lines: Vec<&str> = manifest.lines().collect();
        assert_eq!(lines.len(), 5, "{manifest}");
        assert_eq!(lines[0], tools::QUINT.manifest_line());
        assert!(lines[3].starts_with("java "), "{manifest}");
        assert_eq!(
            lines[4],
            "model sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad \
             formal/smoke.qnt"
        );
    }

    #[test]
    fn arguments_select_the_mode() {
        let parse = |a: &[&str]| parse_args(a.iter().map(|s| (*s).to_owned()));
        assert_eq!(parse(&["sim"]).unwrap(), Mode::Sim);
        assert_eq!(
            parse(&["verify", "smoke"]).unwrap(),
            Mode::Verify(Some("smoke".to_owned()))
        );
        assert_eq!(parse(&["verify", "--all"]).unwrap(), Mode::Verify(None));
        assert_eq!(parse(&["ci-verify"]).unwrap(), Mode::CiVerify);
        assert!(parse(&[]).is_err());
        assert!(parse(&["verify"]).is_err());
        assert!(parse(&["verify", "--bogus"]).is_err());
        assert!(parse(&["sim", "extra"]).is_err());
    }
}
