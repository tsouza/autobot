use super::*;
use crate::contract::testing::assert_breaks;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

/// One way a broken double departs from the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Break {
    WrongDigest,
    ClientLocal,
    Overwrites,
    LookupMisses,
    InventsBytes,
    CorruptsOnRead,
    HidesLostAck,
    AnswersWhenUnavailable,
}

/// The store's versions per key, shared by its clients.
type Versions = Rc<RefCell<BTreeMap<ArtifactKey, Vec<Vec<u8>>>>>;

struct Client {
    shared: Versions,
    broken: Option<Break>,
    available: Rc<RefCell<bool>>,
    fault: Option<StoreFault>,
}

impl Client {
    fn is(&self, b: Break) -> bool {
        self.broken == Some(b)
    }

    fn down(&self) -> bool {
        !*self.available.borrow() && !self.is(Break::AnswersWhenUnavailable)
    }

    fn stored(&self, key: &ArtifactKey, version: usize, bytes: &[u8]) -> StoredArtifact {
        StoredArtifact {
            key: key.clone(),
            version: u64::try_from(version).unwrap(),
            digest: if self.is(Break::WrongDigest) {
                sha256(b"something else")
            } else {
                sha256(bytes)
            },
        }
    }
}

impl ArtifactStore for Client {
    fn put(&mut self, key: &ArtifactKey, bytes: &[u8]) -> Result<StoredArtifact, StoreError> {
        if self.down() {
            return Err(StoreError::Unavailable);
        }
        if self.fault == Some(StoreFault::DroppedUpload) {
            self.fault = None;
            return Err(StoreError::Uncertain);
        }
        let version = {
            let mut shared = self.shared.borrow_mut();
            let versions = shared.entry(key.clone()).or_default();
            if self.is(Break::Overwrites) && !versions.is_empty() {
                versions[0] = bytes.to_vec();
                0
            } else {
                versions.push(bytes.to_vec());
                versions.len() - 1
            }
        };
        if self.fault.take() == Some(StoreFault::LostAcknowledgement)
            && !self.is(Break::HidesLostAck)
        {
            return Err(StoreError::Uncertain);
        }
        Ok(self.stored(key, version, bytes))
    }

    fn get(&mut self, artifact: &StoredArtifact) -> Result<Vec<u8>, StoreError> {
        if self.down() {
            return Err(StoreError::Unavailable);
        }
        let shared = self.shared.borrow();
        let bytes = usize::try_from(artifact.version)
            .ok()
            .and_then(|v| shared.get(&artifact.key)?.get(v).cloned());
        match bytes {
            Some(mut b) if self.is(Break::CorruptsOnRead) => {
                b.push(0);
                Ok(b)
            }
            Some(b) => Ok(b),
            None if self.is(Break::InventsBytes) => Ok(Vec::new()),
            None => Err(StoreError::NotFound),
        }
    }

    fn lookup(
        &mut self,
        key: &ArtifactKey,
        digest: &Digest,
    ) -> Result<Option<StoredArtifact>, StoreError> {
        if self.down() {
            return Err(StoreError::Unavailable);
        }
        if self.is(Break::LookupMisses) {
            return Ok(None);
        }
        let shared = self.shared.borrow();
        let found = shared.get(key).and_then(|versions| {
            versions
                .iter()
                .enumerate()
                .find(|(_, b)| sha256(b) == *digest)
                .map(|(v, b)| (v, b.clone()))
        });
        drop(shared);
        Ok(found.map(|(v, b)| self.stored(key, v, &b)))
    }
}

struct Harness(Option<Break>);

impl ArtifactHarness for Harness {
    type Store = Client;

    fn store(&mut self) -> Client {
        Client {
            shared: Versions::default(),
            broken: self.0,
            available: Rc::new(RefCell::new(true)),
            fault: None,
        }
    }

    fn client(&mut self, store: &Client) -> Client {
        Client {
            shared: if store.is(Break::ClientLocal) {
                Versions::default()
            } else {
                Rc::clone(&store.shared)
            },
            broken: self.0,
            available: Rc::clone(&store.available),
            fault: None,
        }
    }

    fn fault_next_put(&mut self, store: &mut Client, fault: StoreFault) {
        store.fault = Some(fault);
    }

    fn set_available(&mut self, store: &mut Client, available: bool) {
        *store.available.borrow_mut() = available;
    }
}

#[test]
fn a_conforming_double_passes() {
    assert_eq!(run(&mut Harness(None)), Ok(()));
}

#[test]
fn each_broken_double_fails_its_rule() {
    let cases = [
        (Break::WrongDigest, ArtifactRule::PutDigest),
        (Break::CorruptsOnRead, ArtifactRule::RoundTrip),
        (Break::ClientLocal, ArtifactRule::IndependentRestore),
        (Break::Overwrites, ArtifactRule::VersionsImmutable),
        (Break::LookupMisses, ArtifactRule::LookupFinds),
        (Break::LookupMisses, ArtifactRule::LookupSettles),
        (Break::InventsBytes, ArtifactRule::NotFound),
        (Break::HidesLostAck, ArtifactRule::FaultIsUncertain),
        (
            Break::AnswersWhenUnavailable,
            ArtifactRule::UnavailableFailsClosed,
        ),
    ];
    for (broken, rule) in cases {
        assert_breaks(&run(&mut Harness(Some(broken))), &rule);
    }
}

#[test]
fn the_digest_is_sha256_of_the_bytes() {
    assert_eq!(
        sha256(b"").to_string(),
        "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}
