//! `just doctor`: checks the local build setup, and measures the sccache hit split.
//!
//! The checks, each reported as `ok` or `FAIL`:
//!
//! - every development tool answers `--version`: rust-script, just, cargo-nextest,
//!   cargo-deny, cargo-machete and sccache;
//! - the effective rustc wrapper is sccache and comes from the home Cargo configuration
//!   (`$CARGO_HOME/config`, or `$CARGO_HOME/config.toml` when there is no `config`, with
//!   `CARGO_HOME` defaulting to `~/.cargo`; see [`cargo_configs`]), not from the environment
//!   nor from a `.cargo/config` or `.cargo/config.toml` in the repository or above it;
//! - the sccache server is listening (`SCCACHE_SERVER_UDS`, or TCP port
//!   `SCCACHE_SERVER_PORT`, default 4226), checked by connecting so that the check never
//!   starts a server;
//! - the server's local cache directory and the `.worktrees` directory of the main working
//!   tree, followed through symlinks, are on the filesystem whose UUID
//!   `~/.config/autobot/local.toml` names as `require_mount_uuid`, when it names one (see
//!   [`crate::worktree::Settings`]);
//! - the Justfile exports no `CARGO_*` variable, read from `just --dump --dump-format json`:
//!   no exported assignment or recipe parameter of that name, and no dotenv loading (by
//!   `dotenv-load`, `dotenv-required`, `dotenv-override`, `dotenv-filename` or `dotenv-path`).
//!
//! [`measure`] reports how many Rust compilations sccache served from its cache and how many
//! it missed, separately for the dependencies and for the workspace. It builds into
//! `target/doctor`, which it empties first, so every run starts cold and the worktree's own
//! `target/` is left alone. It zeroes the server's counters, builds every non-workspace
//! package of the resolve for the host platform (`cargo build --locked -p <name>@<version>`)
//! and reads the counters as the dependency split; then zeroes them again and builds the
//! workspace, whose dependencies are now fresh, and reads the workspace split. The counters
//! belong to the one server every worktree shares, so a build running elsewhere at the same
//! time is counted too.

use crate::process::Cmd;
use crate::worktree::{Settings, findmnt_uuid, local_config_dir, main_worktree, refusal};
use crate::{Error, Result};
use std::path::{Path, PathBuf};

/// The tools checked, each with the arguments printing its version.
pub const TOOLS: [(&str, &[&str]); 6] = [
    ("rust-script", &["rust-script", "--version"]),
    ("just", &["just", "--version"]),
    ("cargo-nextest", &["cargo", "nextest", "--version"]),
    ("cargo-deny", &["cargo", "deny", "--version"]),
    ("cargo-machete", &["cargo", "machete", "--version"]),
    ("sccache", &["sccache", "--version"]),
];

/// The TCP port the sccache server listens on unless `SCCACHE_SERVER_PORT` says otherwise.
pub const SCCACHE_DEFAULT_PORT: u16 = 4226;

/// Directory, under the worktree, that [`measure`] builds into.
pub const MEASURE_TARGET_DIR: &str = "target/doctor";

/// The outcome of one check: a label and either what was found or why it failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// What was checked.
    pub label: String,
    /// `Ok` with what was found, or `Err` with why the check failed.
    pub outcome: Result<String, String>,
}

impl Check {
    fn new(label: impl Into<String>, outcome: Result<String>) -> Self {
        Self {
            label: label.into(),
            outcome: outcome.map_err(|e| e.to_string()),
        }
    }

    /// The report line: `ok   <label>: <found>` or `FAIL <label>: <reason>`.
    #[must_use]
    pub fn line(&self) -> String {
        match &self.outcome {
            Ok(found) => format!("ok   {}: {found}", self.label),
            Err(why) => format!("FAIL {}: {why}", self.label),
        }
    }
}

/// Where the effective rustc wrapper is configured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WrapperSource {
    /// An environment variable of that name.
    Env(String),
    /// A Cargo configuration file.
    Config(PathBuf),
}

/// The rustc wrapper Cargo would use, and where it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wrapper {
    /// The configured program.
    pub program: String,
    /// Where it is configured.
    pub source: WrapperSource,
}

