//! The formal tools, pinned by version and SHA-256 digest.
//!
//! Every tool is downloaded with `curl` from its pinned release URL, its digest is checked with
//! `sha256sum` before anything is installed, and a mismatching download is deleted and fails.
//! Installed tools live under a tools root (`target/formal` for the repository):
//!
//! - `bin/quint`: the standalone Quint executable, which bundles its Node runtime;
//! - `quint-home/rust-evaluator-<version>/`: the Rust evaluator behind `quint run`;
//! - `quint-home/apalache-dist-<version>/`: the Apalache distribution behind `quint verify`.
//!
//! `quint-home` is passed to Quint as `QUINT_HOME`, where Quint looks for the evaluator and
//! Apalache before it would download them itself. The evaluator version is the one the pinned
//! Quint release expects (`QUINT_EVALUATOR_VERSION` in its bundle); the Apalache version is
//! passed to `quint verify` as `--apalache-version`. Apalache runs on the `java` found on
//! `PATH`, which is not pinned here.
//!
//! An installed tool carries a stamp file `<install path>.sha256` holding the digest it was
//! installed from; a tool whose stamp does not name its pinned digest is reinstalled. The
//! assets are Linux x86-64 builds.

use crate::process::Cmd;
use crate::{Error, Result};
use std::path::{Path, PathBuf};

/// The directory under the tools root that Quint receives as `QUINT_HOME`.
pub const QUINT_HOME: &str = "quint-home";

/// How a downloaded asset becomes an installed tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unpack {
    /// The asset is the executable itself, installed as `bin/<name>`.
    Executable,
    /// The asset is a gzip tarball, extracted into `quint-home/<prefix><version>/`.
    QuintHome(&'static str),
}

/// A tool pinned by version and SHA-256 digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tool {
    /// Tool name.
    pub name: &'static str,
    /// Release version, as the release tags it.
    pub version: &'static str,
    /// Download URL of the release asset.
    pub url: &'static str,
    /// Lowercase hexadecimal SHA-256 of the asset.
    pub sha256: &'static str,
    /// How the asset is installed.
    pub unpack: Unpack,
}

/// Quint, the authoring surface: typechecking, simulation and the front end of verification.
pub const QUINT: Tool = Tool {
    name: "quint",
    version: "0.32.0",
    url: "https://github.com/informalsystems/quint/releases/download/v0.32.0/quint-linux-amd64",
    sha256: "939b64095b706017f2f202c6f99c860c40be7c31bddc2b98557316e50f42cd7f",
    unpack: Unpack::Executable,
};

/// The Rust evaluator that `quint run` simulates with.
pub const QUINT_EVALUATOR: Tool = Tool {
    name: "quint_evaluator",
    version: "v0.6.0",
    url: "https://github.com/informalsystems/quint/releases/download/evaluator/v0.6.0/\
          quint_evaluator-x86_64-unknown-linux-gnu.tar.gz",
    sha256: "61755a09d5052d93a4e75e840059edfd0d3674aeda164b9d2464be3d6e21b1c2",
    unpack: Unpack::QuintHome("rust-evaluator-"),
};

/// Apalache, the symbolic bounded model checker behind `quint verify`.
pub const APALACHE: Tool = Tool {
    name: "apalache",
    version: "0.56.1",
    url: "https://github.com/apalache-mc/apalache/releases/download/v0.56.1/apalache.tgz",
    sha256: "91125e5a3646b9c9d3a7d921d3323f321fac5071909f72b3960c66ff2f998ee1",
    unpack: Unpack::QuintHome("apalache-dist-"),
};

/// Every pinned tool.
pub const ALL: [Tool; 3] = [QUINT, QUINT_EVALUATOR, APALACHE];

impl Tool {
    /// Where the tool is installed under the tools root `root`.
    #[must_use]
    pub fn install_path(&self, root: &Path) -> PathBuf {
        match self.unpack {
            Unpack::Executable => root.join("bin").join(self.name),
            Unpack::QuintHome(prefix) => root
                .join(QUINT_HOME)
                .join(format!("{prefix}{}", self.version)),
        }
    }

    fn stamp_path(&self, root: &Path) -> PathBuf {
        let mut path = self.install_path(root).into_os_string();
        path.push(".sha256");
        PathBuf::from(path)
    }

    /// Whether the tool is installed under `root` from its pinned digest.
    #[must_use]
    pub fn is_installed(&self, root: &Path) -> bool {
        self.install_path(root).exists()
            && std::fs::read_to_string(self.stamp_path(root)).is_ok_and(|s| s.trim() == self.sha256)
    }

