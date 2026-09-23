//! The artifact store contract suite.

use super::{ArtifactStore, StoreError, StoredArtifact, sha256};
use crate::contract::{Checker, SuiteResult};
use crate::text::ArtifactKey;

/// A fault an [`ArtifactHarness`] injects into the next put.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreFault {
    /// The upload never reaches the store.
    DroppedUpload,
    /// The store keeps the bytes and the acknowledgement is lost.
    LostAcknowledgement,
}

/// What the artifact suite needs to drive a store.
pub trait ArtifactHarness {
    /// The store client under test.
    type Store: ArtifactStore;

    /// A client of a fresh, empty store.
    fn store(&mut self) -> Self::Store;

    /// Another client of the store `store` talks to, sharing nothing with it but the store:
    /// what a restore into a fresh location reads through.
    fn client(&mut self, store: &Self::Store) -> Self::Store;

    /// Makes the next put through `store` end in `fault`.
    fn fault_next_put(&mut self, store: &mut Self::Store, fault: StoreFault);

    /// Makes the store behind `store` available or unavailable.
    fn set_available(&mut self, store: &mut Self::Store, available: bool);
}

/// The rules of the artifact suite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtifactRule {
    /// A put answers the key it was given and the SHA-256 of the bytes it was given.
    PutDigest,
    /// A get answers the bytes that were put.
    RoundTrip,
    /// Another client of the same store reads the same bytes.
    IndependentRestore,
    /// A second put under a key adds a version and leaves the first readable, unchanged.
    VersionsImmutable,
    /// A lookup finds a stored version by digest and finds nothing for bytes never stored.
    LookupFinds,
    /// A get of a version never stored answers [`StoreError::NotFound`].
    NotFound,
    /// A dropped upload or a lost acknowledgement answers [`StoreError::Uncertain`].
    FaultIsUncertain,
    /// After an uncertain put, a lookup tells whether the bytes were stored.
    LookupSettles,
    /// An unavailable store answers [`StoreError::Unavailable`] to every call.
    UnavailableFailsClosed,
}

/// Runs the artifact suite against the stores `harness` makes.
///
/// # Errors
///
/// Every [`ArtifactRule`] the stores broke.
pub fn run<H: ArtifactHarness>(harness: &mut H) -> SuiteResult<ArtifactRule> {
    let mut c = Checker::new();
    // The literal is not empty, so the else branch is never taken.
    let Ok(key) = ArtifactKey::new("contract-workspace/manifest") else {
        return c.finish();
    };
    stored(harness, &mut c, &key);
    for fault in [StoreFault::DroppedUpload, StoreFault::LostAcknowledgement] {
        faulted(harness, &mut c, &key, fault);
    }
    unavailable(harness, &mut c, &key);
    c.finish()
}

const FIRST: &[u8] = b"manifest version one";
const SECOND: &[u8] = b"manifest version two";

fn stored<H: ArtifactHarness>(harness: &mut H, c: &mut Checker<ArtifactRule>, key: &ArtifactKey) {
    let mut store = harness.store();
    let first = match store.put(key, FIRST) {
        Ok(first) => first,
        Err(e) => {
            c.fail(ArtifactRule::PutDigest, format!("a put failed with {e:?}"));
            return;
        }
    };
    c.check(
        first.key == *key && first.digest == sha256(FIRST),
        ArtifactRule::PutDigest,
        || format!("a put answered {first:?}"),
    );
    let got = store.get(&first);
    c.check(got.as_deref() == Ok(FIRST), ArtifactRule::RoundTrip, || {
        format!("a get answered {got:?}")
    });
    let mut other = harness.client(&store);
    let restored = other.get(&first);
    c.check(
        restored.as_deref() == Ok(FIRST),
        ArtifactRule::IndependentRestore,
        || format!("another client read {restored:?}"),
    );

    let second = store.put(key, SECOND);
    let versioned = second
        .as_ref()
        .is_ok_and(|s| s.version != first.version && s.digest == sha256(SECOND));
    c.check(versioned, ArtifactRule::VersionsImmutable, || {
        format!("a second put answered {second:?}")
    });
    let again = store.get(&first);
    c.check(
        again.as_deref() == Ok(FIRST),
        ArtifactRule::VersionsImmutable,
        || format!("the first version now reads {again:?}"),
    );

    let found = store.lookup(key, &first.digest);
    c.check(
        found.as_ref() == Ok(&Some(first.clone())),
        ArtifactRule::LookupFinds,
        || format!("a lookup of a stored digest answered {found:?}"),
    );
    let never = sha256(b"never stored");
    let missing = store.lookup(key, &never);
    c.check(missing == Ok(None), ArtifactRule::LookupFinds, || {
        format!("a lookup of a digest never stored answered {missing:?}")
    });
    let ghost = StoredArtifact {
        key: key.clone(),
        version: u64::MAX,
        digest: never,
    };
    let absent = store.get(&ghost);
    c.check(
        absent == Err(StoreError::NotFound),
        ArtifactRule::NotFound,
        || format!("a get of a version never stored answered {absent:?}"),
    );
}

fn faulted<H: ArtifactHarness>(
    harness: &mut H,
    c: &mut Checker<ArtifactRule>,
    key: &ArtifactKey,
    fault: StoreFault,
) {
    let mut store = harness.store();
    harness.fault_next_put(&mut store, fault);
    let put = store.put(key, FIRST);
    c.check(
        put == Err(StoreError::Uncertain),
        ArtifactRule::FaultIsUncertain,
        || format!("a put under {fault:?} answered {put:?}"),
    );
    let found = store.lookup(key, &sha256(FIRST));
    let settled = match (fault, &found) {
        (StoreFault::DroppedUpload, Ok(None)) => true,
        (StoreFault::LostAcknowledgement, Ok(Some(s))) => {
            s.digest == sha256(FIRST) && store.get(s).as_deref() == Ok(FIRST)
        }
        _ => false,
    };
    c.check(settled, ArtifactRule::LookupSettles, || {
        format!("after {fault:?} a lookup answered {found:?}")
    });
}

fn unavailable<H: ArtifactHarness>(
    harness: &mut H,
    c: &mut Checker<ArtifactRule>,
    key: &ArtifactKey,
) {
    let mut store = harness.store();
    let Ok(first) = store.put(key, FIRST) else {
        c.fail(
            ArtifactRule::PutDigest,
            "a put to an available store failed".to_owned(),
        );
        return;
    };
    harness.set_available(&mut store, false);
    let put = store.put(key, SECOND);
    let got = store.get(&first);
    let found = store.lookup(key, &first.digest);
    c.check(
        put == Err(StoreError::Unavailable)
            && got == Err(StoreError::Unavailable)
            && found == Err(StoreError::Unavailable),
        ArtifactRule::UnavailableFailsClosed,
        || format!("an unavailable store answered {put:?}, {got:?} and {found:?}"),
    );
}
