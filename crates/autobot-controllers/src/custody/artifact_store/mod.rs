//! The artifact store adapter: an [`ArtifactStore`] over an S3-compatible bucket, the custody
//! that `docs/design/AUTOBOT-TRUST-MODEL.md` §Trusted names and the artifact fixture of
//! `docs/design/AUTOBOT-M0-AND-GATES.md` §2 provides.
//!
//! [`S3ArtifactStore::connect`] checks the bucket's versioning before it returns a client. The
//! client stores each version of an artifact as its own object, reads it back by name and finds
//! a version by digest, which is how a custody checkpoint (`docs/design/AUTOBOT-KERNEL.md` §7)
//! settles an upload whose acknowledgement was lost.
//!
//! Choices this module makes where the design is open:
//!
//! - Version `n` of key `k` is the object `<prefix>/<k>/<n>`, `n` in [`VERSION_DIGITS`] decimal
//!   digits, in path-style addressing. A put lists the key's versions and creates the next one
//!   with a conditional create (`If-None-Match: *`), so no put replaces an object; when another
//!   client took that version first, it lists again, up to [`CLAIM_ATTEMPTS`] times. The adapter
//!   never deletes an object, and the bucket must have versioning enabled, so an original also
//!   survives a writer that does not follow this layout.
//! - Every put asks for SSE-S3 (`x-amz-server-side-encryption: AES256`) and records the
//!   SHA-256 of the bytes in the object's metadata ([`DIGEST_METADATA`]). The metadata is the
//!   store's word only: a lookup reads it to choose a candidate and answers the candidate only
//!   after it has read the bytes back and hashed them, and a get answers
//!   [`StoreError::NotFound`] when the bytes do not hash to the digest it was asked for.
//! - The size bound is the profile's per-workspace artifact bound
//!   ([`Artifacts::workspace_max_gib`], through [`S3Config::bound`]), applied to each put and to
//!   each read. A put over it is refused before any request and answers
//!   [`StoreError::Unavailable`], the one answer the trait has for "nothing was stored"; the
//!   trait has no dedicated refusal. The footprint of a workspace over several artifacts is
//!   left to the custody controller, which knows which keys belong to a workspace.
//! - Each request opens its own connection, so a failure belongs to one request. A put that
//!   fails before its connection is open, or is answered with a redirect, a client error other than
//!   `409` and `412` (another client took the version) or `503`, answers
//!   [`StoreError::Unavailable`]; any other failure after the connection is open, and any other
//!   server error, answers [`StoreError::Uncertain`]. A get or lookup that gets no usable answer
//!   answers [`StoreError::Unavailable`].
//! - Requests are signed with AWS Signature Version 4 over the SHA-256 of the body, so the
//!   store rejects a body altered in transit.
//! - There is no replication: one bucket is one copy (replicated artifacts arrive with
//!   G-FENCE-CUSTODY, M0 §3).

mod sigv4;
mod xml;

#[cfg(test)]
mod kind_cluster;
#[cfg(test)]
mod tests;

use autobot_adapters::artifact::{ArtifactStore, StoreError, StoredArtifact, sha256};
use autobot_adapters::text::ArtifactKey;
use autobot_kernel::profile::Artifacts;
use autobot_kernel::types::Digest;
use std::fmt;
use std::time::Duration;
use ureq::http::{Response, StatusCode};
use ureq::{AsSendBody, Body};

/// The number of decimal digits of the version in an object name.
pub const VERSION_DIGITS: usize = 20;

/// How many times a put lists and tries to create the next version before it gives up.
pub const CLAIM_ATTEMPTS: usize = 8;

/// The object metadata header that holds the SHA-256 of an object's bytes, in the text form of
/// [`Digest`].
pub const DIGEST_METADATA: &str = "x-amz-meta-sha256";

/// The SHA-256 of an empty body.
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// The most bytes read from a bucket configuration or listing answer.
const XML_LIMIT: u64 = 16 << 20;

/// Where the store is and how the adapter uses it.
#[derive(Clone)]
pub struct S3Config {
    /// The store's base URL, `http://` or `https://` and an authority, such as
    /// `https://s3.example.com` or `http://127.0.0.1:9000`.
    pub endpoint: String,
    /// The region requests are signed for; S3-compatible stores commonly use `us-east-1`.
    pub region: String,
    /// The bucket.
    pub bucket: String,
    /// The object name prefix under which this client keeps artifacts, without leading or
    /// trailing `/`; empty for the bucket's root.
    pub prefix: String,
    /// The access key id.
    pub access_key: String,
    /// The secret access key.
    pub secret_key: String,
    /// The largest artifact, in bytes, a put stores or a get reads.
    pub max_bytes: u64,
    /// The longest one request may take, including its body.
    pub timeout: Duration,
}