    /// Installs the tool under `root` unless it is already installed from its pinned digest.
    ///
    /// # Errors
    /// Fails if the download fails, its digest is not the pinned one (the download is then
    /// deleted and nothing is installed), or unpacking fails.
    pub fn ensure(&self, root: &Path) -> Result<()> {
        if self.is_installed(root) {
            return Ok(());
        }
        let downloads = root.join("downloads");
        create_dir(&downloads)?;
        let asset = downloads.join(format!("{}-{}.part", self.name, self.version));
        let stamp = self.stamp_path(root);
        remove(&stamp)?;
        Cmd::new("curl")
            .args([
                "--fail",
                "--silent",
                "--show-error",
                "--location",
                "--retry",
                "3",
            ])
            .args(["--output", &path_str(&asset)?, self.url])
            .run()?;
        if let Err(e) = check(&asset, self.sha256) {
            remove(&asset)?;
            return Err(e);
        }
        let dest = self.install_path(root);
        remove(&dest)?;
        match self.unpack {
            Unpack::Executable => {
                create_dir(dest.parent().unwrap_or(root))?;
                std::fs::rename(&asset, &dest).map_err(|e| io_error("installing", &dest, &e))?;
                make_executable(&dest)?;
            }
            Unpack::QuintHome(_) => {
                create_dir(&dest)?;
                Cmd::new("tar")
                    .args(["-xzf", &path_str(&asset)?, "-C", &path_str(&dest)?])
                    .run()?;
                remove(&asset)?;
            }
        }
        std::fs::write(&stamp, format!("{}\n", self.sha256))
            .map_err(|e| io_error("writing", &stamp, &e))
    }

    /// The tool's line in the digest manifest: name, version, digest and URL.
    #[must_use]
    pub fn manifest_line(&self) -> String {
        format!(
            "tool {} {} sha256:{} {}",
            self.name, self.version, self.sha256, self.url
        )
    }
}

/// The lowercase hexadecimal SHA-256 of the file at `path`, from `sha256sum`.
///
/// # Errors
/// Fails if `sha256sum` fails or prints no digest.
pub fn sha256(path: &Path) -> Result<String> {
    let out = Cmd::new("sha256sum")
        .args(["--binary", &path_str(path)?])
        .output()?;
    out.split_whitespace()
        .next()
        .filter(|d| d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| Error::Parse(format!("sha256sum printed no digest: {out}")))
}

/// Checks that the file at `path` has the SHA-256 digest `expected`.
///
/// # Errors
/// Fails if the digest cannot be computed or differs from `expected`.
pub fn check(path: &Path, expected: &str) -> Result<()> {
    let actual = sha256(path)?;
    if actual == expected {
        Ok(())
    } else {
        Err(Error::Command {
            command: format!("sha256sum {}", path.display()),
            detail: format!("digest {actual} does not match the pinned digest {expected}"),
        })
    }
}

fn path_str(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::Parse(format!("path is not UTF-8: {}", path.display())))
}

pub(super) fn io_error(what: &str, path: &Path, e: &std::io::Error) -> Error {
    Error::Parse(format!("{what} {}: {e}", path.display()))
}

pub(super) fn create_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).map_err(|e| io_error("creating", dir, &e))
}