/// The `build.rustc-wrapper` value of a Cargo configuration file, whatever TOML form sets it.
///
/// # Errors
/// Fails if the text is not TOML or the value is not a string.
pub fn config_wrapper(text: &str) -> Result<Option<String>> {
    let table: toml::Table = text.parse().map_err(|e| Error::Parse(format!("{e}")))?;
    match table.get("build").and_then(|b| b.get("rustc-wrapper")) {
        None => Ok(None),
        Some(toml::Value::String(s)) => Ok(Some(s.clone())),
        Some(other) => Err(Error::Parse(format!(
            "`build.rustc-wrapper` must be a string, got `{other:?}`"
        ))),
    }
}

/// The wrapper Cargo would use: `RUSTC_WRAPPER`, else `CARGO_BUILD_RUSTC_WRAPPER`, else the
/// first of `configs` (path and text, highest precedence first) that sets it.
///
/// # Errors
/// Fails if a configuration file cannot be parsed.
pub fn effective_wrapper(
    env: &dyn Fn(&str) -> Option<String>,
    configs: &[(PathBuf, String)],
) -> Result<Option<Wrapper>> {
    for var in ["RUSTC_WRAPPER", "CARGO_BUILD_RUSTC_WRAPPER"] {
        if let Some(program) = env(var) {
            return Ok(Some(Wrapper {
                program,
                source: WrapperSource::Env(var.to_owned()),
            }));
        }
    }
    for (path, text) in configs {
        let wrapper =
            config_wrapper(text).map_err(|e| Error::Parse(format!("{}: {e}", path.display())))?;
        if let Some(program) = wrapper {
            return Ok(Some(Wrapper {
                program,
                source: WrapperSource::Config(path.clone()),
            }));
        }
    }
    Ok(None)
}

/// Checks that `wrapper` is sccache configured in the home Cargo configuration `home`.
///
/// # Errors
/// Fails, with the reason, if there is no wrapper, if it is not sccache, or if it is
/// configured anywhere but `home`.
pub fn check_wrapper(wrapper: Option<&Wrapper>, home: &Path) -> Result<String> {
    let op = "rustc wrapper";
    let Some(w) = wrapper else {
        return Err(refusal(
            op,
            format!(
                "none set; add `[build] rustc-wrapper = \"sccache\"` to {}",
                home.display()
            ),
        ));
    };
    let from = match &w.source {
        WrapperSource::Env(var) => format!("environment variable {var}"),
        WrapperSource::Config(path) => path.display().to_string(),
    };
    if Path::new(&w.program).file_stem().and_then(|s| s.to_str()) != Some("sccache") {
        return Err(refusal(
            op,
            format!("`{}` from {from} is not sccache", w.program),
        ));
    }
    if w.source != WrapperSource::Config(home.to_path_buf()) {
        return Err(refusal(
            op,
            format!(
                "`{}` comes from {from}; configure it only in {}",
                w.program,
                home.display()
            ),
        ));
    }
    Ok(format!("`{}` from {from}", w.program))
}

