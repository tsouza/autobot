//! An in-memory [`ArtifactStore`] with injectable faults, for unit and fixture tests.
//!
//! [`MemoryArtifactStore`] keeps every version of every key in memory shared by all of its
//! clients: [`MemoryArtifactStore::client`] makes another client of the same store, which is
//! what an independent restore reads through. A test injects the faults of the artifact
//! contract suite on one client ([`MemoryArtifactStore::fault_next_put`]) or on the whole store
//! ([`MemoryArtifactStore::set_available`]).
//!
//! Choices this module makes where the design is open, matching the S3-compatible adapter in
//! `autobot-controllers`:
//!
//! - The size bound is the profile's per-workspace artifact bound
//!   ([`Artifacts::workspace_max_gib`]), applied to each put. A put over it is refused before it
//!   reaches the store and answers [`StoreError::Unavailable`], the one answer the trait has for
//!   "nothing was stored"; the trait has no dedicated refusal.
//! - Versions are numbered from 0 per key, in the order the puts reached the store. Nothing
//!   removes a version.
//! - A fault is consumed by the next put through the client it was set on that reaches the
//!   store; a put refused as unavailable or oversize leaves it pending.

use autobot_adapters::artifact::{ArtifactStore, StoreError, StoreFault, StoredArtifact, sha256};
use autobot_adapters::text::ArtifactKey;
use autobot_kernel::profile::Artifacts;
use autobot_kernel::types::Digest;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// The contents and availability of one store, shared by its clients.
#[derive(Debug)]
struct Shared {
    versions: BTreeMap<ArtifactKey, Vec<Vec<u8>>>,
    available: bool,
}

/// A client of an in-memory artifact store.
#[derive(Debug)]
pub struct MemoryArtifactStore {
    shared: Arc<Mutex<Shared>>,
    fault: Option<StoreFault>,
    max_bytes: u64,
}

impl MemoryArtifactStore {
    /// A client of a fresh, empty, available store that accepts puts of at most `max_bytes`.
    #[must_use]
    pub fn new(max_bytes: u64) -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared {
                versions: BTreeMap::new(),
                available: true,
            })),
            fault: None,
            max_bytes,
        }
    }

    /// A client of a fresh store bounded by the profile's per-workspace artifact bound.
    #[must_use]
    pub fn from_profile(artifacts: &Artifacts) -> Self {
        Self::new(u64::from(artifacts.workspace_max_gib.get()) << 30)
    }

    /// Another client of this store, with no pending fault.
    #[must_use]
    pub fn client(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
            fault: None,
            max_bytes: self.max_bytes,
        }
    }

    /// The largest put this client accepts, in bytes.
    #[must_use]
    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    /// Makes the next put through this client that reaches the store end in `fault`.
    pub fn fault_next_put(&mut self, fault: StoreFault) {
        self.fault = Some(fault);
    }

    /// Makes the store available or unavailable to every client.
    pub fn set_available(&self, available: bool) {
        self.lock().available = available;
    }

    fn lock(&self) -> MutexGuard<'_, Shared> {
        // Every critical section leaves `Shared` consistent, so a poisoned lock is still usable.
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The store's contents when it is available.
    fn reach(&self) -> Result<MutexGuard<'_, Shared>, StoreError> {
        let shared = self.lock();
        if shared.available {
            Ok(shared)
        } else {
            Err(StoreError::Unavailable)
        }
    }
}

fn stored(key: &ArtifactKey, version: usize, bytes: &[u8]) -> Result<StoredArtifact, StoreError> {
    Ok(StoredArtifact {
        key: key.clone(),
        version: u64::try_from(version).map_err(|_| StoreError::Unavailable)?,
        digest: sha256(bytes),
    })
}

impl ArtifactStore for MemoryArtifactStore {
    fn put(&mut self, key: &ArtifactKey, bytes: &[u8]) -> Result<StoredArtifact, StoreError> {
        if u64::try_from(bytes.len()).map_or(true, |len| len > self.max_bytes) {
            return Err(StoreError::Unavailable);
        }
        let fault = self.fault;
        let mut shared = self.reach()?;
        if fault == Some(StoreFault::DroppedUpload) {
            drop(shared);
            self.fault = None;
            return Err(StoreError::Uncertain);
        }
        let versions = shared.versions.entry(key.clone()).or_default();
        versions.push(bytes.to_vec());
        let version = versions.len() - 1;
        drop(shared);
        if self.fault.take() == Some(StoreFault::LostAcknowledgement) {
            return Err(StoreError::Uncertain);
        }
        stored(key, version, bytes)
    }

    fn get(&mut self, artifact: &StoredArtifact) -> Result<Vec<u8>, StoreError> {
        let shared = self.reach()?;
        usize::try_from(artifact.version)
            .ok()
            .and_then(|v| shared.versions.get(&artifact.key)?.get(v))
            .filter(|bytes| sha256(bytes) == artifact.digest)
            .cloned()
            .ok_or(StoreError::NotFound)
    }

    fn lookup(
        &mut self,
        key: &ArtifactKey,
        digest: &Digest,
    ) -> Result<Option<StoredArtifact>, StoreError> {
        let shared = self.reach()?;
        let Some(versions) = shared.versions.get(key) else {
            return Ok(None);
        };
        versions
            .iter()
            .position(|bytes| sha256(bytes) == *digest)
            .map(|v| stored(key, v, &versions[v]))
            .transpose()
    }
}

#[cfg(test)]
mod tests;