impl S3Config {
    /// The profile's per-workspace artifact bound, in bytes.
    #[must_use]
    pub fn bound(artifacts: &Artifacts) -> u64 {
        u64::from(artifacts.workspace_max_gib.get()) << 30
    }
}

impl fmt::Debug for S3Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("S3Config")
            .field("endpoint", &self.endpoint)
            .field("region", &self.region)
            .field("bucket", &self.bucket)
            .field("prefix", &self.prefix)
            .field("access_key", &self.access_key)
            .field("secret_key", &"<redacted>")
            .field("max_bytes", &self.max_bytes)
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Why [`S3ArtifactStore::connect`] returned no client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectError {
    /// The configuration is not usable; the text says which field and why.
    Config(String),
    /// The store gave no answer, or an error answer, to the versioning check.
    Store(String),
    /// The bucket's versioning is not enabled; the status it reported, if any.
    NotVersioned(Option<String>),
}

impl fmt::Display for ConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(why) => write!(f, "artifact store configuration: {why}"),
            Self::Store(why) => write!(f, "artifact store versioning check: {why}"),
            Self::NotVersioned(status) => {
                write!(
                    f,
                    "artifact store bucket versioning is {status:?}, need Enabled"
                )
            }
        }
    }
}

impl std::error::Error for ConnectError {}

/// A client of an S3-compatible artifact store.
pub struct S3ArtifactStore {
    agent: ureq::Agent,
    /// `scheme://authority`, without a default port.
    base: String,
    /// The `host` header value: the authority of `base`.
    host: String,
    config: S3Config,
}

impl fmt::Debug for S3ArtifactStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("S3ArtifactStore")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

/// `scheme://authority` and the authority of `endpoint`, without a default port.
fn parse_endpoint(endpoint: &str) -> Result<(String, String), ConnectError> {
    let invalid = || {
        ConnectError::Config(format!(
            "endpoint `{endpoint}` is not http:// or https:// and an authority"
        ))
    };
    let (scheme, rest) = endpoint.split_once("://").ok_or_else(invalid)?;
    let default_port = match scheme {
        "http" => ":80",
        "https" => ":443",
        _ => return Err(invalid()),
    };
    let authority = rest.strip_suffix('/').unwrap_or(rest);
    if authority.is_empty() || authority.contains(['/', '?', '#', '@']) {
        return Err(invalid());
    }
    let host = authority.strip_suffix(default_port).unwrap_or(authority);
    Ok((format!("{scheme}://{host}"), host.to_owned()))
}

/// Whether `error` happened before the request could reach the store: nothing was sent.
fn not_sent(error: &ureq::Error) -> bool {
    use std::io::ErrorKind;
    match error {
        ureq::Error::HostNotFound
        | ureq::Error::ConnectionFailed
        | ureq::Error::BadUri(_)
        | ureq::Error::Http(_) => true,
        ureq::Error::Timeout(t) => matches!(t, ureq::Timeout::Resolve | ureq::Timeout::Connect),
        ureq::Error::Io(e) => matches!(
            e.kind(),
            ErrorKind::ConnectionRefused
                | ErrorKind::HostUnreachable
                | ErrorKind::NetworkUnreachable
                | ErrorKind::AddrNotAvailable
        ),
        _ => false,
    }
}

/// What a put's answer means: stored, taken by another client, or an error.
#[derive(Debug, PartialEq, Eq)]
enum Claim {
    Stored,
    Taken,
    Failed(StoreError),
}

/// The meaning of the answer to the conditional create of one version.
fn claim(answer: &Result<Response<Body>, ureq::Error>) -> Claim {
    match answer {
        Err(e) if not_sent(e) => Claim::Failed(StoreError::Unavailable),
        Err(_) => Claim::Failed(StoreError::Uncertain),
        Ok(r) => match r.status().as_u16() {
            200..=299 => Claim::Stored,
            409 | 412 => Claim::Taken,
            503 => Claim::Failed(StoreError::Unavailable),
            500..=599 => Claim::Failed(StoreError::Uncertain),
            _ => Claim::Failed(StoreError::Unavailable),
        },
    }
}

/// `text` with `%XX` escapes and `+` decoded, as a listing with `encoding-type=url` encodes an
/// object name; `None` when the result is not UTF-8.
fn url_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = text.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// The version an encoded listed object name `listed` holds under the listing prefix `prefix`,
/// if it is a version object of this layout.
fn version_of(listed: &str, prefix: &str) -> Option<u64> {
    let name = url_decode(listed)?;
    let digits = name.strip_prefix(prefix)?;
    if digits.len() == VERSION_DIGITS && digits.bytes().all(|b| b.is_ascii_digit()) {
        digits.parse().ok()
    } else {
        None
    }
}