/// The Cargo configuration files that apply in `dir`, highest precedence first, with their
/// text: `.cargo/config` (or `.cargo/config.toml`) in `dir` and each ancestor, then the home
/// configuration `<cargo_home>/config` (or `<cargo_home>/config.toml`). Where both names exist
/// in one directory, the one without the extension is taken, as Cargo does. The home file is
/// listed once even when it is also an ancestor's. Returns the path of the home file as well.
///
/// # Errors
/// Fails if an existing file cannot be read.
pub fn cargo_configs(dir: &Path, cargo_home: &Path) -> Result<(PathBuf, Vec<(PathBuf, String)>)> {
    let pick = |base: &Path| -> Option<PathBuf> {
        [base.join("config"), base.join("config.toml")]
            .into_iter()
            .find(|p| p.is_file())
    };
    let home = pick(cargo_home).unwrap_or_else(|| cargo_home.join("config.toml"));
    let home_canon = std::fs::canonicalize(&home).ok();
    let mut paths: Vec<PathBuf> = dir
        .ancestors()
        .filter_map(|d| pick(&d.join(".cargo")))
        .filter(|p| home_canon.is_none() || std::fs::canonicalize(p).ok() != home_canon)
        .collect();
    paths.push(home.clone());
    let mut configs = Vec::new();
    for path in paths {
        match std::fs::read_to_string(&path) {
            Ok(text) => configs.push((path, text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(refusal(&format!("read {}", path.display()), e.to_string())),
        }
    }
    Ok((home, configs))
}

/// Resolves `path` through symlinks and, when `want` is set, checks that the filesystem
/// holding it has that UUID according to `mount_uuid`. Returns the resolved path.
///
/// # Errors
/// Fails if `path` does not lead to an existing directory, or if the UUID differs or is
/// unknown.
pub fn check_volume(
    label: &str,
    path: &Path,
    want: Option<&str>,
    mount_uuid: &dyn Fn(&Path) -> Result<Option<String>>,
) -> Result<PathBuf> {
    let resolved = std::fs::canonicalize(path)
        .ok()
        .filter(|p| p.is_dir())
        .ok_or_else(|| {
            refusal(
                label,
                format!("{} does not lead to an existing directory", path.display()),
            )
        })?;
    if let Some(want) = want {
        let got = mount_uuid(&resolved)?;
        if got.as_deref() != Some(want) {
            return Err(refusal(
                label,
                format!(
                    "{} is on a filesystem with UUID {}, but local.toml requires {want}",
                    resolved.display(),
                    got.as_deref().unwrap_or("<none>"),
                ),
            ));
        }
    }
    Ok(resolved)
}

/// The local cache directory from the `cache_location` of `sccache --show-stats
/// --stats-format json` (`Local disk: "<path>"`).
///
/// # Errors
/// Fails if the JSON has no `cache_location` or the cache is not on local disk.
pub fn cache_dir(stats: &serde_json::Value) -> Result<PathBuf> {
    let location = stats["cache_location"]
        .as_str()
        .ok_or_else(|| Error::Parse("sccache stats without `cache_location`".into()))?;
    location
        .strip_prefix("Local disk: ")
        .and_then(|quoted| serde_json::from_str::<String>(quoted).ok())
        .map(PathBuf::from)
        .ok_or_else(|| {
            Error::Parse(format!(
                "sccache cache is not a local directory: {location}"
            ))
        })
}

/// The `CARGO_*` variables a Justfile exports, from its `just --dump --dump-format json`:
/// assignments and recipe parameters marked `export`, or all of them under `set export`,
/// in the root and every module. A module that loads a dotenv file yields an entry naming
/// that, because a dotenv file's variables are exported too.
#[must_use]
pub fn cargo_exports(dump: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    let settings = &dump["settings"];
    let export_all = settings["export"].as_bool() == Some(true);
    // `just` loads a dotenv file under any of these settings, not only `dotenv-load`.
    let dotenv = ["dotenv_load", "dotenv_required", "dotenv_override"]
        .iter()
        .any(|k| settings[*k].as_bool() == Some(true))
        || !settings["dotenv_filename"].is_null()
        || !settings["dotenv_path"].is_null();
    if dotenv {
        out.push("(a dotenv file, whose variables are exported)".to_owned());
    }
    let exported = |v: &serde_json::Value| export_all || v["export"].as_bool() == Some(true);
    let cargo = |v: &serde_json::Value| {
        v["name"]
            .as_str()
            .filter(|n| n.starts_with("CARGO_"))
            .map(str::to_owned)
    };
    let objects = |key: &str| dump[key].as_object().into_iter().flat_map(|m| m.values());
    for a in objects("assignments").filter(|a| exported(a)) {
        out.extend(cargo(a));
    }
    for r in objects("recipes") {
        for p in r["parameters"].as_array().into_iter().flatten() {
            if exported(p) {
                out.extend(cargo(p).map(|n| format!("{n} (parameter of {})", r["name"])));
            }
        }
    }
    for m in objects("modules") {
        out.extend(cargo_exports(m));
    }
    out
}

/// The sccache counters of one build phase.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Split {
    /// Rust compilations served from the cache.
    pub hits: u64,
    /// Rust compilations looked up and not found.
    pub misses: u64,
    /// Requests sccache did not cache, all languages.
    pub not_cacheable: u64,
    /// Why they were not cached, by reason, sorted by reason.
    pub not_cached: Vec<(String, u64)>,
}

impl std::fmt::Display for Split {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} Rust hits, {} Rust misses, {} not cacheable",
            self.hits, self.misses, self.not_cacheable
        )?;
        if !self.not_cached.is_empty() {
            let reasons: Vec<String> = self
                .not_cached
                .iter()
                .map(|(r, n)| format!("{r}: {n}"))
                .collect();
            write!(f, " ({})", reasons.join(", "))?;
        }
        Ok(())
    }
}

