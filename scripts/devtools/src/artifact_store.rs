//! `just artifact-store-up`, `just artifact-store-down`, `just artifact-store-check`: the M0
//! artifact-store fixture, one MinIO server on the kind cluster of [`crate::kind`].
//!
//! The manifests are the [`MANIFESTS`] under [`DIR`], relative to the repository root. Every
//! `image:` in them is pinned by digest ([`check_images`]); [`up`] and [`check`] refuse
//! otherwise. Every call reaches the cluster through `kubectl --context kind-<cluster>`, with
//! the cluster named by [`crate::kind::CLUSTER`] and the kubeconfig that `KUBECONFIG` names,
//! which the Justfile recipes set to the kind cluster's own file.
//!
//! [`up`] applies `namespace.yaml`, creates the Secret [`SECRET`] when it is absent, applies
//! `store.yaml`, waits for the rollout, requires every store pod to run on the one node labelled
//! [`NODE_KEY`]`=`[`NODE_ROLE`] (the kind worker that `deploy/kind/cluster.yaml` dedicates to
//! the store and taints with the same key and value, `NoSchedule`, so that only pods
//! tolerating it run there), and then, through the `mc` client bundled in the
//! server image, creates the bucket [`BUCKET`], enables versioning on it and sets SSE-S3 as its
//! default encryption. It asserts the result rather than trusting the commands: versioning
//! reads back `Enabled`, the default encryption reads back `AES256`, and an object written
//! afterwards is stored with `X-Amz-Server-Side-Encryption: AES256` and a version id. Running
//! [`up`] again is harmless.
//!
//! Choices this fixture makes that the design leaves open:
//!
//! - The Secret is generated once, from the operating system's random source, and kept while
//!   the namespace lives: the root password is 16 random bytes in hex, and the SSE-S3 master
//!   key is MinIO's single static KMS key ([`KMS_KEY`]), 32 random bytes in base64. A new key
//!   over the old volume would leave every object unreadable, so [`up`] never replaces it. The
//!   values reach `kubectl` through a mode-0600 file under [`SCRATCH`] that is removed right
//!   after, never through the command line, which [`crate::process::Cmd`] logs.
//! - The store is a single server on one volume of the cluster's default storage class, which
//!   provisions it on the node the pod is scheduled to, the store's node. It is
//!   a qualification fixture: it has no replication and no restore process of its own.
//!
//! [`check`] stands in for an agent workspace. It creates the namespace [`PROBE_NAMESPACE`],
//! gives it a client Secret for the store's Service, and runs two pods there: `writer` uploads an
//! object named after its own UID and must have run on another node than the store's;
//! `intruder` asks for the store's node without the toleration and must stay `Unschedulable`
//! by the untolerated taint. It then deletes that namespace, waits until it is gone, and
//! requires the store's namespace to be `Active`, its deployment to be available, and the
//! object to read back as the same version, with the workspace's content, encrypted.
//!
//! [`down`] deletes the store's namespace, and with it the volume and every object.

use crate::image::is_pinned;
use crate::kind::CLUSTER;
use crate::process::Cmd;
use crate::worktree::refusal;
use crate::{Error, Result};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Directory of the manifests, relative to the repository root.
pub const DIR: &str = "deploy/artifact-store";

/// The manifests under [`DIR`], in the order they are applied.
pub const MANIFESTS: [&str; 4] = [
    "namespace.yaml",
    "store.yaml",
    "workspace-probe-namespace.yaml",
    "workspace-probe.yaml",
];

/// The store's namespace.
pub const NAMESPACE: &str = "autobot-artifact-store";

/// The namespace [`check`] creates and deletes in place of an agent workspace.
pub const PROBE_NAMESPACE: &str = "autobot-workspace-probe";

/// The bucket [`up`] creates.
pub const BUCKET: &str = "artifacts";

/// The Secret holding the server's root credentials and its KMS key.
pub const SECRET: &str = "minio-credentials";

/// The Secret, in [`PROBE_NAMESPACE`], holding the probe's client alias.
pub const PROBE_SECRET: &str = "store-client";

/// Name of MinIO's static KMS key, the master key of SSE-S3.
pub const KMS_KEY: &str = "autobot-fixture";

/// The root user of the server.
pub const ROOT_USER: &str = "autobot";

/// Directory, relative to the repository root, of the short-lived Secret input files.
pub const SCRATCH: &str = "target/artifact-store";

