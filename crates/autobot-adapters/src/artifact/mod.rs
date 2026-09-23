//! The artifact store: independent, versioned, restorable custody of bytes outside the
//! worker-node loss domain (`docs/design/AUTOBOT-TRUST-MODEL.md` §Trusted, KERNEL §7).
//!
//! A custody checkpoint uploads its manifest and content through [`ArtifactStore::put`],
//! verifies the digest, and restores into a fresh location through [`ArtifactStore::get`] on
//! another client of the same store before it records `ArtifactCommit(VERIFIED)`. A lost upload
//! acknowledgement is [`StoreError::Uncertain`] and stays uncertain until
//! [`ArtifactStore::lookup`] or a restore settles it.
//!
//! Choices this module makes where the design is open:
//!
//! - An artifact is addressed by a key and a version the store assigns; a put under a key
//!   adds a version and never changes an earlier one.
//! - The store is trusted only for the bytes it stores: the digest it reports is checked
//!   against the bytes, never taken on its word ([`sha256`]).

mod contract;

pub use contract::{ArtifactHarness, ArtifactRule, StoreFault, run};

use crate::text::ArtifactKey;
use autobot_kernel::types::Digest;
use sha2::{Digest as _, Sha256};

/// One stored version of an artifact.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StoredArtifact {
    /// The artifact's key.
    pub key: ArtifactKey,
    /// The version the store assigned.
    pub version: u64,
    /// The SHA-256 of the stored bytes.
    pub digest: Digest,
}

/// Why a store call produced no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// The store is unavailable; nothing was stored or read.
    Unavailable,
    /// The upload's acknowledgement was lost: the bytes may or may not be stored.
    Uncertain,
    /// No such version is stored.
    NotFound,
}

/// The SHA-256 of `bytes`, the digest a [`StoredArtifact`] carries.
#[must_use]
pub fn sha256(bytes: &[u8]) -> Digest {
    Digest::from_bytes(Sha256::digest(bytes).into())
}

/// An artifact store client.
pub trait ArtifactStore {
    /// Stores `bytes` as a new version under `key`.
    ///
    /// # Errors
    ///
    /// [`StoreError::Unavailable`] when nothing was stored and [`StoreError::Uncertain`] when
    /// the acknowledgement was lost.
    fn put(&mut self, key: &ArtifactKey, bytes: &[u8]) -> Result<StoredArtifact, StoreError>;

    /// The bytes of `artifact`.
    ///
    /// # Errors
    ///
    /// [`StoreError::NotFound`] when that version is not stored and
    /// [`StoreError::Unavailable`] when the store cannot be read.
    fn get(&mut self, artifact: &StoredArtifact) -> Result<Vec<u8>, StoreError>;

    /// The stored version under `key` whose bytes have `digest`, if there is one.
    ///
    /// # Errors
    ///
    /// [`StoreError::Unavailable`] when the store cannot be read.
    fn lookup(
        &mut self,
        key: &ArtifactKey,
        digest: &Digest,
    ) -> Result<Option<StoredArtifact>, StoreError>;
}

#[cfg(test)]
mod tests;