impl S3ArtifactStore {
    /// A client of the store `config` names, after checking that its bucket has versioning
    /// enabled.
    ///
    /// # Errors
    ///
    /// [`ConnectError::Config`] for an unusable endpoint, bucket or prefix,
    /// [`ConnectError::Store`] when the check gets no answer or an error answer, and
    /// [`ConnectError::NotVersioned`] when versioning is not enabled.
    pub fn connect(config: S3Config) -> Result<Self, ConnectError> {
        let store = Self::unchecked(config)?;
        let answer = store
            .send(
                "GET",
                &store.bucket_path(),
                &[("versioning", "")],
                &[],
                (),
                EMPTY_SHA256,
            )
            .map_err(|e| ConnectError::Store(e.to_string()))?;
        let status = answer.status();
        let xml = read_xml(answer).ok_or_else(|| {
            ConnectError::Store("the versioning answer is not readable".to_owned())
        })?;
        if status != StatusCode::OK {
            return Err(ConnectError::Store(format!(
                "status {status}, code {:?}",
                xml::text(&xml, "Code")
            )));
        }
        match xml::text(&xml, "Status") {
            Some(s) if s == "Enabled" => Ok(store),
            other => Err(ConnectError::NotVersioned(other)),
        }
    }

    /// A client of the store `config` names, without the versioning check.
    fn unchecked(config: S3Config) -> Result<Self, ConnectError> {
        let (base, host) = parse_endpoint(&config.endpoint)?;
        if config.bucket.is_empty() || config.bucket.contains('/') {
            return Err(ConnectError::Config(format!(
                "bucket `{}` is empty or holds `/`",
                config.bucket
            )));
        }
        if config.prefix.starts_with('/') || config.prefix.ends_with('/') {
            return Err(ConnectError::Config(format!(
                "prefix `{}` starts or ends with `/`",
                config.prefix
            )));
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .max_idle_connections(0)
            .max_idle_connections_per_host(0)
            .timeout_global(Some(config.timeout))
            .build()
            .into();
        Ok(Self {
            agent,
            base,
            host,
            config,
        })
    }

    /// The configuration this client was made with.
    #[must_use]
    pub fn config(&self) -> &S3Config {
        &self.config
    }

    fn bucket_path(&self) -> String {
        format!("/{}", self.config.bucket)
    }

    /// The object name prefix of the versions of `key`.
    fn listing_prefix(&self, key: &ArtifactKey) -> String {
        if self.config.prefix.is_empty() {
            format!("{key}/")
        } else {
            format!("{}/{key}/", self.config.prefix)
        }
    }

    /// The request path of version `version` of `key`.
    fn object_path(&self, key: &ArtifactKey, version: u64) -> String {
        format!(
            "/{}/{}{version:0width$}",
            self.config.bucket,
            self.listing_prefix(key),
            width = VERSION_DIGITS
        )
    }

    /// Signs and sends one request; `path` is not yet encoded and `payload_sha256` is the
    /// hexadecimal SHA-256 of `body`.
    fn send(
        &self,
        method: &str,
        path: &str,
        query: &[(&str, &str)],
        extra: &[(&str, &str)],
        body: impl AsSendBody,
        payload_sha256: &str,
    ) -> Result<Response<Body>, ureq::Error> {
        let date = sigv4::now();
        let mut headers: Vec<(&str, &str)> = vec![
            ("host", &self.host),
            ("x-amz-content-sha256", payload_sha256),
            ("x-amz-date", &date),
        ];
        headers.extend_from_slice(extra);
        let path = sigv4::encode_path(path);
        let signer = sigv4::Signer {
            access_key: &self.config.access_key,
            secret_key: &self.config.secret_key,
            region: &self.config.region,
        };
        let authorization = signer.authorization(
            &sigv4::Request {
                method,
                path: &path,
                query,
                headers: &headers,
                payload_sha256,
            },
            &date,
        );
        let query = sigv4::encode_query(query);
        let uri = if query.is_empty() {
            format!("{}{path}", self.base)
        } else {
            format!("{}{path}?{query}", self.base)
        };
        let mut request = ureq::http::Request::builder().method(method).uri(uri);
        // ureq writes `host` from the URI, which carries the same authority.
        for (name, value) in headers.iter().filter(|(name, _)| *name != "host") {
            request = request.header(*name, *value);
        }
        let request = request.header("authorization", authorization).body(body)?;
        self.agent.run(request)
    }

    /// Every version of `key` the store lists, in ascending order.
    fn versions(&self, key: &ArtifactKey) -> Result<Vec<u64>, StoreError> {
        let prefix = self.listing_prefix(key);
        let mut versions = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut query = vec![
                ("delimiter", "/"),
                ("encoding-type", "url"),
                ("list-type", "2"),
                ("prefix", prefix.as_str()),
            ];
            if let Some(token) = &token {
                query.push(("continuation-token", token.as_str()));
            }
            let answer = self
                .send("GET", &self.bucket_path(), &query, &[], (), EMPTY_SHA256)
                .map_err(|_| StoreError::Unavailable)?;
            if answer.status() != StatusCode::OK {
                return Err(StoreError::Unavailable);
            }
            let xml = read_xml(answer).ok_or(StoreError::Unavailable)?;
            versions.extend(
                xml::texts(&xml, "Key")
                    .iter()
                    .filter_map(|listed| version_of(listed, &prefix)),
            );
            match (
                xml::text(&xml, "IsTruncated").as_deref(),
                xml::text(&xml, "NextContinuationToken"),
            ) {
                (Some("true"), Some(next)) => token = Some(next),
                (Some("true"), None) => return Err(StoreError::Unavailable),
                _ => break,
            }
        }
        versions.sort_unstable();
        versions.dedup();
        Ok(versions)
    }

