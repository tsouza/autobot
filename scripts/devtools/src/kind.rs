//! `just kind-up`, `just kind-down`, `just kind-load <image>`: the kind cluster that
//! integration tests run against.
//!
//! The cluster is named [`CLUSTER`] and created from the kind configuration at [`CONFIG`],
//! relative to the repository root. Before creating it, [`up`] requires the `kind` on `PATH`
//! to be release [`KIND_VERSION`], because a kind node image is built for one kind release,
//! and requires every node of the configuration to pin its image by digest
//! ([`check_config`]). [`up`] leaves an existing cluster of that name running, and
//! `kind delete cluster` succeeds when there is none, so both are idempotent.
//!
//! `kind create cluster` makes the cluster's context, `kind-autobot`, the current context of
//! the kubeconfig `KUBECONFIG` names; when the cluster already exists, [`up`] writes that
//! context there with `kind export kubeconfig` instead, so the file is present after every
//! successful [`up`], including after `cargo clean` or when another worktree created the
//! cluster. `kind delete cluster` removes the context from there. The
//! Justfile recipes set `KUBECONFIG` to the cluster's own file under `target/kind/` for these
//! and for `just test-integration`, which is where integration tests find the cluster.

use crate::process::Cmd;
use crate::worktree::refusal;
use crate::{Error, Result};
use std::path::Path;

/// Name of the cluster.
pub const CLUSTER: &str = "autobot";

/// The kind configuration, relative to the repository root.
pub const CONFIG: &str = "deploy/kind/cluster.yaml";

/// The kind release the node image in [`CONFIG`] is built for.
pub const KIND_VERSION: &str = "0.33.0";

/// How long `kind create cluster` waits for the control plane to become ready.
pub const WAIT: &str = "300s";

/// Checks that `kind version` output names release [`KIND_VERSION`].
///
/// # Errors
/// Fails when the output names another release or has no version.
pub fn check_version(output: &str) -> Result<()> {
    let found = output.split_whitespace().nth(1).unwrap_or_default();
    if found.strip_prefix('v') == Some(KIND_VERSION) {
        Ok(())
    } else {
        Err(refusal(
            "kind version",
            format!(
                "found `{}`, need kind v{KIND_VERSION}: the node image in {CONFIG} is built \
                 for that release, and the two are updated together",
                output.trim()
            ),
        ))
    }
}

/// Checks that every node of a kind configuration pins its image by digest.
///
/// The configuration is read line by line in the block style of [`CONFIG`]: each node starts
/// with a `role:` key and must have an `image:` key whose value ends in
/// `@sha256:<64 lowercase hex digits>`.
///
/// # Errors
/// Fails when there is no node, when a node has no image, or when an image is not pinned by
/// digest.
pub fn check_config(text: &str) -> Result<()> {
    let mut roles = 0usize;
    let mut images = Vec::new();
    for line in text.lines() {
        let line = line.trim_start().trim_start_matches("- ");
        if line.starts_with("role:") {
            roles += 1;
        } else if let Some(value) = line.strip_prefix("image:") {
            images.push(value.trim().trim_matches(['"', '\'']));
        }
    }
    let invalid = |detail: String| refusal("kind config", detail);
    if roles == 0 {
        return Err(invalid("no node".into()));
    }
    if images.len() != roles {
        return Err(invalid(format!(
            "{roles} node(s) but {} image(s): every node pins its image",
            images.len()
        )));
    }
    for image in images {
        if !crate::image::is_pinned(image) {
            return Err(invalid(format!("image `{image}` is not pinned by digest")));
        }
    }
    Ok(())
}

/// Whether `kind get clusters` output lists `name`.
#[must_use]
pub fn has_cluster(output: &str, name: &str) -> bool {
    output.lines().any(|line| line.trim() == name)
}

/// Creates the cluster from the configuration under `repo`; when it already exists, writes its
/// context to the kubeconfig `KUBECONFIG` names instead.
///
/// # Errors
/// Fails when kind is missing or another release, when the configuration cannot be read or
/// pins no digest, or when kind fails.
pub fn up(repo: &Path) -> Result<()> {
    check_version(&Cmd::new("kind").args(["version"]).output()?)?;
    let config = repo.join(CONFIG);
    let text = std::fs::read_to_string(&config)
        .map_err(|e| refusal("kind config", format!("{}: {e}", config.display())))?;
    check_config(&text)?;
    if has_cluster(
        &Cmd::new("kind").args(["get", "clusters"]).output()?,
        CLUSTER,
    ) {
        eprintln!("kind cluster `{CLUSTER}` is already up; exporting its kubeconfig");
        return Cmd::new("kind")
            .args(["export", "kubeconfig", "--name", CLUSTER])
            .run();
    }
    let config = config
        .to_str()
        .ok_or_else(|| Error::Parse(format!("non-UTF-8 path {}", config.display())))?;
    Cmd::new("kind")
        .args(["create", "cluster", "--name", CLUSTER, "--config", config])
        .args(["--wait", WAIT])
        .run()
}