/// How long a rollout, the probe pod or the probe namespace deletion may take.
pub const WAIT: &str = "300s";

/// The label key, and the taint key, of the kind worker dedicated to the store.
pub const NODE_KEY: &str = "autobot/role";

/// The value of [`NODE_KEY`] in that label and taint.
pub const NODE_ROLE: &str = "artifact-store";

/// The store's Service address, as a client in another namespace reaches it.
pub const SERVICE_URL: &str = "minio.autobot-artifact-store.svc.cluster.local:9000";

/// Checks that manifest text names at least one image and pins every image by digest.
///
/// Images are the values of `image:` keys, read line by line, quoted or not.
///
/// # Errors
/// Fails when there is no image or an image is not pinned by digest.
pub fn check_images(text: &str) -> Result<()> {
    let images: Vec<&str> = text
        .lines()
        .map(|line| line.trim_start().trim_start_matches("- "))
        .filter_map(|line| line.strip_prefix("image:"))
        .map(|value| value.trim().trim_matches(['"', '\'']))
        .collect();
    let invalid = |detail: String| refusal("artifact-store manifests", detail);
    if images.is_empty() {
        return Err(invalid("no image".into()));
    }
    match images.iter().find(|image| !is_pinned(image)) {
        Some(image) => Err(invalid(format!("image `{image}` is not pinned by digest"))),
        None => Ok(()),
    }
}

fn json(text: &str) -> Result<serde_json::Value> {
    serde_json::from_str(text.trim()).map_err(|e| Error::Parse(format!("mc output: {e}")))
}

fn assertion(detail: String) -> Error {
    refusal("artifact-store assertion", detail)
}

/// Checks `mc version info --json` output: versioning is `Enabled`.
///
/// # Errors
/// Fails on output that is not JSON or names another status.
pub fn check_versioning(output: &str) -> Result<()> {
    let value = json(output)?;
    match value.pointer("/versioning/status").and_then(|s| s.as_str()) {
        Some("Enabled") => Ok(()),
        other => Err(assertion(format!(
            "bucket versioning is {other:?}, need \"Enabled\""
        ))),
    }
}

/// Checks `mc encrypt info --json` output: the default encryption is SSE-S3 (`AES256`).
///
/// # Errors
/// Fails on output that is not JSON or names another algorithm.
pub fn check_encryption(output: &str) -> Result<()> {
    let value = json(output)?;
    match value
        .pointer("/encryption/algorithm")
        .and_then(|s| s.as_str())
    {
        Some("AES256") => Ok(()),
        other => Err(assertion(format!(
            "bucket default encryption is {other:?}, need \"AES256\" (SSE-S3)"
        ))),
    }
}

/// Checks `mc stat --json` output of one object: it is stored with SSE-S3 and has a version
/// id. Returns the version id.
///
/// # Errors
/// Fails on output that is not JSON, an object without SSE-S3, or an object without a version
/// id (none, empty, or `null`, the id of an object written while versioning was off).
pub fn check_object(output: &str) -> Result<String> {
    let value = json(output)?;
    let sse = value
        .pointer("/metadata/X-Amz-Server-Side-Encryption")
        .and_then(|s| s.as_str());
    if sse != Some("AES256") {
        return Err(assertion(format!(
            "object server-side encryption is {sse:?}, need \"AES256\""
        )));
    }
    match value.get("versionID").and_then(|s| s.as_str()) {
        Some(id) if !id.is_empty() && id != "null" => Ok(id.to_owned()),
        other => Err(assertion(format!("object version id is {other:?}"))),
    }
}

/// The one node `kubectl get nodes -l <NODE_KEY>=<NODE_ROLE> -o name` lists, without its
/// `node/` prefix.
///
/// # Errors
/// Fails when there is no such node or more than one.
pub fn store_node(output: &str) -> Result<String> {
    let nodes: Vec<&str> = output.split_whitespace().collect();
    match nodes[..] {
        [node] => Ok(node.trim_start_matches("node/").to_owned()),
        _ => Err(assertion(format!(
            "need exactly one node labelled {NODE_KEY}={NODE_ROLE}, found {nodes:?}"
        ))),
    }
}