/// Reads the Rust hit and miss counts and the non-cacheable requests out of `sccache
/// --show-stats --stats-format json`.
///
/// # Errors
/// Fails if the JSON has no `stats` object.
pub fn parse_split(stats: &serde_json::Value) -> Result<Split> {
    let s = stats["stats"]
        .as_object()
        .ok_or_else(|| Error::Parse("sccache stats without `stats`".into()))?;
    let rust = |key: &str| {
        s.get(key)
            .and_then(|v| v["counts"]["Rust"].as_u64())
            .unwrap_or(0)
    };
    let mut not_cached: Vec<(String, u64)> = s
        .get("not_cached")
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| v.as_u64().map(|n| (k.clone(), n)))
        .collect();
    not_cached.sort();
    Ok(Split {
        hits: rust("cache_hits"),
        misses: rust("cache_misses"),
        not_cacheable: s
            .get("requests_not_cacheable")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        not_cached,
    })
}

/// `<name>@<version>` of every package in the resolve of `cargo metadata --format-version 1`
/// output that is not a workspace member, sorted.
///
/// # Errors
/// Fails if the JSON lacks the resolve, the member list or a package's name or version.
pub fn external_packages(metadata: &serde_json::Value) -> Result<Vec<String>> {
    let array = |v: &serde_json::Value, what: &str| {
        v.as_array()
            .ok_or_else(|| Error::Parse(format!("cargo metadata without `{what}`")))
            .cloned()
    };
    let members = array(&metadata["workspace_members"], "workspace_members")?;
    let nodes = array(&metadata["resolve"]["nodes"], "resolve.nodes")?;
    let packages = array(&metadata["packages"], "packages")?;
    let mut out = Vec::new();
    for node in nodes.iter().filter(|n| !members.contains(&n["id"])) {
        let package = packages
            .iter()
            .find(|p| p["id"] == node["id"])
            .ok_or_else(|| Error::Parse(format!("no package for resolve node {}", node["id"])))?;
        match (package["name"].as_str(), package["version"].as_str()) {
            (Some(name), Some(version)) => out.push(format!("{name}@{version}")),
            _ => {
                return Err(Error::Parse(format!(
                    "package {} without name or version",
                    node["id"]
                )));
            }
        }
    }
    out.sort();
    Ok(out)
}

fn json(cmd: &Cmd) -> Result<serde_json::Value> {
    serde_json::from_str(&cmd.output()?)
        .map_err(|e| Error::Parse(format!("{}: {e}", cmd.display())))
}

fn sccache_stats() -> Result<serde_json::Value> {
    json(&Cmd::new("sccache").args(["--show-stats", "--stats-format", "json"]))
}

/// Connects to the sccache server without starting one; returns where it listens.
fn sccache_listening() -> Result<String> {
    let op = "sccache server";
    if let Some(socket) = std::env::var_os("SCCACHE_SERVER_UDS") {
        std::os::unix::net::UnixStream::connect(&socket)
            .map_err(|e| refusal(op, format!("nothing listens on {}: {e}", socket.display())))?;
        return Ok(format!("listening on {}", socket.display()));
    }
    let port = match std::env::var("SCCACHE_SERVER_PORT") {
        Ok(p) => p
            .parse::<u16>()
            .map_err(|_| refusal(op, format!("SCCACHE_SERVER_PORT `{p}` is not a port")))?,
        Err(_) => SCCACHE_DEFAULT_PORT,
    };
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(2)).map_err(
        |e| {
            refusal(
                op,
                format!("nothing listens on {addr} ({e}); start it with `sccache --start-server`"),
            )
        },
    )?;
    Ok(format!("listening on {addr}"))
}

fn tool_version(argv: &[&str]) -> Result<String> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| refusal("tool", "empty command"))?;
    let out = Cmd::new(*program).args(args.iter().copied()).output()?;
    Ok(out.lines().next().unwrap_or_default().trim().to_owned())
}

