//! The adapter against the MinIO artifact-store fixture on the kind cluster.
//!
//! The test deploys the fixture with `autobot_devtools::artifact_store::up`, which is what
//! `just artifact-store-up` runs and which leaves a running fixture as it is, reads the
//! fixture's credentials from its Secret and reaches its Service through `kubectl
//! port-forward`. A store the contract suite faults is reached through a [`Proxy`] on the
//! loopback interface, which injects the faults on the wire, so the adapter under test carries
//! no fault hook: a dropped upload is a `PUT` connection closed before anything reaches the
//! store, a lost acknowledgement is a `PUT` relayed to the store whose answer is withheld, and an
//! unavailable store is a proxy that no longer listens. Another client of the same store connects
//! to the port-forward directly.
//!
//! Everything runs in one test, because deploying the fixture from two processes at once could
//! race on creating its Secret.

use super::*;
use autobot_adapters::artifact::{ArtifactHarness, StoreFault, run};
use autobot_devtools::artifact_store::{self as fixture, BUCKET, NAMESPACE, SECRET};
use autobot_devtools::kind::CLUSTER;
use autobot_devtools::process::Cmd;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

fn context() -> String {
    format!("kind-{CLUSTER}")
}

/// A value of the fixture's Secret.
fn secret(name: &str) -> String {
    Cmd::new("kubectl")
        .args(["--context".to_owned(), context()])
        .args(["-n", NAMESPACE, "get", "secret", SECRET, "-o"])
        .args([format!("go-template={{{{.data.{name} | base64decode}}}}")])
        .output()
        .unwrap_or_else(|e| panic!("reading {name} from the fixture's Secret: {e}"))
}

/// A `kubectl port-forward` to the fixture's Service, stopped on drop.
struct Forward {
    child: Child,
    addr: SocketAddr,
}

impl Forward {
    fn start() -> Self {
        let mut child = Command::new("kubectl")
            .args(["--context", &context(), "-n", NAMESPACE, "port-forward"])
            .args(["--address", "127.0.0.1", "svc/minio", ":9000"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("kubectl port-forward starts");
        let stdout = child.stdout.take().expect("port-forward stdout is piped");
        let mut lines = BufReader::new(stdout).lines();
        // kubectl prints `Forwarding from 127.0.0.1:<port> -> 9000` once it listens.
        let port = loop {
            let line = lines
                .next()
                .expect("port-forward prints its local port")
                .expect("port-forward output is text");
            if let Some(rest) = line.strip_prefix("Forwarding from 127.0.0.1:") {
                break rest
                    .split_whitespace()
                    .next()
                    .and_then(|p| p.parse::<u16>().ok())
                    .expect("a port number");
            }
        };
        // Keep reading, so kubectl never blocks on a full pipe.
        thread::spawn(move || lines.for_each(drop));
        Self {
            child,
            addr: SocketAddr::from(([127, 0, 0, 1], port)),
        }
    }
}

impl Drop for Forward {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A loopback TCP relay to the store that injects the next put's fault and stops listening
/// when the store is made unavailable.
struct Proxy {
    addr: SocketAddr,
    fault: Arc<Mutex<Option<StoreFault>>>,
    listening: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
}

impl Proxy {
    fn start(upstream: SocketAddr) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        listener
            .set_nonblocking(true)
            .expect("a non-blocking listener");
        let addr = listener.local_addr().expect("a bound address");
        let fault = Arc::new(Mutex::new(None));
        let listening = Arc::new(AtomicBool::new(true));
        let accept = {
            let fault = Arc::clone(&fault);
            let listening = Arc::clone(&listening);
            thread::spawn(move || {
                while listening.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((client, _)) => {
                            let fault = Arc::clone(&fault);
                            thread::spawn(move || relay(client, upstream, &fault));
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(2));
                        }
                        Err(_) => break,
                    }
                }
                // The listener drops here: from now on a connection is refused.
            })
        };
        Self {
            addr,
            fault,
            listening,
            accept: Some(accept),
        }
    }

    fn fault_next_put(&self, fault: StoreFault) {
        *self.fault.lock().unwrap_or_else(PoisonError::into_inner) = Some(fault);
    }

    /// Stops listening, and returns once a connection to [`Proxy::addr`] is refused.
    fn close(&mut self) {
        self.listening.store(false, Ordering::SeqCst);
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.close();
    }
}

/// Relays one connection; a `PUT` takes the pending fault.
fn relay(mut client: TcpStream, upstream: SocketAddr, fault: &Mutex<Option<StoreFault>>) {
    let _ = client.set_nonblocking(false);
    let mut head = [0u8; 4];
    if client.read_exact(&mut head).is_err() {
        return;
    }
    let fault = if &head == b"PUT " {
        fault.lock().unwrap_or_else(PoisonError::into_inner).take()
    } else {
        None
    };
    if fault == Some(StoreFault::DroppedUpload) {
        // Dropping the connection unread resets it; the store never sees the request.
        return;
    }
    let Ok(mut server) = TcpStream::connect(upstream) else {
        return;
    };
    let (Ok(mut from_client), Ok(mut to_server)) = (client.try_clone(), server.try_clone()) else {
        return;
    };
    if to_server.write_all(&head).is_err() {
        return;
    }
    let up = thread::spawn(move || {
        let _ = io::copy(&mut from_client, &mut to_server);
        let _ = to_server.shutdown(Shutdown::Write);
    });
    if fault == Some(StoreFault::LostAcknowledgement) {
        // The store answers only once it has stored the object: read its answer's head, then
        // end the client's connection without relaying it.
        let mut seen = Vec::new();
        let mut buf = [0u8; 4096];
        while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
            match server.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => seen.extend_from_slice(&buf[..n]),
            }
        }
    } else {
        let _ = io::copy(&mut server, &mut client);
    }
    let _ = client.shutdown(Shutdown::Both);
    let _ = up.join();
    let _ = server.shutdown(Shutdown::Both);
}