/// Deletes the cluster; succeeds when there is none.
///
/// # Errors
/// Fails when kind is missing or fails.
pub fn down() -> Result<()> {
    Cmd::new("kind")
        .args(["delete", "cluster", "--name", CLUSTER])
        .run()
}

/// Loads a local container image into the cluster's nodes.
///
/// # Errors
/// Fails when kind is missing or fails, for instance when the image or the cluster does not
/// exist.
pub fn load(image: &str) -> Result<()> {
    Cmd::new("kind")
        .args(["load", "docker-image", image, "--name", CLUSTER])
        .run()
}

/// Entry point of `scripts/kind.rs`: `up`, `down` or `load <image>`, run from the repository
/// root.
///
/// # Errors
/// Fails on a usage error or when the subcommand fails.
pub fn cli(args: &[String]) -> Result<()> {
    match args {
        [cmd] if cmd == "up" => up(Path::new(".")),
        [cmd] if cmd == "down" => down(),
        [cmd, image] if cmd == "load" => load(image),
        _ => Err(refusal(
            "kind",
            "usage: kind up | kind down | kind load <image>",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "a1ed56cfb0e7b93589bdf97c8cd566405a265939e3620fc4f5de89adff580ae5";

    fn config(nodes: &[&str]) -> String {
        let mut text = String::from("kind: Cluster\napiVersion: kind.x-k8s.io/v1alpha4\nnodes:\n");
        for node in nodes {
            text.push_str(node);
        }
        text
    }

    #[test]
    fn the_committed_config_pins_every_node_image() {
        let text = include_str!("../../../deploy/kind/cluster.yaml");
        check_config(text).unwrap();
    }

    #[test]
    fn a_digest_pinned_node_passes_quoted_or_not() {
        let plain =
            format!("  - role: control-plane\n    image: kindest/node:v1.37.0@sha256:{DIGEST}\n");
        let quoted = format!("  - role: worker\n    image: \"kindest/node@sha256:{DIGEST}\"\n");
        check_config(&config(&[&plain, &quoted])).unwrap();
    }

    #[test]
    fn a_tag_only_image_is_refused() {
        let node = "  - role: control-plane\n    image: kindest/node:v1.37.0\n";
        let err = check_config(&config(&[node])).unwrap_err().to_string();
        assert!(
            err.contains("`kindest/node:v1.37.0` is not pinned"),
            "{err}"
        );
    }

    #[test]
    fn a_malformed_digest_is_refused() {
        for digest in [
            &DIGEST[1..],
            &DIGEST.to_uppercase(),
            &format!("{}g", &DIGEST[1..]),
        ] {
            let node =
                format!("  - role: control-plane\n    image: kindest/node@sha256:{digest}\n");
            assert!(check_config(&config(&[&node])).is_err(), "{digest}");
        }
    }

    #[test]
    fn a_node_without_an_image_is_refused() {
        let pinned = format!("  - role: control-plane\n    image: kindest/node@sha256:{DIGEST}\n");
        let bare = "  - role: worker\n";
        let err = check_config(&config(&[&pinned, bare]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("2 node(s) but 1 image(s)"), "{err}");
    }

    #[test]
    fn a_config_without_nodes_is_refused() {
        let err = check_config(&config(&[])).unwrap_err().to_string();
        assert!(err.contains("no node"), "{err}");
    }

    #[test]
    fn only_the_pinned_kind_release_is_accepted() {
        check_version(&format!("kind v{KIND_VERSION} go1.26.7 linux/amd64\n")).unwrap();
        for other in ["kind v0.32.0 go1.25.1 linux/amd64", "kind", ""] {
            let err = check_version(other).unwrap_err().to_string();
            assert!(err.contains(&format!("need kind v{KIND_VERSION}")), "{err}");
        }
        let longer = format!("kind v{KIND_VERSION}1 go1.26.7 linux/amd64");
        assert!(check_version(&longer).is_err());
    }

    #[test]
    fn a_cluster_is_found_only_by_its_whole_name() {
        let output = "autobot-old\nother\n";
        assert!(!has_cluster(output, CLUSTER));
        assert!(has_cluster("other\nautobot\n", CLUSTER));
        assert!(!has_cluster("", CLUSTER));
    }

    #[test]
    fn usage_errors_run_nothing() {
        for args in [
            &[][..],
            &["load".to_owned()],
            &["up".to_owned(), "x".to_owned()],
        ] {
            let err = cli(args).unwrap_err().to_string();
            assert!(err.contains("usage: kind up"), "{err}");
        }
    }

    #[test]
    fn the_integration_workflow_also_runs_nightly() {
        const WORKFLOW: &str = include_str!("../../../.github/workflows/kind.yml");
        let on = WORKFLOW
            .split("\non:\n")
            .nth(1)
            .and_then(|rest| rest.split("\n\n").next())
            .unwrap_or_default();
        assert!(on.contains("  pull_request:"), "{on}");
        assert!(on.contains("  schedule:\n    - cron: "), "{on}");
    }
}
