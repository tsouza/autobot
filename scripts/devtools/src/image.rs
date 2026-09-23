//! `just operator-image`: the operator's qualification image, built from a host binary.
//!
//! [`build`] compiles the `autobot-operator` binary on the host with `cargo build --release
//! --locked --features` [`FEATURE`], so the home Cargo configuration's compiler wrapper
//! (sccache) applies and no compilation happens inside Docker. It copies the binary alone into
//! a fresh build context at [`CONTEXT`] and runs `docker build` with the Dockerfile at
//! [`DOCKERFILE`], tagging the result [`IMAGE`], then prints the image's digest, the
//! `sha256:` content address Docker reports as its `Id`. `just kind-load` loads [`IMAGE`]
//! into the kind cluster when given no image.
//!
//! Choices the design leaves open:
//!
//! - Only the qualification image exists: [`FEATURE`] is always on, because the M0-Q slice
//!   runs the fakes in-cluster. A release image is out of scope, so a build without the
//!   feature never reaches Docker.
//! - The base is `gcr.io/distroless/cc-debian13`, `nonroot` tag, pinned by the digest of its
//!   multi-platform index ([`check_dockerfile`] refuses a base that is not pinned). A Rust
//!   binary built for the GNU target links the host's glibc dynamically, and `cc` carries glibc
//!   and `libgcc_s`; Debian 13 ships glibc 2.41, at least the 2.39 of Ubuntu 24.04, the CI
//!   runner image, so a binary built there loads in the base.
//! - The build context holds only the binary, so no `.dockerignore` is needed and nothing else
//!   of the worktree reaches the daemon.
//! - The binary's path comes from cargo's JSON messages, so a `CARGO_TARGET_DIR` or a
//!   `build.target-dir` in the home configuration is honoured.

use crate::process::Cmd;
use crate::worktree::refusal;
use crate::{Error, Result};
use std::path::{Path, PathBuf};

/// The tag of the image [`build`] produces.
pub const IMAGE: &str = "autobot-operator:m0-q";

/// The Dockerfile, relative to the repository root.
pub const DOCKERFILE: &str = "deploy/image/Dockerfile";

/// The package and binary target the image runs.
pub const BINARY: &str = "autobot-operator";

/// The operator feature the image is built with: it links the fakes.
pub const FEATURE: &str = "m0-fakes";

/// The build context, relative to the repository root; [`build`] recreates it on every run.
pub const CONTEXT: &str = "target/operator-image";

