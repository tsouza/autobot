//! The crate layering check behind `just layering`.
//!
//! The check reads the dependency declarations of every workspace member through
//! [`crate::cargo::workspace_members`] and fails on any of these:
//!
//! - `autobot-kernel` has a normal dependency on `kube`, `k8s-openapi`, `tokio`, or a workspace
//!   crate other than `autobot-kernel-derive`. Other external crates, such as `serde` and
//!   `schemars` for derives, are allowed.
//! - A dev-dependency on `autobot-devtools` does not set `default-features = false`.
//! - `autobot-api` or `autobot-adapters` has a normal or build dependency on a workspace crate
//!   other than `autobot-kernel`.
//! - `autobot-controllers` depends on `autobot-operator` or `autobot-cli`, with any kind.
//! - `autobot-fakes` or `autobot-testkit` is a normal or build dependency, except the normal
//!   dependency of `autobot-testkit` on `autobot-fakes`, and an optional normal dependency of
//!   `autobot-operator` on `autobot-fakes` that only the non-default feature `m0-fakes`
//!   activates, written as `dep:autobot-fakes` so that no implicit feature exposes it.
//! - A normal or build dependency on `autobot-operator` enables `m0-fakes`, or a feature that
//!   `default` enables, directly or through other features, contains `autobot-operator/m0-fakes`
//!   or `autobot-operator?/m0-fakes`: either would link `autobot-fakes` into a normal build.
//! - The source file of a binary target has more than [`MAX_MAIN_LINES`] lines: a binary is a
//!   thin wrapper over a library.
//!
//! The rules live only here: no per-crate metadata table and no `deny.toml` ban repeats them.
//! Only declared dependencies are checked, which is where a crate chooses its layer; what a
//! permitted external crate itself depends on is outside this check.

use crate::cargo::{Dependency, Kind, Package, workspace_members};
use crate::{Error, Result};
use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;
use std::process::ExitCode;

/// The most lines a binary target's source file may have.
pub const MAX_MAIN_LINES: usize = 40;

/// The kernel crate.
pub const KERNEL: &str = "autobot-kernel";
/// The kernel's derive-macro crate, the one workspace crate the kernel may depend on.
pub const KERNEL_DERIVE: &str = "autobot-kernel-derive";
/// External crates the kernel may not depend on normally.
pub const KERNEL_BANNED: [&str; 3] = ["kube", "k8s-openapi", "tokio"];
/// The development library, used by other crates as a dev-dependency.
pub const DEVTOOLS: &str = "autobot-devtools";
/// Crates whose only workspace dependency may be the kernel.
pub const KERNEL_ONLY: [&str; 2] = ["autobot-api", "autobot-adapters"];
/// The controllers crate.
pub const CONTROLLERS: &str = "autobot-controllers";
/// The binary crates the controllers may not depend on.
pub const CONTROLLERS_BANNED: [&str; 2] = ["autobot-operator", "autobot-cli"];
/// The fakes crate.
pub const FAKES: &str = "autobot-fakes";
/// The test harness crate.
pub const TESTKIT: &str = "autobot-testkit";
/// The operator crate.
pub const OPERATOR: &str = "autobot-operator";
/// The operator feature that may link [`FAKES`].
pub const M0_FAKES: &str = "m0-fakes";

/// One broken rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The crate that breaks it.
    pub krate: String,
    /// What is wrong.
    pub message: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.krate, self.message)
    }
}

/// Checks the workspace containing `dir`, prints every violation or a clean line, and returns
/// the exit code.
///
/// # Errors
/// Fails if `cargo metadata` fails or prints unexpected JSON, or a binary's source file cannot
/// be read.
pub fn run(dir: impl AsRef<Path>) -> Result<ExitCode> {
    let crates = workspace_members(dir)?;
    let violations = check(&crates, line_count)?;
    if violations.is_empty() {
        println!("layering: {} crates follow the rules", crates.len());
        return Ok(ExitCode::SUCCESS);
    }
    for v in &violations {
        println!("{v}");
    }
    println!("layering: {} violation(s)", violations.len());
    Ok(ExitCode::FAILURE)
}

/// The number of lines of the file at `path`.
fn line_count(path: &Path) -> Result<usize> {
    std::fs::read_to_string(path)
        .map(|text| text.lines().count())
        .map_err(|e| Error::Parse(format!("reading {}: {e}", path.display())))
}