fn cargo_home() -> Result<PathBuf> {
    match std::env::var_os("CARGO_HOME") {
        Some(home) => Ok(PathBuf::from(home)),
        None => std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join(".cargo"))
            .ok_or_else(|| {
                refusal(
                    "locate the Cargo home",
                    "neither CARGO_HOME nor HOME is set",
                )
            }),
    }
}

/// Runs every check for the worktree at `root` and returns their outcomes, in report order.
#[must_use]
pub fn checks(root: &Path) -> Vec<Check> {
    let mut out: Vec<Check> = TOOLS
        .iter()
        .map(|(name, argv)| Check::new(*name, tool_version(argv)))
        .collect();

    let wrapper = (|| {
        let (home, configs) = cargo_configs(root, &cargo_home()?)?;
        let env = |k: &str| std::env::var(k).ok();
        check_wrapper(effective_wrapper(&env, &configs)?.as_ref(), &home)
    })();
    out.push(Check::new("rustc wrapper", wrapper));

    let listening = sccache_listening();
    let running = listening.is_ok();
    out.push(Check::new("sccache server", listening));

    let settings = local_config_dir().and_then(|d| Settings::load(&d.join("local.toml")));
    let want = settings
        .as_ref()
        .ok()
        .and_then(|s| s.require_mount_uuid.clone());
    if let Err(e) = &settings {
        out.push(Check::new(
            "local.toml",
            Err(refusal("local.toml", e.to_string())),
        ));
    }
    let volume = |label: &str, path: Result<PathBuf>| {
        path.and_then(|p| check_volume(label, &p, want.as_deref(), &findmnt_uuid))
            .map(|p| match &want {
                Some(uuid) => format!("{} on UUID {uuid}", p.display()),
                None => format!("{} (no require_mount_uuid)", p.display()),
            })
    };
    let cache = if running {
        sccache_stats().and_then(|s| cache_dir(&s))
    } else {
        Err(refusal(
            "sccache cache directory",
            "the server is not running",
        ))
    };
    out.push(Check::new(
        "sccache cache directory",
        volume("sccache cache directory", cache),
    ));
    let worktrees = main_worktree(root).map(|m| m.join(".worktrees"));
    out.push(Check::new(".worktrees", volume(".worktrees", worktrees)));

    let exports = json(
        &Cmd::new("just")
            .args(["--dump", "--dump-format", "json", "--justfile"])
            .args([root.join("Justfile").to_string_lossy()]),
    )
    .map(|dump| cargo_exports(&dump))
    .and_then(|found| {
        if found.is_empty() {
            Ok("none".to_owned())
        } else {
            Err(refusal("Justfile", format!("exports {}", found.join(", "))))
        }
    });
    out.push(Check::new("Justfile CARGO_* exports", exports));
    out
}

/// Measures the dependency and workspace hit splits of the worktree at `root`.
///
/// # Errors
/// Fails if a command fails or prints unexpected output.
pub fn measure(root: &Path) -> Result<(Split, Split)> {
    let host = Cmd::new("rustc").args(["-vV"]).current_dir(root).output()?;
    let host = host
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .ok_or_else(|| Error::Parse("`rustc -vV` printed no host".into()))?
        .to_owned();
    let metadata = json(
        &Cmd::new("cargo")
            .args([
                "metadata",
                "--format-version",
                "1",
                "--locked",
                "--filter-platform",
                &host,
            ])
            .current_dir(root),
    )?;
    let external = external_packages(&metadata)?;
    let target = root.join(MEASURE_TARGET_DIR);
    match std::fs::remove_dir_all(&target) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(refusal(
                &format!("empty {}", target.display()),
                e.to_string(),
            ));
        }
        _ => {}
    }
    let target = target.to_string_lossy().into_owned();
    let zero = || {
        Cmd::new("sccache")
            .args(["--zero-stats"])
            .output()
            .map(drop)
    };
    let build = |selection: Vec<String>| {
        Cmd::new("cargo")
            .args(["build", "--locked", "--target-dir", &target])
            .args(selection)
            .current_dir(root)
            .run()
    };

    zero()?;
    build(
        external
            .iter()
            .flat_map(|p| ["-p".to_owned(), p.clone()])
            .collect(),
    )?;
    let dependencies = parse_split(&sccache_stats()?)?;
    zero()?;
    build(vec!["--workspace".to_owned()])?;
    let workspace = parse_split(&sccache_stats()?)?;
    Ok((dependencies, workspace))
}