/// Checks the space-separated node names of the store's pods: at least one, all `node`.
///
/// # Errors
/// Fails when there is no pod or a pod runs, or is bound, elsewhere.
pub fn check_placement(node: &str, pod_nodes: &str) -> Result<()> {
    let pods: Vec<&str> = pod_nodes.split_whitespace().collect();
    if pods.is_empty() || pods.iter().any(|p| *p != node) {
        return Err(assertion(format!(
            "store pods run on {pods:?}, need all on `{node}`"
        )));
    }
    Ok(())
}

/// Checks the `PodScheduled` condition of a pod that asks for the store's node without
/// tolerating its taint, as `<reason> <message>`: it is `Unschedulable` because of an
/// untolerated taint.
///
/// # Errors
/// Fails for any other reason or message.
pub fn check_repelled(condition: &str) -> Result<()> {
    let condition = condition.trim();
    if condition.starts_with("Unschedulable ") && condition.contains("had untolerated taint") {
        Ok(())
    } else {
        Err(assertion(format!(
            "a pod without the toleration is `{condition}`, need Unschedulable by an \
             untolerated taint"
        )))
    }
}

/// Standard base64 (RFC 4648 §4) with padding.
#[must_use]
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Lowercase hexadecimal.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The `KEY=value` lines of the Secret [`SECRET`], from 48 random bytes: the first 16 make the
/// root password, the other 32 the KMS key.
#[must_use]
pub fn credentials_env(random: &[u8; 48]) -> String {
    let (password, key) = random.split_at(16);
    format!(
        "MINIO_ROOT_USER={ROOT_USER}\nMINIO_ROOT_PASSWORD={}\nMINIO_KMS_SECRET_KEY={KMS_KEY}:{}\n",
        hex(password),
        base64(key)
    )
}

/// The `KEY=value` line of the Secret [`PROBE_SECRET`]: the `store` alias of `mc`, reaching the
/// store's Service with the given credentials.
#[must_use]
pub fn client_env(user: &str, password: &str) -> String {
    format!("MC_HOST_store=http://{user}:{password}@{SERVICE_URL}\n")
}

fn kubectl() -> Cmd {
    Cmd::new("kubectl").args(["--context".to_owned(), format!("kind-{CLUSTER}")])
}

/// `kubectl exec` of `args` in the store's server container.
fn exec<I, S>(args: I) -> Cmd
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    kubectl()
        .args(["-n", NAMESPACE, "exec", "deploy/minio", "-c", "minio", "--"])
        .args(args)
}

/// Runs `cmd`, logging it and its standard output to standard error, so that every value an
/// assertion reads is in the log. Nothing run this way carries a credential.
fn logged(cmd: &Cmd) -> Result<String> {
    eprintln!("+ {}", cmd.display());
    let out = cmd.output()?;
    eprintln!("{}", out.trim_end());
    Ok(out)
}

/// `mc <args> --json` in the store's server container, against its `local` alias.
fn mc(args: &[&str]) -> Result<String> {
    logged(&exec(["mc"]).args(args.iter().copied()).args(["--json"]))
}

fn bucket() -> String {
    format!("local/{BUCKET}")
}

fn object(key: &str) -> String {
    format!("{}/{key}", bucket())
}

fn read_manifests(repo: &Path) -> Result<()> {
    let mut all = String::new();
    for name in MANIFESTS {
        let path = repo.join(DIR).join(name);
        let text = std::fs::read_to_string(&path).map_err(|e| {
            refusal(
                "artifact-store manifests",
                format!("{}: {e}", path.display()),
            )
        })?;
        all.push_str(&text);
        all.push('\n');
    }
    check_images(&all)
}

fn apply(repo: &Path, name: &str) -> Result<()> {
    let path = repo.join(DIR).join(name);
    let path = path
        .to_str()
        .ok_or_else(|| Error::Parse(format!("non-UTF-8 path {}", path.display())))?;
    kubectl().args(["apply", "-f", path]).run()
}

fn random48() -> Result<[u8; 48]> {
    let mut bytes = [0u8; 48];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| refusal("/dev/urandom", e.to_string()))?;
    Ok(bytes)
}