/// A client under test, with the proxy it reaches the store through when it is a store the
/// suite faults.
struct Proxied {
    store: S3ArtifactStore,
    proxy: Option<Proxy>,
}

impl ArtifactStore for Proxied {
    fn put(&mut self, key: &ArtifactKey, bytes: &[u8]) -> Result<StoredArtifact, StoreError> {
        self.store.put(key, bytes)
    }

    fn get(&mut self, artifact: &StoredArtifact) -> Result<Vec<u8>, StoreError> {
        self.store.get(artifact)
    }

    fn lookup(
        &mut self,
        key: &ArtifactKey,
        digest: &Digest,
    ) -> Result<Option<StoredArtifact>, StoreError> {
        self.store.lookup(key, digest)
    }
}

struct Harness {
    forward: Forward,
    user: String,
    password: String,
    /// Distinguishes this run's stores from every other run's in the shared bucket.
    run: String,
    stores: usize,
}

impl Harness {
    fn start() -> Self {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        fixture::up(&repo).expect("the artifact-store fixture is up");
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        Self {
            forward: Forward::start(),
            user: secret("MINIO_ROOT_USER"),
            password: secret("MINIO_ROOT_PASSWORD"),
            run: format!("contract/{nanos}-{}", std::process::id()),
            stores: 0,
        }
    }

    fn config(&self, endpoint: SocketAddr, prefix: &str) -> S3Config {
        S3Config {
            endpoint: format!("http://{endpoint}"),
            region: "us-east-1".to_owned(),
            bucket: BUCKET.to_owned(),
            prefix: prefix.to_owned(),
            access_key: self.user.clone(),
            secret_key: self.password.clone(),
            max_bytes: 1 << 20,
            timeout: Duration::from_secs(60),
        }
    }
}

impl ArtifactHarness for Harness {
    type Store = Proxied;

    fn store(&mut self) -> Proxied {
        let n = self.stores;
        self.stores += 1;
        let proxy = Proxy::start(self.forward.addr);
        let config = self.config(proxy.addr, &format!("{}/{n}", self.run));
        Proxied {
            store: S3ArtifactStore::connect(config).expect("a client through the proxy"),
            proxy: Some(proxy),
        }
    }

    fn client(&mut self, store: &Proxied) -> Proxied {
        let prefix = store.store.config().prefix.clone();
        Proxied {
            store: S3ArtifactStore::connect(self.config(self.forward.addr, &prefix))
                .expect("a direct client"),
            proxy: None,
        }
    }

    fn fault_next_put(&mut self, store: &mut Proxied, fault: StoreFault) {
        store
            .proxy
            .as_ref()
            .expect("the suite faults a store, not a client")
            .fault_next_put(fault);
    }

    fn set_available(&mut self, store: &mut Proxied, available: bool) {
        assert!(!available, "the suite only makes a store unavailable");
        store
            .proxy
            .as_mut()
            .expect("the suite makes a store unavailable, not a client")
            .close();
    }
}

#[test]
fn the_adapter_keeps_the_contract_on_the_fixture() {
    let mut harness = Harness::start();

    // Acceptance: the artifact-store contract suite, against the fixture.
    assert_eq!(run(&mut harness), Ok(()));

    // Acceptance: an upload whose acknowledgement is lost is settled by lookup.
    let key = ArtifactKey::new("workspace/manifest").expect("a key");
    let mut store = harness.store();
    let first = store.put(&key, b"first").expect("a put");
    harness.fault_next_put(&mut store, StoreFault::LostAcknowledgement);
    assert_eq!(store.put(&key, b"lost ack"), Err(StoreError::Uncertain));
    let settled = store
        .lookup(&key, &sha256(b"lost ack"))
        .expect("a lookup")
        .expect("the upload was stored");
    assert_eq!(settled.version, first.version + 1);
    let mut other = harness.client(&store);
    assert_eq!(other.get(&settled).as_deref(), Ok(&b"lost ack"[..]));
    assert_eq!(
        store.put(&key, b"next").expect("a put").version,
        settled.version + 1
    );

    // The objects are encrypted, versioned by the bucket, and never replaced.
    let path = store.store.object_path(&key, first.version);
    let head = store
        .store
        .send("HEAD", &path, &[], &[], (), EMPTY_SHA256)
        .expect("a HEAD answer");
    let header = |name: &str| {
        head.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    assert_eq!(
        header("x-amz-server-side-encryption").as_deref(),
        Some("AES256")
    );
    assert!(
        header("x-amz-version-id").is_some_and(|v| !v.is_empty() && v != "null"),
        "{:?}",
        header("x-amz-version-id")
    );
    assert_eq!(header(DIGEST_METADATA), Some(first.digest.to_string()));
    let replace = store.store.send(
        "PUT",
        &path,
        &[],
        &[("if-none-match", "*")],
        &b"replacement"[..],
        &sigv4::sha256_hex(b"replacement"),
    );
    assert_eq!(claim(&replace), Claim::Taken);
    assert_eq!(other.get(&first).as_deref(), Ok(&b"first"[..]));

    // A client of a bucket that does not exist is refused.
    let mut missing = harness.config(harness.forward.addr, &harness.run);
    missing.bucket = "no-such-bucket".to_owned();
    assert!(
        matches!(
            S3ArtifactStore::connect(missing),
            Err(ConnectError::Store(_))
        ),
        "a missing bucket is refused"
    );
}