    /// The digest the metadata of version `version` of `key` claims; `Ok(None)` when the
    /// object or its claim is missing.
    fn claimed_digest(
        &self,
        key: &ArtifactKey,
        version: u64,
    ) -> Result<Option<Digest>, StoreError> {
        let answer = self
            .send(
                "HEAD",
                &self.object_path(key, version),
                &[],
                &[],
                (),
                EMPTY_SHA256,
            )
            .map_err(|_| StoreError::Unavailable)?;
        match answer.status().as_u16() {
            200 => Ok(answer
                .headers()
                .get(DIGEST_METADATA)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())),
            404 => Ok(None),
            _ => Err(StoreError::Unavailable),
        }
    }
}

/// The UTF-8 body of `answer`, up to [`XML_LIMIT`] bytes.
fn read_xml(mut answer: Response<Body>) -> Option<String> {
    let bytes = answer
        .body_mut()
        .with_config()
        .limit(XML_LIMIT)
        .read_to_vec()
        .ok()?;
    String::from_utf8(bytes).ok()
}

impl ArtifactStore for S3ArtifactStore {
    fn put(&mut self, key: &ArtifactKey, bytes: &[u8]) -> Result<StoredArtifact, StoreError> {
        if u64::try_from(bytes.len()).map_or(true, |len| len > self.config.max_bytes) {
            return Err(StoreError::Unavailable);
        }
        let digest = sha256(bytes);
        let payload = sigv4::hex(digest.as_bytes());
        let claimed = digest.to_string();
        let extra = [
            ("if-none-match", "*"),
            ("x-amz-server-side-encryption", "AES256"),
            (DIGEST_METADATA, claimed.as_str()),
        ];
        for _ in 0..CLAIM_ATTEMPTS {
            let version = match self.versions(key)?.last() {
                None => 0,
                Some(last) => last.checked_add(1).ok_or(StoreError::Unavailable)?,
            };
            let path = self.object_path(key, version);
            match claim(&self.send("PUT", &path, &[], &extra, bytes, &payload)) {
                Claim::Stored => {
                    return Ok(StoredArtifact {
                        key: key.clone(),
                        version,
                        digest,
                    });
                }
                Claim::Taken => {}
                Claim::Failed(e) => return Err(e),
            }
        }
        Err(StoreError::Unavailable)
    }

    fn get(&mut self, artifact: &StoredArtifact) -> Result<Vec<u8>, StoreError> {
        let path = self.object_path(&artifact.key, artifact.version);
        let mut answer = self
            .send("GET", &path, &[], &[], (), EMPTY_SHA256)
            .map_err(|_| StoreError::Unavailable)?;
        match answer.status().as_u16() {
            200 => {}
            404 => return Err(StoreError::NotFound),
            _ => return Err(StoreError::Unavailable),
        }
        let bytes = answer
            .body_mut()
            .with_config()
            .limit(self.config.max_bytes)
            .read_to_vec()
            .map_err(|_| StoreError::Unavailable)?;
        if sha256(&bytes) == artifact.digest {
            Ok(bytes)
        } else {
            Err(StoreError::NotFound)
        }
    }

    fn lookup(
        &mut self,
        key: &ArtifactKey,
        digest: &Digest,
    ) -> Result<Option<StoredArtifact>, StoreError> {
        for version in self.versions(key)? {
            if self.claimed_digest(key, version)?.as_ref() != Some(digest) {
                continue;
            }
            let candidate = StoredArtifact {
                key: key.clone(),
                version,
                digest: *digest,
            };
            match self.get(&candidate) {
                Ok(_) => return Ok(Some(candidate)),
                Err(StoreError::NotFound) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(None)
    }
}