/// Removes the file or directory at `path`, if there is one.
fn remove(path: &Path) -> Result<()> {
    let result = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else if path.exists() {
        std::fs::remove_file(path)
    } else {
        return Ok(());
    };
    result.map_err(|e| io_error("removing", path, &e))
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| io_error("making executable", path, &e))
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree::test_support::TempDir;

    /// SHA-256 of the three bytes `abc` (FIPS 180-2, appendix B.1).
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    /// A tool whose asset is the local file `asset`, fetched through a `file://` URL.
    fn local(asset: &Path, sha256: &'static str, unpack: Unpack) -> Tool {
        let url: &'static str = Box::leak(format!("file://{}", asset.display()).into_boxed_str());
        Tool {
            name: "fake",
            version: "1.0.0",
            url,
            sha256,
            unpack,
        }
    }

    #[test]
    fn sha256_is_the_file_digest() {
        let tmp = TempDir::new();
        let file = tmp.0.join("abc");
        std::fs::write(&file, "abc").unwrap();
        assert_eq!(sha256(&file).unwrap(), ABC);
        check(&file, ABC).unwrap();
        std::fs::write(&file, "abd").unwrap();
        let err = check(&file, ABC).unwrap_err().to_string();
        assert!(err.contains(&format!("the pinned digest {ABC}")), "{err}");
    }

    #[test]
    fn an_executable_installs_once_from_its_pinned_digest() {
        let tmp = TempDir::new();
        let asset = tmp.0.join("asset");
        std::fs::write(&asset, "abc").unwrap();
        let root = tmp.0.join("tools");
        let tool = local(&asset, ABC, Unpack::Executable);
        assert!(!tool.is_installed(&root));
        tool.ensure(&root).unwrap();
        let bin = root.join("bin/fake");
        assert_eq!(std::fs::read_to_string(&bin).unwrap(), "abc");
        assert!(tool.is_installed(&root));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&bin).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755);
        }
        // Installed: the asset is not fetched again, so its removal does not matter.
        std::fs::remove_file(&asset).unwrap();
        tool.ensure(&root).unwrap();
        assert!(tool.is_installed(&root));
    }

    #[test]
    fn a_tampered_download_fails_and_installs_nothing() {
        let tmp = TempDir::new();
        let asset = tmp.0.join("asset");
        std::fs::write(&asset, "abc, tampered").unwrap();
        let root = tmp.0.join("tools");
        let tool = local(&asset, ABC, Unpack::Executable);
        let err = tool.ensure(&root).unwrap_err().to_string();
        assert!(err.contains("does not match the pinned digest"), "{err}");
        assert!(!root.join("bin/fake").exists());
        assert!(!tool.is_installed(&root));
        let leftovers = std::fs::read_dir(root.join("downloads")).unwrap().count();
        assert_eq!(leftovers, 0, "the tampered download is deleted");
    }

    #[test]
    fn a_changed_pin_reinstalls() {
        let tmp = TempDir::new();
        let asset = tmp.0.join("asset");
        std::fs::write(&asset, "abc").unwrap();
        let root = tmp.0.join("tools");
        local(&asset, ABC, Unpack::Executable)
            .ensure(&root)
            .unwrap();
        // The same install path pinned to another digest is not installed, and the asset at
        // the URL does not have that digest, so the reinstall fails.
        let other = "0".repeat(64);
        let repinned = local(
            &asset,
            Box::leak(other.into_boxed_str()),
            Unpack::Executable,
        );
        assert!(!repinned.is_installed(&root));
        assert!(repinned.ensure(&root).is_err());
        assert!(!repinned.is_installed(&root));
    }

    #[test]
    fn a_tarball_extracts_into_quint_home() {
        let tmp = TempDir::new();
        let src = tmp.0.join("src");
        std::fs::create_dir_all(src.join("apalache/bin")).unwrap();
        std::fs::write(src.join("apalache/bin/apalache-mc"), "#!/bin/sh\n").unwrap();
        let asset = tmp.0.join("asset.tgz");
        Cmd::new("tar")
            .args([
                "-czf",
                asset.to_str().unwrap(),
                "-C",
                src.to_str().unwrap(),
                "apalache",
            ])
            .output()
            .unwrap();
        let digest: &'static str = Box::leak(sha256(&asset).unwrap().into_boxed_str());
        let root = tmp.0.join("tools");
        let tool = local(&asset, digest, Unpack::QuintHome("apalache-dist-"));
        tool.ensure(&root).unwrap();
        assert_eq!(
            tool.install_path(&root),
            root.join("quint-home/apalache-dist-1.0.0")
        );
        assert!(
            root.join("quint-home/apalache-dist-1.0.0/apalache/bin/apalache-mc")
                .is_file()
        );
        assert!(tool.is_installed(&root));
        assert_eq!(
            std::fs::read_dir(root.join("downloads")).unwrap().count(),
            0
        );
    }

    #[test]
    fn pins_are_well_formed() {
        for tool in ALL {
            assert_eq!(tool.sha256.len(), 64, "{}", tool.name);
            assert!(
                tool.sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "{}",
                tool.name
            );
            assert!(tool.url.starts_with("https://github.com/"), "{}", tool.name);
            assert!(tool.url.contains(tool.version), "{}", tool.name);
        }
        // Quint looks for these directories under QUINT_HOME before downloading anything.
        let root = Path::new("/r");
        assert_eq!(
            QUINT_EVALUATOR.install_path(root),
            Path::new("/r/quint-home/rust-evaluator-v0.6.0")
        );
        assert_eq!(
            APALACHE.install_path(root),
            Path::new("/r/quint-home/apalache-dist-0.56.1")
        );
        assert_eq!(QUINT.install_path(root), Path::new("/r/bin/quint"));
    }
}