/// Entry point of `scripts/doctor.rs`: the checks, then with `--measure` the hit split,
/// for the worktree in the current directory.
///
/// # Errors
/// Fails on bad arguments, when any check fails (the measurement is then skipped), and when
/// the measurement fails.
pub fn cli(args: &[String]) -> Result<()> {
    let measuring = match args {
        [] => false,
        [flag] if flag == "--measure" => true,
        _ => return Err(refusal("doctor", "usage: doctor [--measure]")),
    };
    let root = PathBuf::from(crate::git::toplevel(".")?);
    let results = checks(&root);
    for c in &results {
        println!("{}", c.line());
    }
    let failed = results.iter().filter(|c| c.outcome.is_err()).count();
    if failed > 0 {
        return Err(refusal("doctor", format!("{failed} check(s) failed")));
    }
    if measuring {
        let (dependencies, workspace) = measure(&root)?;
        println!("dependencies: {dependencies}");
        println!("workspace:    {workspace}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree::test_support::TempDir;

    fn uuid(answer: Option<&'static str>) -> impl Fn(&Path) -> Result<Option<String>> {
        move |_| Ok(answer.map(str::to_owned))
    }

    fn no_lookup(_: &Path) -> Result<Option<String>> {
        panic!("the mount lookup must not run without require_mount_uuid")
    }

    #[test]
    fn volume_check_fails_when_a_directory_resolves_off_the_named_volume() {
        let tmp = TempDir::new();
        let volume = tmp.0.join("volume");
        std::fs::create_dir(&volume).unwrap();
        for name in ["sccache", ".worktrees"] {
            let link = tmp.0.join(name);
            std::os::unix::fs::symlink(&volume, &link).unwrap();
            let asked = std::cell::RefCell::new(Vec::new());
            let lookup = |p: &Path| {
                asked.borrow_mut().push(p.to_path_buf());
                Ok(Some("internal-disk".to_owned()))
            };
            let err = check_volume(name, &link, Some("usb"), &lookup)
                .unwrap_err()
                .to_string();
            assert!(err.contains(name), "{err}");
            assert!(
                err.contains("UUID internal-disk, but local.toml requires usb"),
                "{err}"
            );
            // The lookup sees the symlink's target, not the link.
            assert_eq!(asked.borrow().as_slice(), std::slice::from_ref(&volume));

            let err = check_volume(name, &link, Some("usb"), &uuid(None)).unwrap_err();
            assert!(err.to_string().contains("UUID <none>"), "{err}");
            assert_eq!(
                check_volume(name, &link, Some("usb"), &uuid(Some("usb"))).unwrap(),
                volume
            );
            assert_eq!(check_volume(name, &link, None, &no_lookup).unwrap(), volume);
        }
    }

    #[test]
    fn volume_check_fails_on_a_dangling_symlink() {
        let tmp = TempDir::new();
        let link = tmp.0.join(".worktrees");
        std::os::unix::fs::symlink(tmp.0.join("unmounted"), &link).unwrap();
        let err = check_volume(".worktrees", &link, None, &no_lookup).unwrap_err();
        assert!(
            err.to_string()
                .contains("does not lead to an existing directory"),
            "{err}"
        );
    }

    #[test]
    fn config_wrapper_reads_every_toml_form() {
        for text in [
            "[build]\nrustc-wrapper = \"sccache\"\njobs = 4\n",
            "build.rustc-wrapper = \"sccache\"\n",
            "build = { rustc-wrapper = \"sccache\" }\n",
        ] {
            assert_eq!(
                config_wrapper(text).unwrap().as_deref(),
                Some("sccache"),
                "{text}"
            );
        }
        assert_eq!(config_wrapper("[profile.dev]\ndebug = 1\n").unwrap(), None);
        assert!(config_wrapper("[build]\nrustc-wrapper = 1\n").is_err());
        assert!(config_wrapper("[build\n").is_err());
    }

    #[test]
    fn wrapper_must_be_sccache_from_the_home_config() {
        let home = PathBuf::from("/h/.cargo/config.toml");
        let repo = PathBuf::from("/r/.cargo/config.toml");
        let set = |p: &Path, w: &str| {
            (
                p.to_path_buf(),
                format!("[build]\nrustc-wrapper = \"{w}\"\n"),
            )
        };
        let unset = |p: &Path| (p.to_path_buf(), "[profile.dev]\ndebug = 1\n".to_owned());
        let no_env = |_: &str| None;
        let run = |env: &dyn Fn(&str) -> Option<String>, configs: &[(PathBuf, String)]| {
            check_wrapper(effective_wrapper(env, configs).unwrap().as_ref(), &home)
                .map_err(|e| e.to_string())
        };

        let ok = run(&no_env, &[unset(&repo), set(&home, "/usr/bin/sccache")]).unwrap();
        assert!(ok.contains("/h/.cargo/config.toml"), "{ok}");

        let err = run(&no_env, &[set(&repo, "sccache"), set(&home, "sccache")]).unwrap_err();
        assert!(err.contains("comes from /r/.cargo/config.toml"), "{err}");
        let err = run(&no_env, &[set(&home, "ccache")]).unwrap_err();
        assert!(err.contains("is not sccache"), "{err}");
        let err = run(&no_env, &[unset(&home)]).unwrap_err();
        assert!(err.contains("none set"), "{err}");
        let env = |k: &str| (k == "CARGO_BUILD_RUSTC_WRAPPER").then(|| "sccache".to_owned());
        let err = run(&env, &[set(&home, "sccache")]).unwrap_err();
        assert!(
            err.contains("environment variable CARGO_BUILD_RUSTC_WRAPPER"),
            "{err}"
        );
    }

    #[test]
    fn cargo_configs_lists_ancestors_before_home_once() {
        let tmp = TempDir::new();
        let home = tmp.0.join("home/.cargo");
        let repo = tmp.0.join("home/repo");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(repo.join(".cargo")).unwrap();
        std::fs::write(home.join("config.toml"), "h").unwrap();
        // Both names in one directory: Cargo reads `config`, so `config.toml` must be skipped.
        std::fs::write(repo.join(".cargo/config"), "r").unwrap();
        std::fs::write(repo.join(".cargo/config.toml"), "ignored").unwrap();

        let (home_file, configs) = cargo_configs(&repo, &home).unwrap();
        assert_eq!(home_file, home.join("config.toml"));
        // Ancestors of the temporary directory may hold real configuration files of this
        // machine; only the entries created here are compared, and the home file stays last.
        assert_eq!(configs.last().map(|(p, _)| p), Some(&home_file));
        let texts: Vec<&str> = configs
            .iter()
            .filter(|(p, _)| p.starts_with(&tmp.0))
            .map(|(_, t)| t.as_str())
            .collect();
        // `home/.cargo` is also the `.cargo` of an ancestor of `repo`, but is listed once, last.
        assert_eq!(texts, ["r", "h"]);
    }

    #[test]
    fn cargo_configs_prefers_config_without_extension_in_home() {
        let tmp = TempDir::new();
        let home = tmp.0.join(".cargo");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join("config"), "plain").unwrap();
        std::fs::write(home.join("config.toml"), "toml").unwrap();

        let (home_file, configs) = cargo_configs(&tmp.0.join("elsewhere"), &home).unwrap();
        assert_eq!(home_file, home.join("config"));
        let texts: Vec<&str> = configs
            .iter()
            .filter(|(p, _)| p.starts_with(&tmp.0))
            .map(|(_, t)| t.as_str())
            .collect();
        assert_eq!(texts, ["plain"]);
    }

    /// `sccache --show-stats --stats-format json` from sccache 0.17.0, trimmed to the fields
    /// read.
    const STATS: &str = r#"{"stats":{"compile_requests":2128,"requests_not_cacheable":1463,
      "cache_hits":{"counts":{"Assembler":98,"C/C++":112,"Rust":248}},
      "cache_misses":{"counts":{"Rust":169,"C/C++":16}},
      "not_cached":{"incremental":469,"crate-type":139}},
      "cache_location":"Local disk: \"/home/u/.sccache\"","version":"0.17.0"}"#;

    #[test]
    fn stats_give_the_rust_split_and_the_cache_directory() {
        let stats: serde_json::Value = serde_json::from_str(STATS).unwrap();
        let split = parse_split(&stats).unwrap();
        assert_eq!(
            split,
            Split {
                hits: 248,
                misses: 169,
                not_cacheable: 1463,
                not_cached: vec![("crate-type".into(), 139), ("incremental".into(), 469)],
            }
        );
        assert_eq!(
            split.to_string(),
            "248 Rust hits, 169 Rust misses, 1463 not cacheable (crate-type: 139, incremental: 469)"
        );
        assert_eq!(
            cache_dir(&stats).unwrap(),
            PathBuf::from("/home/u/.sccache")
        );
        let zeroed = serde_json::json!({"stats": {"cache_hits": {"counts": {}}}});
        assert_eq!(parse_split(&zeroed).unwrap(), Split::default());
        let remote = serde_json::json!({"cache_location": "Redis: redis://x"});
        assert!(cache_dir(&remote).is_err());
        assert!(parse_split(&remote).is_err());
    }

    #[test]
    fn cargo_exports_finds_exported_cargo_names_only() {
        let dump = serde_json::json!({
            "settings": {"export": false, "dotenv_load": false, "dotenv_filename": null, "dotenv_path": null},
            "assignments": {
                "RUSTUP_TOOLCHAIN": {"name": "RUSTUP_TOOLCHAIN", "export": true},
                "CARGO_TARGET_DIR": {"name": "CARGO_TARGET_DIR", "export": true},
                "CARGO_LOCAL": {"name": "CARGO_LOCAL", "export": false}
            },
            "recipes": {"b": {"name": "b", "parameters": [
                {"name": "CARGO_PROFILE", "export": true}, {"name": "args", "export": true}
            ]}},
            "modules": {"m": {"settings": {"export": true},
                "assignments": {"CARGO_INCREMENTAL": {"name": "CARGO_INCREMENTAL", "export": false}},
                "recipes": {}, "modules": {}}}
        });
        assert_eq!(
            cargo_exports(&dump),
            [
                "CARGO_TARGET_DIR",
                "CARGO_PROFILE (parameter of \"b\")",
                "CARGO_INCREMENTAL"
            ]
        );
        let clean = serde_json::json!({"settings": {"export": false}, "assignments": {
            "CARGO_LOCAL": {"name": "CARGO_LOCAL", "export": false}}});
        assert!(cargo_exports(&clean).is_empty());
        for key in ["dotenv_load", "dotenv_required", "dotenv_override"] {
            let dotenv = serde_json::json!({"settings": {
                "dotenv_load": false, "dotenv_required": false, "dotenv_override": false,
                "dotenv_filename": null, "dotenv_path": null, key: true}});
            assert_eq!(cargo_exports(&dotenv).len(), 1, "{key}");
        }
        for (key, value) in [("dotenv_filename", ".env.local"), ("dotenv_path", "x/.env")] {
            let dotenv = serde_json::json!({"settings": {key: value}});
            assert_eq!(cargo_exports(&dotenv).len(), 1, "{key}");
        }
    }

    #[test]
    fn external_packages_skips_workspace_members() {
        let metadata = serde_json::json!({
            "workspace_members": ["path+file:///r/a#0.0.0"],
            "packages": [
                {"id": "path+file:///r/a#0.0.0", "name": "a", "version": "0.0.0"},
                {"id": "registry+x#serde@1.0.1", "name": "serde", "version": "1.0.1"},
                {"id": "registry+x#itoa@1.0.0", "name": "itoa", "version": "1.0.0"},
                {"id": "registry+x#winapi@0.3.9", "name": "winapi", "version": "0.3.9"}
            ],
            "resolve": {"nodes": [
                {"id": "path+file:///r/a#0.0.0"},
                {"id": "registry+x#serde@1.0.1"},
                {"id": "registry+x#itoa@1.0.0"}
            ]}
        });
        // `winapi` is a package of the lock file but not of the host resolve.
        assert_eq!(
            external_packages(&metadata).unwrap(),
            ["itoa@1.0.0", "serde@1.0.1"]
        );
        assert!(external_packages(&serde_json::json!({})).is_err());
    }
}