/// Every violation of the layering rules by `crates`, the workspace members, in crate order.
/// `lines` returns the line count of a binary's source file.
///
/// # Errors
/// Fails if `lines` fails.
pub fn check(crates: &[Package], lines: impl Fn(&Path) -> Result<usize>) -> Result<Vec<Violation>> {
    let workspace: BTreeSet<&str> = crates.iter().map(|c| c.name.as_str()).collect();
    let mut out = Vec::new();
    for c in crates {
        let mut flag = |message: String| {
            out.push(Violation {
                krate: c.name.clone(),
                message,
            });
        };
        for d in &c.dependencies {
            if let Some(message) = dependency_violation(c, d, &workspace) {
                flag(message);
            }
        }
        let (_, entries) = enabled_by_default(c);
        for entry in entries {
            if enables_operator_m0_fakes(entry) {
                flag(format!(
                    "default features enable `{entry}`, which links `{FAKES}` into a normal build"
                ));
            }
        }
        for b in &c.binaries {
            let n = lines(&b.src_path)?;
            if n > MAX_MAIN_LINES {
                flag(format!(
                    "binary `{}` has {n} lines in {}, more than {MAX_MAIN_LINES}: move the logic into a library",
                    b.name,
                    b.src_path.display()
                ));
            }
        }
    }
    Ok(out)
}

fn dependency_violation(c: &Package, d: &Dependency, workspace: &BTreeSet<&str>) -> Option<String> {
    let dep = d.name.as_str();
    let kind = d.kind.name();
    let in_workspace = workspace.contains(dep);
    if dep == DEVTOOLS && d.kind == Kind::Dev && d.default_features {
        return Some(format!(
            "dev-dependency on `{DEVTOOLS}` must set `default-features = false`"
        ));
    }
    if c.name == KERNEL
        && d.kind == Kind::Normal
        && (KERNEL_BANNED.contains(&dep) || (in_workspace && dep != KERNEL_DERIVE))
    {
        return Some(format!(
            "normal dependency on `{dep}`: the kernel depends on no Kubernetes, async or workspace crate other than `{KERNEL_DERIVE}`"
        ));
    }
    if KERNEL_ONLY.contains(&c.name.as_str())
        && d.kind != Kind::Dev
        && in_workspace
        && dep != KERNEL
    {
        return Some(format!(
            "{kind} dependency on `{dep}`: this crate depends on no workspace crate other than `{KERNEL}`"
        ));
    }
    if c.name == CONTROLLERS && CONTROLLERS_BANNED.contains(&dep) {
        return Some(format!(
            "{kind} dependency on `{dep}`: the controllers never depend on a binary crate"
        ));
    }
    if dep == OPERATOR && d.kind != Kind::Dev && d.features.iter().any(|f| f == M0_FAKES) {
        return Some(format!(
            "{kind} dependency on `{OPERATOR}` enables `{M0_FAKES}`, which links `{FAKES}` into a normal build"
        ));
    }
    if (dep == FAKES || dep == TESTKIT) && d.kind != Kind::Dev && !test_crate_exception(c, d) {
        return Some(format!(
            "{kind} dependency on `{dep}`: test crates are dev-dependencies only"
        ));
    }
    None
}

/// Whether `d`, a normal or build dependency of `c` on a test crate, is one of the two allowed.
fn test_crate_exception(c: &Package, d: &Dependency) -> bool {
    if d.kind != Kind::Normal || d.name != FAKES {
        return false;
    }
    if c.name == TESTKIT {
        return true;
    }
    if c.name != OPERATOR || !d.optional {
        return false;
    }
    let activators: BTreeSet<&str> = c
        .features
        .iter()
        .filter(|(_, enables)| enables.iter().any(|e| activates(e, &d.name)))
        .map(|(feature, _)| feature.as_str())
        .collect();
    activators == BTreeSet::from([M0_FAKES]) && !enabled_by_default(c).0.contains(M0_FAKES)
}