/// Creates Secret `name` in `namespace` from `KEY=value` lines, through a mode-0600 file under
/// [`SCRATCH`] that is removed whether or not `kubectl` succeeds.
fn create_secret(repo: &Path, namespace: &str, name: &str, env: &str) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let dir = repo.join(SCRATCH);
    let io = |e: std::io::Error| refusal("secret input", format!("{}: {e}", dir.display()));
    std::fs::create_dir_all(&dir).map_err(io)?;
    let path: PathBuf = dir.join(format!("{namespace}.{name}.env"));
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .and_then(|mut f| f.write_all(env.as_bytes()))
        .map_err(io)?;
    let result = path
        .to_str()
        .ok_or_else(|| Error::Parse(format!("non-UTF-8 path {}", path.display())))
        .and_then(|p| {
            kubectl()
                .args(["-n", namespace, "create", "secret", "generic", name])
                .args(["--from-env-file", p])
                .run()
        });
    let removed = std::fs::remove_file(&path).map_err(io);
    result.and(removed)
}

fn secret_value(key: &str) -> Result<String> {
    kubectl()
        .args(["-n", NAMESPACE, "get", "secret", SECRET, "-o"])
        .args([format!("go-template={{{{.data.{key} | base64decode}}}}")])
        .output()
}

fn rollout() -> Result<()> {
    kubectl()
        .args(["-n", NAMESPACE, "rollout", "status", "deployment/minio"])
        .args(["--timeout", WAIT])
        .run()
}

/// The store's node, after checking that every store pod runs on it.
fn placement() -> Result<String> {
    let node = store_node(&logged(
        &kubectl()
            .args(["get", "nodes", "-o", "name", "-l"])
            .args([format!("{NODE_KEY}={NODE_ROLE}")]),
    )?)?;
    let pods = logged(
        &kubectl()
            .args([
                "-n",
                NAMESPACE,
                "get",
                "pods",
                "-l",
                "app.kubernetes.io/name=minio",
            ])
            .args(["-o", "jsonpath={.items[*].spec.nodeName}"]),
    )?;
    check_placement(&node, &pods)?;
    Ok(node)
}

/// A field of pod `name` in [`PROBE_NAMESPACE`], by JSONPath.
fn probe_field(name: &str, path: &str) -> Result<String> {
    logged(
        &kubectl()
            .args(["-n", PROBE_NAMESPACE, "get", "pod", name, "-o"])
            .args([format!("jsonpath={path}")]),
    )
    .map(|s| s.trim().to_owned())
}

/// Deploys the store and configures and asserts its bucket, as the module documentation says.
///
/// # Errors
/// Fails when a manifest is unreadable or pins no digest, when `kubectl` fails, or when
/// versioning or SSE does not read back as required.
pub fn up(repo: &Path) -> Result<()> {
    read_manifests(repo)?;
    apply(repo, "namespace.yaml")?;
    let existing = kubectl()
        .args(["-n", NAMESPACE, "get", "secret", "-o", "name"])
        .args(["--field-selector", &format!("metadata.name={SECRET}")])
        .output()?;
    if existing.trim().is_empty() {
        create_secret(repo, NAMESPACE, SECRET, &credentials_env(&random48()?))?;
    } else {
        eprintln!("secret `{SECRET}` exists; keeping it, since the stored objects need its key");
    }
    apply(repo, "store.yaml")?;
    rollout()?;
    let node = placement()?;
    let bucket = &bucket();
    mc(&["mb", "--ignore-existing", bucket])?;
    mc(&["version", "enable", bucket])?;
    mc(&["encrypt", "set", "sse-s3", bucket])?;
    check_versioning(&mc(&["version", "info", bucket])?)?;
    check_encryption(&mc(&["encrypt", "info", bucket])?)?;
    let probe = object("fixture/up-probe");
    logged(&exec([
        "sh",
        "-c",
        &format!("printf up-probe | mc pipe {probe}"),
    ]))?;
    let version = check_object(&mc(&["stat", &probe])?)?;
    eprintln!(
        "artifact store up: bucket `{BUCKET}` in namespace `{NAMESPACE}` on node `{node}`, \
         versioning Enabled, default encryption SSE-S3 (AES256); {probe} stored encrypted as version {version}"
    );
    Ok(())
}