/// Whether `image` ends in `@sha256:<64 lowercase hex digits>`.
#[must_use]
pub fn is_pinned(image: &str) -> bool {
    image.rsplit_once("@sha256:").is_some_and(|(_, d)| {
        d.len() == 64
            && d.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// Checks that a Dockerfile has a `FROM` instruction and that every one of them pins its image
/// by digest.
///
/// A `FROM` line is read as `FROM [--flag=value ...] <image> [AS <name>]`, its keyword in any
/// case; line continuations are not supported.
///
/// # Errors
/// Fails when there is no `FROM`, when one names no image, or when an image is not pinned.
pub fn check_dockerfile(text: &str) -> Result<()> {
    let invalid = |detail: String| refusal("operator Dockerfile", detail);
    let mut froms = 0usize;
    for line in text.lines() {
        let mut words = line.split_whitespace();
        if !words.next().is_some_and(|w| w.eq_ignore_ascii_case("FROM")) {
            continue;
        }
        froms += 1;
        let image = words
            .find(|w| !w.starts_with("--"))
            .ok_or_else(|| invalid(format!("`{}` names no image", line.trim())))?;
        if !is_pinned(image) {
            return Err(invalid(format!("base `{image}` is not pinned by digest")));
        }
    }
    if froms == 0 {
        return Err(invalid("no FROM instruction".into()));
    }
    Ok(())
}

/// The arguments of the host `cargo build` that produces the binary.
#[must_use]
pub fn cargo_args() -> Vec<String> {
    [
        "build",
        "--release",
        "--locked",
        "--package",
        BINARY,
        "--bin",
        BINARY,
        "--features",
        FEATURE,
        "--message-format",
        "json-render-diagnostics",
    ]
    .map(str::to_owned)
    .to_vec()
}

/// The executable of binary target `bin` in `cargo build --message-format json` output: the
/// `executable` of the last `compiler-artifact` message for a `bin` target of that name.
///
/// # Errors
/// Fails when a line is not JSON or when no such message has an executable.
pub fn executable(messages: &str, bin: &str) -> Result<PathBuf> {
    let mut found = None;
    for line in messages.lines().filter(|l| !l.trim().is_empty()) {
        let m: serde_json::Value =
            serde_json::from_str(line).map_err(|e| Error::Parse(format!("cargo message: {e}")))?;
        let is_bin = m["target"]["kind"]
            .as_array()
            .is_some_and(|k| k.iter().any(|k| k == "bin"));
        if m["reason"] == "compiler-artifact"
            && m["target"]["name"] == bin
            && is_bin
            && let Some(path) = m["executable"].as_str()
        {
            found = Some(PathBuf::from(path));
        }
    }
    found.ok_or_else(|| Error::Parse(format!("cargo reported no executable for `{bin}`")))
}

fn io(op: &str, path: &Path, e: &std::io::Error) -> Error {
    refusal(op, format!("{}: {e}", path.display()))
}

fn utf8(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| Error::Parse(format!("non-UTF-8 path {}", path.display())))
}

/// Builds the binary on the host, builds [`IMAGE`] from it and prints the image digest to
/// standard output as `<IMAGE> <digest>`.
///
/// # Errors
/// Fails when the Dockerfile cannot be read or pins no digest, when cargo or Docker fails, or
/// when the build context cannot be written.
pub fn build(repo: &Path) -> Result<()> {
    let dockerfile = repo.join(DOCKERFILE);
    let text = std::fs::read_to_string(&dockerfile)
        .map_err(|e| io("operator Dockerfile", &dockerfile, &e))?;
    check_dockerfile(&text)?;

    let cargo = Cmd::new("cargo").args(cargo_args()).current_dir(repo);
    eprintln!("+ {}", cargo.display());
    let exe = executable(&cargo.output()?, BINARY)?;

    let context = repo.join(CONTEXT);
    if context.exists() {
        std::fs::remove_dir_all(&context).map_err(|e| io("build context", &context, &e))?;
    }
    std::fs::create_dir_all(&context).map_err(|e| io("build context", &context, &e))?;
    let staged = context.join(BINARY);
    std::fs::copy(&exe, &staged).map_err(|e| io("build context", &staged, &e))?;

    Cmd::new("docker")
        .args([
            "build",
            "--file",
            utf8(&dockerfile)?,
            "--tag",
            IMAGE,
            utf8(&context)?,
        ])
        .run()?;
    let digest = Cmd::new("docker")
        .args(["image", "inspect", "--format", "{{.Id}}", IMAGE])
        .output()?;
    println!("{IMAGE} {}", digest.trim());
    Ok(())
}

/// Entry point of `scripts/operator_image.rs`, run from the repository root with no argument.
///
/// # Errors
/// Fails on a usage error or when [`build`] fails.
pub fn cli(args: &[String]) -> Result<()> {
    if args.is_empty() {
        build(Path::new("."))
    } else {
        Err(refusal(
            "operator-image",
            "usage: operator-image (no arguments)",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "54df941ed0d06a1bd95ef5e0ce391fd8d9f94b64782dc9a60062727849ee3f97";

    #[test]
    fn the_committed_dockerfile_pins_its_base() {
        check_dockerfile(include_str!("../../../deploy/image/Dockerfile")).unwrap();
    }

    #[test]
    fn pinned_froms_pass_with_flags_stages_and_any_case() {
        let text = format!(
            "# FROM comment is not an instruction\n\
             FROM --platform=linux/amd64 base:tag@sha256:{DIGEST} AS build\n\
             from other@sha256:{DIGEST}\n"
        );
        check_dockerfile(&text).unwrap();
    }

    #[test]
    fn an_unpinned_from_is_refused_even_after_a_pinned_one() {
        let text = format!("FROM a@sha256:{DIGEST}\nFROM gcr.io/distroless/cc-debian13:nonroot\n");
        let err = check_dockerfile(&text).unwrap_err().to_string();
        assert!(
            err.contains("`gcr.io/distroless/cc-debian13:nonroot` is not pinned"),
            "{err}"
        );
    }

    #[test]
    fn malformed_digests_are_not_pinned() {
        assert!(is_pinned(&format!("a:t@sha256:{DIGEST}")));
        for image in [
            format!("a@sha256:{}", &DIGEST[1..]),
            format!("a@sha256:{}", DIGEST.to_uppercase()),
            format!("a@sha256:{}g", &DIGEST[1..]),
            format!("a@sha512:{DIGEST}"),
            "a:latest".to_owned(),
        ] {
            assert!(!is_pinned(&image), "{image}");
        }
    }

    #[test]
    fn a_dockerfile_without_from_or_image_is_refused() {
        let none = check_dockerfile("COPY a b\n").unwrap_err().to_string();
        assert!(none.contains("no FROM instruction"), "{none}");
        let bare = check_dockerfile("FROM --platform=x\n")
            .unwrap_err()
            .to_string();
        assert!(bare.contains("names no image"), "{bare}");
    }

    #[test]
    fn cargo_builds_the_operator_binary_with_the_fakes_on_the_host() {
        let args = cargo_args().join(" ");
        assert!(args.starts_with("build --release --locked "), "{args}");
        assert!(
            args.contains("--package autobot-operator --bin autobot-operator"),
            "{args}"
        );
        assert!(args.contains("--features m0-fakes"), "{args}");
        assert!(
            args.ends_with("--message-format json-render-diagnostics"),
            "{args}"
        );
    }

    #[test]
    fn the_feature_is_the_operator_feature_that_links_the_fakes() {
        assert_eq!(FEATURE, crate::layering::M0_FAKES);
        assert_eq!(BINARY, crate::layering::OPERATOR);
        let manifest = include_str!("../../../crates/autobot-operator/Cargo.toml");
        assert!(
            manifest.contains(&format!("\n{FEATURE} = [\"dep:autobot-fakes\"]\n")),
            "{manifest}"
        );
    }

    fn artifact(name: &str, kind: &str, exe: Option<&str>) -> String {
        let exe = exe.map_or_else(|| "null".to_owned(), |e| format!("\"{e}\""));
        format!(
            r#"{{"reason":"compiler-artifact","target":{{"name":"{name}","kind":["{kind}"]}},"executable":{exe}}}"#
        )
    }

    #[test]
    fn the_executable_is_the_last_artifact_of_the_named_binary() {
        let messages = [
            artifact("autobot-operator", "lib", None),
            artifact(
                "autobot-controllers",
                "bin",
                Some("/t/release/autobot-controllers"),
            ),
            artifact("autobot-operator", "bin", Some("/t/old/autobot-operator")),
            artifact(
                "autobot-operator",
                "bin",
                Some("/t/release/autobot-operator"),
            ),
            r#"{"reason":"build-finished","success":true}"#.to_owned(),
            String::new(),
        ]
        .join("\n");
        assert_eq!(
            executable(&messages, BINARY).unwrap(),
            PathBuf::from("/t/release/autobot-operator")
        );
    }

    #[test]
    fn no_executable_or_non_json_output_is_an_error() {
        let lib_only = artifact("autobot-operator", "lib", None);
        let err = executable(&lib_only, BINARY).unwrap_err().to_string();
        assert!(
            err.contains("no executable for `autobot-operator`"),
            "{err}"
        );
        assert!(executable("   Compiling autobot-operator", BINARY).is_err());
    }

    #[test]
    fn kind_load_defaults_to_the_operator_image() {
        let justfile = include_str!("../../../Justfile");
        assert!(
            justfile.contains(&format!("\nkind-load image=\"{IMAGE}\":\n")),
            "{justfile}"
        );
    }

    #[test]
    fn arguments_are_refused_before_anything_runs() {
        let err = cli(&["x".to_owned()]).unwrap_err().to_string();
        assert!(err.contains("usage: operator-image"), "{err}");
    }
}