/// Whether the feature entry `entry` activates the optional dependency `dep`.
fn activates(entry: &str, dep: &str) -> bool {
    let target = entry.strip_prefix("dep:").unwrap_or(entry);
    // `dep?/feature` enables a feature only if something else activates `dep`.
    target == dep
        || target
            .strip_prefix(dep)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Whether the feature entry `entry` enables the operator's `m0-fakes`, weakly or not.
fn enables_operator_m0_fakes(entry: &str) -> bool {
    entry
        .strip_prefix(OPERATOR)
        .and_then(|rest| rest.strip_prefix('?').or(Some(rest)))
        .and_then(|rest| rest.strip_prefix('/'))
        == Some(M0_FAKES)
}

/// The features `default` enables, directly or through other features, and every entry of
/// those features.
fn enabled_by_default(c: &Package) -> (BTreeSet<&str>, BTreeSet<&str>) {
    let mut enabled = BTreeSet::new();
    let mut entries = BTreeSet::new();
    let mut pending = vec!["default"];
    while let Some(feature) = pending.pop() {
        if !enabled.insert(feature) {
            continue;
        }
        for entry in c.features.get(feature).into_iter().flatten() {
            entries.insert(entry.as_str());
            if c.features.contains_key(entry.as_str()) {
                pending.push(entry.as_str());
            }
        }
    }
    (enabled, entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cargo::Binary;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn dep(name: &str, kind: Kind) -> Dependency {
        Dependency {
            name: name.to_owned(),
            kind,
            optional: false,
            default_features: true,
            features: Vec::new(),
        }
    }

    fn krate(name: &str, dependencies: Vec<Dependency>) -> Package {
        Package {
            name: name.to_owned(),
            dependencies,
            ..Package::default()
        }
    }

    /// The nine workspace crates with no dependency, plus `devtools`.
    fn workspace() -> Vec<Package> {
        [
            KERNEL,
            KERNEL_DERIVE,
            "autobot-api",
            CONTROLLERS,
            OPERATOR,
            "autobot-adapters",
            FAKES,
            TESTKIT,
            "autobot-cli",
            DEVTOOLS,
        ]
        .into_iter()
        .map(|n| krate(n, Vec::new()))
        .collect()
    }

    /// The workspace with `c` replacing the crate of the same name.
    fn with(c: Package) -> Vec<Package> {
        let mut crates = workspace();
        let slot = crates.iter_mut().find(|x| x.name == c.name);
        if let Some(slot) = slot {
            *slot = c;
        } else {
            crates.push(c);
        }
        crates
    }

    fn short(_: &Path) -> Result<usize> {
        Ok(5)
    }

    fn violations(crates: &[Package]) -> Vec<String> {
        check(crates, short)
            .unwrap_or_else(|e| panic!("{e}"))
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    fn operator_with_fakes(features: &[(&str, &[&str])]) -> Package {
        Package {
            name: OPERATOR.to_owned(),
            dependencies: vec![Dependency {
                optional: true,
                ..dep(FAKES, Kind::Normal)
            }],
            features: features
                .iter()
                .map(|(f, e)| ((*f).to_owned(), e.iter().map(|s| (*s).to_owned()).collect()))
                .collect(),
            ..Package::default()
        }
    }

    #[test]
    fn empty_workspace_passes() {
        assert_eq!(violations(&workspace()), Vec::<String>::new());
    }

    #[test]
    fn kernel_on_tokio_fails() {
        let v = violations(&with(krate(KERNEL, vec![dep("tokio", Kind::Normal)])));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(
            v[0].starts_with("autobot-kernel: normal dependency on `tokio`"),
            "{v:?}"
        );
    }

    #[test]
    fn kernel_on_kube_or_k8s_openapi_fails() {
        for banned in ["kube", "k8s-openapi"] {
            let v = violations(&with(krate(KERNEL, vec![dep(banned, Kind::Normal)])));
            assert_eq!(v.len(), 1, "{banned}: {v:?}");
        }
    }

    #[test]
    fn kernel_on_schemars_serde_and_derive_passes() {
        let deps = vec![
            dep("schemars", Kind::Normal),
            dep("serde", Kind::Normal),
            dep(KERNEL_DERIVE, Kind::Normal),
        ];
        assert_eq!(violations(&with(krate(KERNEL, deps))), Vec::<String>::new());
    }

    #[test]
    fn kernel_on_another_workspace_crate_fails() {
        let v = violations(&with(krate(KERNEL, vec![dep("autobot-api", Kind::Normal)])));
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].contains("`autobot-api`"), "{v:?}");
    }

    #[test]
    fn kernel_dev_dependency_on_tokio_passes() {
        let v = violations(&with(krate(KERNEL, vec![dep("tokio", Kind::Dev)])));
        assert_eq!(v, Vec::<String>::new());
    }

    #[test]
    fn kernel_dev_dependency_on_devtools_with_default_features_fails() {
        let v = violations(&with(krate(KERNEL, vec![dep(DEVTOOLS, Kind::Dev)])));
        assert_eq!(
            v,
            vec![format!(
                "autobot-kernel: dev-dependency on `{DEVTOOLS}` must set `default-features = false`"
            )]
        );
    }

    #[test]
    fn kernel_dev_dependency_on_devtools_without_default_features_passes() {
        let d = Dependency {
            default_features: false,
            ..dep(DEVTOOLS, Kind::Dev)
        };
        assert_eq!(
            violations(&with(krate(KERNEL, vec![d]))),
            Vec::<String>::new()
        );
    }

    #[test]
    fn dev_dependency_on_fakes_passes() {
        let v = violations(&with(krate(CONTROLLERS, vec![dep(FAKES, Kind::Dev)])));
        assert_eq!(v, Vec::<String>::new());
    }

    #[test]
    fn normal_dependency_on_testkit_fails() {
        let v = violations(&with(krate(CONTROLLERS, vec![dep(TESTKIT, Kind::Normal)])));
        assert_eq!(
            v,
            vec![format!(
                "{CONTROLLERS}: normal dependency on `{TESTKIT}`: test crates are dev-dependencies only"
            )]
        );
    }

    #[test]
    fn build_dependency_on_fakes_fails() {
        let v = violations(&with(krate("autobot-cli", vec![dep(FAKES, Kind::Build)])));
        assert_eq!(v.len(), 1, "{v:?}");
    }

    #[test]
    fn testkit_on_fakes_normal_edge_passes() {
        let v = violations(&with(krate(TESTKIT, vec![dep(FAKES, Kind::Normal)])));
        assert_eq!(v, Vec::<String>::new());
    }

    #[test]
    fn fakes_on_testkit_normal_edge_fails() {
        let v = violations(&with(krate(FAKES, vec![dep(TESTKIT, Kind::Normal)])));
        assert_eq!(v.len(), 1, "{v:?}");
    }

    #[test]
    fn operator_m0_fakes_feature_edge_passes() {
        let op = operator_with_fakes(&[(M0_FAKES, &["dep:autobot-fakes"])]);
        assert_eq!(violations(&with(op)), Vec::<String>::new());
    }

    #[test]
    fn operator_m0_fakes_in_default_fails() {
        let op = operator_with_fakes(&[
            ("default", &["all"]),
            ("all", &[M0_FAKES]),
            (M0_FAKES, &["dep:autobot-fakes"]),
        ]);
        assert_eq!(violations(&with(op)).len(), 1);
    }

    #[test]
    fn operator_fakes_behind_implicit_feature_fails() {
        // `m0-fakes = ["autobot-fakes"]` leaves the implicit `autobot-fakes` feature in place.
        let op = operator_with_fakes(&[
            (M0_FAKES, &["autobot-fakes"]),
            (FAKES, &["dep:autobot-fakes"]),
        ]);
        assert_eq!(violations(&with(op)).len(), 1);
    }

    #[test]
    fn operator_fakes_behind_another_feature_fails() {
        let op = operator_with_fakes(&[("sim", &["dep:autobot-fakes"])]);
        assert_eq!(violations(&with(op)).len(), 1);
    }

    #[test]
    fn operator_non_optional_fakes_fails() {
        let mut op = operator_with_fakes(&[(M0_FAKES, &["dep:autobot-fakes"])]);
        op.dependencies[0].optional = false;
        assert_eq!(violations(&with(op)).len(), 1);
    }

    #[test]
    fn dependent_enabling_operator_m0_fakes_fails() {
        for kind in [Kind::Normal, Kind::Build] {
            let d = Dependency {
                features: vec![M0_FAKES.to_owned()],
                ..dep(OPERATOR, kind)
            };
            let v = violations(&with(krate("autobot-cli", vec![d])));
            assert_eq!(
                v,
                vec![format!(
                    "autobot-cli: {} dependency on `{OPERATOR}` enables `{M0_FAKES}`, which links `{FAKES}` into a normal build",
                    kind.name()
                )]
            );
        }
    }

    #[test]
    fn dependent_dev_dependency_enabling_operator_m0_fakes_passes() {
        let d = Dependency {
            features: vec![M0_FAKES.to_owned()],
            ..dep(OPERATOR, Kind::Dev)
        };
        let v = violations(&with(krate("autobot-cli", vec![d])));
        assert_eq!(v, Vec::<String>::new());
    }

    #[test]
    fn dependent_default_feature_enabling_operator_m0_fakes_fails() {
        for entry in ["autobot-operator/m0-fakes", "autobot-operator?/m0-fakes"] {
            let cli = Package {
                features: BTreeMap::from([
                    ("default".to_owned(), vec!["sim".to_owned()]),
                    ("sim".to_owned(), vec![entry.to_owned()]),
                ]),
                ..krate("autobot-cli", vec![dep(OPERATOR, Kind::Normal)])
            };
            let v = violations(&with(cli));
            assert_eq!(
                v,
                vec![format!(
                    "autobot-cli: default features enable `{entry}`, which links `{FAKES}` into a normal build"
                )],
                "{entry}"
            );
        }
    }

    #[test]
    fn dependent_non_default_feature_enabling_operator_m0_fakes_passes() {
        let cli = Package {
            features: BTreeMap::from([(
                "sim".to_owned(),
                vec!["autobot-operator/m0-fakes".to_owned()],
            )]),
            ..krate("autobot-cli", vec![dep(OPERATOR, Kind::Normal)])
        };
        assert_eq!(violations(&with(cli)), Vec::<String>::new());
    }

    #[test]
    fn operator_m0_fakes_entry_matching() {
        assert!(enables_operator_m0_fakes("autobot-operator/m0-fakes"));
        assert!(enables_operator_m0_fakes("autobot-operator?/m0-fakes"));
        assert!(!enables_operator_m0_fakes("autobot-operator/other"));
        assert!(!enables_operator_m0_fakes("autobot-operator/m0-fakes-x"));
        assert!(!enables_operator_m0_fakes("autobot-operator-x/m0-fakes"));
        assert!(!enables_operator_m0_fakes("m0-fakes"));
    }

    #[test]
    fn weak_feature_reference_does_not_activate() {
        assert!(!activates("autobot-fakes?/x", FAKES));
        assert!(activates("autobot-fakes/x", FAKES));
        assert!(activates("dep:autobot-fakes", FAKES));
        assert!(!activates("autobot-fakes-extra", FAKES));
    }

    #[test]
    fn api_and_adapters_depend_only_on_the_kernel() {
        for c in KERNEL_ONLY {
            let ok = krate(
                c,
                vec![dep(KERNEL, Kind::Normal), dep("serde", Kind::Normal)],
            );
            assert_eq!(violations(&with(ok)), Vec::<String>::new(), "{c}");
            let bad = krate(c, vec![dep(CONTROLLERS, Kind::Normal)]);
            assert_eq!(violations(&with(bad)).len(), 1, "{c}");
        }
    }

    #[test]
    fn controllers_on_a_binary_crate_fails() {
        for b in CONTROLLERS_BANNED {
            let v = violations(&with(krate(CONTROLLERS, vec![dep(b, Kind::Dev)])));
            assert_eq!(v.len(), 1, "{b}: {v:?}");
        }
    }

    #[test]
    fn long_binary_source_fails() {
        let cli = Package {
            binaries: vec![Binary {
                name: "autobot".to_owned(),
                src_path: PathBuf::from("/w/crates/autobot-cli/src/main.rs"),
            }],
            ..krate("autobot-cli", Vec::new())
        };
        let crates = with(cli);
        let at_bound = check(&crates, |_| Ok(MAX_MAIN_LINES)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(at_bound, Vec::new());
        let over = check(&crates, |_| Ok(MAX_MAIN_LINES + 1)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(over.len(), 1);
        assert!(
            over[0].message.starts_with("binary `autobot` has 41 lines"),
            "{over:?}"
        );
    }
}