/// Runs the workspace-deletion probe against a running store, as the module documentation
/// says.
///
/// # Errors
/// Fails when a manifest is unreadable or pins no digest, when `kubectl` fails, when the probe
/// pod does not succeed, or when the store's namespace, deployment or the probe's object does
/// not survive the probe namespace's deletion.
pub fn check(repo: &Path) -> Result<()> {
    read_manifests(repo)?;
    rollout()?;
    let node = placement()?;
    let delete_probe = || {
        kubectl()
            .args(["delete", "namespace", PROBE_NAMESPACE, "--ignore-not-found"])
            .args(["--wait", "--timeout", WAIT])
            .run()
    };
    delete_probe()?;
    apply(repo, "workspace-probe-namespace.yaml")?;
    let env = client_env(
        &secret_value("MINIO_ROOT_USER")?,
        &secret_value("MINIO_ROOT_PASSWORD")?,
    );
    create_secret(repo, PROBE_NAMESPACE, PROBE_SECRET, &env)?;
    apply(repo, "workspace-probe.yaml")?;
    kubectl()
        .args(["-n", PROBE_NAMESPACE, "wait", "pod/writer"])
        .args([
            "--for=jsonpath={.status.phase}=Succeeded",
            "--timeout",
            WAIT,
        ])
        .run()?;
    let writer_node = probe_field("writer", "{.spec.nodeName}")?;
    if writer_node == node {
        return Err(assertion(format!(
            "the workspace pod `writer` ran on the store's node `{node}`"
        )));
    }
    const SCHEDULED: &str = "{.status.conditions[?(@.type==\"PodScheduled\")]";
    kubectl()
        .args(["-n", PROBE_NAMESPACE, "wait", "pod/intruder"])
        .args([format!("--for=jsonpath={SCHEDULED}.reason}}=Unschedulable")])
        .args(["--timeout", WAIT])
        .run()?;
    check_repelled(&probe_field(
        "intruder",
        &format!("{SCHEDULED}.reason}} {SCHEDULED}.message}}"),
    )?)?;
    let uid = probe_field("writer", "{.metadata.uid}")?;
    let key = object(&format!("workspace-probe/{uid}"));
    let written = check_object(&mc(&["stat", &key])?)?;
    delete_probe()?;
    let gone = kubectl()
        .args([
            "get",
            "namespace",
            PROBE_NAMESPACE,
            "-o",
            "name",
            "--ignore-not-found",
        ])
        .output()?;
    if !gone.trim().is_empty() {
        return Err(assertion(format!(
            "namespace `{PROBE_NAMESPACE}` still exists"
        )));
    }
    let phase = kubectl()
        .args([
            "get",
            "namespace",
            NAMESPACE,
            "-o",
            "jsonpath={.status.phase}",
        ])
        .output()?;
    if phase.trim() != "Active" {
        return Err(assertion(format!(
            "namespace `{NAMESPACE}` is `{}`, need `Active`",
            phase.trim()
        )));
    }
    rollout()?;
    placement()?;
    let version = check_object(&mc(&["stat", &key])?)?;
    if version != written {
        return Err(assertion(format!(
            "{key} is version {version} after the deletion, was {written}"
        )));
    }
    let content = logged(&exec(["mc", "cat", &key]))?;
    if content.trim_end() != PROBE_NAMESPACE {
        return Err(assertion(format!(
            "{key} reads `{}`, need `{PROBE_NAMESPACE}`",
            content.trim_end()
        )));
    }
    eprintln!(
        "artifact store check: store on node `{node}`, where `intruder` stayed unschedulable and \
         `writer` did not run (it ran on `{writer_node}`); namespace `{PROBE_NAMESPACE}` \
         deleted; namespace `{NAMESPACE}` Active, deployment available, {key} intact \
         (version {version}, SSE-S3)"
    );
    Ok(())
}

/// Deletes the store's namespace, its volume and every object; succeeds when there is none.
///
/// # Errors
/// Fails when `kubectl` fails.
pub fn down() -> Result<()> {
    kubectl()
        .args(["delete", "namespace", NAMESPACE, "--ignore-not-found"])
        .args(["--wait", "--timeout", WAIT])
        .run()
}

/// Entry point of `scripts/artifact_store.rs`: `up`, `down` or `check`, run from the
/// repository root.
///
/// # Errors
/// Fails on a usage error or when the subcommand fails.
pub fn cli(args: &[String]) -> Result<()> {
    match args {
        [cmd] if cmd == "up" => up(Path::new(".")),
        [cmd] if cmd == "down" => down(),
        [cmd] if cmd == "check" => check(Path::new(".")),
        _ => Err(refusal(
            "artifact-store",
            "usage: artifact-store up | artifact-store down | artifact-store check",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yaml_rust2::{Yaml, YamlLoader};

    const DIGEST: &str = "14cea493d9a34af32f524e538b8346cf79f3321eff8e708c1e2960462bd8936e";

    const COMMITTED: [(&str, &str); 4] = [
        (
            "namespace.yaml",
            include_str!("../../../deploy/artifact-store/namespace.yaml"),
        ),
        (
            "store.yaml",
            include_str!("../../../deploy/artifact-store/store.yaml"),
        ),
        (
            "workspace-probe-namespace.yaml",
            include_str!("../../../deploy/artifact-store/workspace-probe-namespace.yaml"),
        ),
        (
            "workspace-probe.yaml",
            include_str!("../../../deploy/artifact-store/workspace-probe.yaml"),
        ),
    ];

    fn documents(name: &str) -> Vec<Yaml> {
        let (_, text) = COMMITTED.iter().find(|(n, _)| *n == name).unwrap();
        YamlLoader::load_from_str(text).unwrap()
    }

    fn str_at<'a>(doc: &'a Yaml, path: &[&str]) -> &'a str {
        path.iter()
            .fold(doc, |node, key| &node[*key])
            .as_str()
            .unwrap_or_default()
    }

    #[test]
    fn the_committed_manifests_are_the_applied_ones_and_pin_every_image() {
        let names: Vec<&str> = COMMITTED.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, MANIFESTS);
        let all: String = COMMITTED.iter().map(|(_, t)| *t).collect();
        check_images(&all).unwrap();
        read_manifests(Path::new("../..")).unwrap();
    }

    #[test]
    fn store_objects_live_in_the_store_namespace_and_nowhere_else() {
        let ns = documents("namespace.yaml");
        assert_eq!(ns.len(), 1);
        assert_eq!(str_at(&ns[0], &["kind"]), "Namespace");
        assert_eq!(str_at(&ns[0], &["metadata", "name"]), NAMESPACE);
        let store = documents("store.yaml");
        let kinds: Vec<&str> = store.iter().map(|d| str_at(d, &["kind"])).collect();
        assert_eq!(kinds, ["PersistentVolumeClaim", "Deployment", "Service"]);
        for doc in &store {
            assert_eq!(str_at(doc, &["metadata", "namespace"]), NAMESPACE);
            assert!(doc["metadata"]["ownerReferences"].is_badvalue());
        }
        assert_eq!(
            SERVICE_URL,
            format!(
                "{}.{NAMESPACE}.svc.cluster.local:9000",
                str_at(&store[2], &["metadata", "name"])
            )
        );
    }

    #[test]
    fn the_store_reads_its_secret_and_its_volume() {
        let store = documents("store.yaml");
        let pod = &store[1]["spec"]["template"]["spec"];
        let container = &pod["containers"][0];
        assert_eq!(
            str_at(&container["envFrom"][0], &["secretRef", "name"]),
            SECRET
        );
        let claim = str_at(&pod["volumes"][0], &["persistentVolumeClaim", "claimName"]);
        assert_eq!(claim, str_at(&store[0], &["metadata", "name"]));
        assert_eq!(str_at(&store[1], &["spec", "strategy", "type"]), "Recreate");
    }

    #[test]
    fn the_probe_lives_in_its_own_namespace_and_reads_its_client_secret() {
        let ns = documents("workspace-probe-namespace.yaml");
        assert_eq!(str_at(&ns[0], &["metadata", "name"]), PROBE_NAMESPACE);
        assert_ne!(PROBE_NAMESPACE, NAMESPACE);
        let pod = documents("workspace-probe.yaml");
        assert_eq!(pod.len(), 2);
        assert_eq!(str_at(&pod[0], &["kind"]), "Pod");
        assert_eq!(str_at(&pod[0], &["metadata", "name"]), "writer");
        assert_eq!(str_at(&pod[0], &["metadata", "namespace"]), PROBE_NAMESPACE);
        let container = &pod[0]["spec"]["containers"][0];
        assert_eq!(
            str_at(&container["envFrom"][0], &["secretRef", "name"]),
            PROBE_SECRET
        );
        let script = container["command"][2].as_str().unwrap_or_default();
        assert!(
            script.contains(&format!(
                "mc pipe \"store/{BUCKET}/workspace-probe/$POD_UID\""
            )),
            "{script}"
        );
        assert!(pod[0]["spec"]["nodeSelector"].is_badvalue());
        assert!(pod[0]["spec"]["tolerations"].is_badvalue());
        let intruder = &pod[1];
        assert_eq!(str_at(intruder, &["metadata", "name"]), "intruder");
        assert_eq!(
            str_at(intruder, &["metadata", "namespace"]),
            PROBE_NAMESPACE
        );
        assert_eq!(
            str_at(intruder, &["spec", "nodeSelector", NODE_KEY]),
            NODE_ROLE
        );
        assert!(intruder["spec"]["tolerations"].is_badvalue());
    }

    #[test]
    fn the_store_selects_and_tolerates_its_dedicated_node() {
        let store = documents("store.yaml");
        let pod = &store[1]["spec"]["template"]["spec"];
        assert_eq!(str_at(pod, &["nodeSelector", NODE_KEY]), NODE_ROLE);
        let tolerations = pod["tolerations"].as_vec().unwrap();
        assert_eq!(tolerations.len(), 1);
        assert_eq!(str_at(&tolerations[0], &["key"]), NODE_KEY);
        assert_eq!(str_at(&tolerations[0], &["operator"]), "Equal");
        assert_eq!(str_at(&tolerations[0], &["value"]), NODE_ROLE);
        assert_eq!(str_at(&tolerations[0], &["effect"]), "NoSchedule");
    }

    #[test]
    fn the_kind_cluster_labels_and_taints_one_worker_for_the_store() {
        let text = include_str!("../../../deploy/kind/cluster.yaml");
        let cluster = YamlLoader::load_from_str(text).unwrap();
        let nodes = cluster[0]["nodes"].as_vec().unwrap();
        let labelled: Vec<&Yaml> = nodes
            .iter()
            .filter(|n| str_at(n, &["labels", NODE_KEY]) == NODE_ROLE)
            .collect();
        assert_eq!(labelled.len(), 1);
        assert_eq!(str_at(labelled[0], &["role"]), "worker");
        let patch = labelled[0]["kubeadmConfigPatches"][0].as_str().unwrap();
        let patch = &YamlLoader::load_from_str(patch).unwrap()[0];
        assert_eq!(str_at(patch, &["kind"]), "JoinConfiguration");
        let taint = &patch["nodeRegistration"]["taints"][0];
        assert_eq!(str_at(taint, &["key"]), NODE_KEY);
        assert_eq!(str_at(taint, &["value"]), NODE_ROLE);
        assert_eq!(str_at(taint, &["effect"]), "NoSchedule");
        let others: Vec<&Yaml> = nodes
            .iter()
            .filter(|n| str_at(n, &["labels", NODE_KEY]) != NODE_ROLE)
            .collect();
        assert_eq!(others.len(), 1);
        let init = others[0]["kubeadmConfigPatches"][0].as_str().unwrap();
        let init = &YamlLoader::load_from_str(init).unwrap()[0];
        assert_eq!(str_at(init, &["kind"]), "InitConfiguration");
        assert_eq!(
            init["nodeRegistration"]["taints"].as_vec().map(Vec::len),
            Some(0)
        );
    }

    #[test]
    fn exactly_one_store_node_is_required_and_every_store_pod_runs_on_it() {
        assert_eq!(
            store_node("node/autobot-worker\n").unwrap(),
            "autobot-worker"
        );
        for output in ["", "node/a\nnode/b\n"] {
            let err = store_node(output).unwrap_err().to_string();
            assert!(err.contains("need exactly one node"), "{err}");
        }
        check_placement("autobot-worker", "autobot-worker").unwrap();
        for pods in [
            "",
            "autobot-control-plane",
            "autobot-worker autobot-control-plane",
        ] {
            let err = check_placement("autobot-worker", pods)
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("need all on `autobot-worker`"),
                "{pods}: {err}"
            );
        }
    }

    #[test]
    fn a_pod_without_the_toleration_must_be_repelled_by_the_taint() {
        check_repelled(
            "Unschedulable 0/2 nodes are available: 1 node(s) didn't match Pod's node \
             affinity/selector, 1 node(s) had untolerated taint(s). preemption: 0/2 nodes are \
             available: 2 Preemption is not helpful for scheduling.",
        )
        .unwrap();
        for condition in [
            "",
            "Unschedulable 0/2 nodes are available: 2 Insufficient cpu.",
            "had untolerated taint(s)",
        ] {
            assert!(check_repelled(condition).is_err(), "{condition}");
        }
    }

    #[test]
    fn every_image_line_must_be_pinned() {
        let pinned = format!("      image: minio@sha256:{DIGEST}\n");
        let quoted = format!("    - image: \"minio@sha256:{DIGEST}\"\n");
        check_images(&format!("{pinned}{quoted}")).unwrap();
        let err = check_images(&format!("{pinned}      image: minio:latest\n"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("`minio:latest` is not pinned"), "{err}");
        let err = check_images("kind: Namespace\n").unwrap_err().to_string();
        assert!(err.contains("no image"), "{err}");
    }

    #[test]
    fn versioning_must_read_back_enabled() {
        check_versioning(
            r#"{"Op":"info","status":"success","url":"local/artifacts","versioning":{"status":"Enabled","MFADelete":""}}"#,
        )
        .unwrap();
        for output in [
            r#"{"versioning":{"status":"Suspended"}}"#,
            r#"{"versioning":{"status":""}}"#,
            r#"{"status":"success"}"#,
        ] {
            let err = check_versioning(output).unwrap_err().to_string();
            assert!(err.contains("need \"Enabled\""), "{err}");
        }
        assert!(check_versioning("not json").is_err());
    }

    #[test]
    fn default_encryption_must_read_back_sse_s3() {
        check_encryption(
            r#"{"op":"info","status":"success","url":"local/artifacts","encryption":{"algorithm":"AES256"}}"#,
        )
        .unwrap();
        for output in [
            r#"{"encryption":{"algorithm":"aws:kms"}}"#,
            r#"{"encryption":{}}"#,
            "{}",
        ] {
            let err = check_encryption(output).unwrap_err().to_string();
            assert!(err.contains("need \"AES256\""), "{err}");
        }
    }

    #[test]
    fn an_object_must_be_encrypted_and_versioned() {
        let stat = |sse: &str, version: &str| {
            format!(
                r#"{{"status":"success","name":"x","metadata":{{"Content-Type":"application/octet-stream"{sse}}}{version}}}"#
            )
        };
        let sse = r#","X-Amz-Server-Side-Encryption":"AES256""#;
        let id = r#","versionID":"f53eef69-d1f5-4a30-a2fc-cb4631b8f504""#;
        assert_eq!(
            check_object(&stat(sse, id)).unwrap(),
            "f53eef69-d1f5-4a30-a2fc-cb4631b8f504"
        );
        let err = check_object(&stat("", id)).unwrap_err().to_string();
        assert!(err.contains("encryption is None"), "{err}");
        for version in ["", r#","versionID":"""#, r#","versionID":"null""#] {
            let err = check_object(&stat(sse, version)).unwrap_err().to_string();
            assert!(err.contains("version id"), "{version}: {err}");
        }
    }

    #[test]
    fn base64_matches_rfc_4648_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), expected, "{input}");
        }
        assert_eq!(base64(&[0xfb, 0xff, 0xbf]), "+/+/");
    }

    #[test]
    fn the_credentials_split_the_random_bytes_into_password_and_key() {
        let mut random = [0u8; 48];
        for (i, b) in random.iter_mut().enumerate() {
            *b = u8::try_from(i).unwrap();
        }
        let env = credentials_env(&random);
        let lines: Vec<&str> = env.lines().collect();
        assert_eq!(
            lines,
            [
                "MINIO_ROOT_USER=autobot",
                "MINIO_ROOT_PASSWORD=000102030405060708090a0b0c0d0e0f",
                "MINIO_KMS_SECRET_KEY=autobot-fixture:EBESExQVFhcYGRobHB0eHyAhIiMkJSYnKCkqKywtLi8=",
            ]
        );
    }

    #[test]
    fn the_client_alias_reaches_the_store_service() {
        assert_eq!(
            client_env("u", "p"),
            "MC_HOST_store=http://u:p@minio.autobot-artifact-store.svc.cluster.local:9000\n"
        );
    }

    #[test]
    fn usage_errors_run_nothing() {
        for args in [
            &[][..],
            &["load".to_owned()],
            &["up".to_owned(), "x".to_owned()],
        ] {
            let err = cli(args).unwrap_err().to_string();
            assert!(err.contains("usage: artifact-store up"), "{err}");
        }
    }
}
