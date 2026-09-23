//! The authority witness: the external party that holds dispatch authority and signs the
//! `RestoreWitnessReceipt` a restored installation needs before it may dispatch again
//! (`docs/design/AUTOBOT-KERNEL.md` §7).
//!
//! AutoBot asks for a receipt through [`AuthorityWitness::request_receipt`] and checks one
//! through [`AuthorityWitness::verify`]. It never issues a receipt: the installation cannot
//! self-assert that its predecessor is fenced. When the witness is unavailable no receipt is
//! issued, and the restored installation stays read-only.
//!
//! Choices this module makes where the design is open:
//!
//! - The receipt's fields are those KERNEL §7 lists. FORMAL §2 also lists the
//!   `ambiguous_operation_set` itself beside its digest; the receipt carries the digest, which
//!   binds the set the installation holds.
//! - A signature covers [`RestoreWitnessReceipt::signed_bytes`], a fixed, length-prefixed
//!   encoding of every other field, so every witness implementation signs the same bytes.
//! - Verification needs only the witness's public key, never the witness service: a receipt
//!   can be checked while the witness is unavailable.
//! - The signature scheme belongs to the witness implementation; the signature is opaque
//!   bytes here.

mod contract;

pub use contract::{WitnessHarness, WitnessRule, run};

use crate::text::InstallationId;
use autobot_kernel::types::Digest;

/// What a restored installation asks the witness to sign.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RestoreWitnessRequest {
    /// The restored installation's new `installation_id`.
    pub installation_id: InstallationId,
    /// The restore's `restore_generation`.
    pub restore_generation: u64,
    /// The digest of the evidence that the old installation is fenced.
    pub old_installation_fence_evidence: Digest,
    /// The digest of the set of operations whose outcome the restore could not tell.
    pub ambiguous_operation_set_digest: Digest,
    /// The digest of the mapping from old identities to new ones.
    pub identity_mapping_digest: Digest,
    /// When the last grant of the old installation expires, in seconds since the Unix epoch.
    pub old_grant_expiry: u64,
}

/// A witness-signed restore receipt (KERNEL §7).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RestoreWitnessReceipt {
    /// The restored installation's `installation_id`.
    pub installation_id: InstallationId,
    /// The restore's `restore_generation`.
    pub restore_generation: u64,
    /// The witness's generation for this receipt, which becomes
    /// `dispatch_authority_generation`.
    pub witness_generation: u64,
    /// The digest of the evidence that the old installation is fenced.
    pub old_installation_fence_evidence: Digest,
    /// The digest of the ambiguous operation set.
    pub ambiguous_operation_set_digest: Digest,
    /// The digest of the identity mapping.
    pub identity_mapping_digest: Digest,
    /// When the last grant of the old installation expires, in seconds since the Unix epoch.
    pub old_grant_expiry: u64,
    /// The witness's signature over [`Self::signed_bytes`].
    pub signature: Vec<u8>,
}

impl RestoreWitnessReceipt {
    /// The bytes the signature covers: every field but the signature, in declaration order,
    /// text as a big-endian `u64` length followed by its UTF-8 bytes, integers as big-endian
    /// `u64`, digests as their 32 bytes.
    #[must_use]
    pub fn signed_bytes(&self) -> Vec<u8> {
        let id = self.installation_id.as_str().as_bytes();
        let mut out = Vec::with_capacity(8 + id.len() + 3 * 8 + 3 * 32);
        out.extend_from_slice(&(id.len() as u64).to_be_bytes());
        out.extend_from_slice(id);
        out.extend_from_slice(&self.restore_generation.to_be_bytes());
        out.extend_from_slice(&self.witness_generation.to_be_bytes());
        out.extend_from_slice(self.old_installation_fence_evidence.as_bytes());
        out.extend_from_slice(self.ambiguous_operation_set_digest.as_bytes());
        out.extend_from_slice(self.identity_mapping_digest.as_bytes());
        out.extend_from_slice(&self.old_grant_expiry.to_be_bytes());
        out
    }

    /// Whether the receipt answers `request`: every field the request names is the same.
    #[must_use]
    pub fn answers(&self, request: &RestoreWitnessRequest) -> bool {
        self.installation_id == request.installation_id
            && self.restore_generation == request.restore_generation
            && self.old_installation_fence_evidence == request.old_installation_fence_evidence
            && self.ambiguous_operation_set_digest == request.ambiguous_operation_set_digest
            && self.identity_mapping_digest == request.identity_mapping_digest
            && self.old_grant_expiry == request.old_grant_expiry
    }
}

/// Why the witness issued or accepted no receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WitnessError {
    /// The witness is unavailable; no receipt was issued.
    Unavailable,
    /// The witness refused to sign, with its reason.
    Refused(String),
    /// The receipt's signature does not verify.
    BadSignature,
}

/// An authority witness client.
pub trait AuthorityWitness {
    /// Asks the witness to sign a receipt for `request`.
    ///
    /// # Errors
    ///
    /// [`WitnessError::Unavailable`] when the witness cannot be reached and
    /// [`WitnessError::Refused`] when it will not sign.
    fn request_receipt(
        &mut self,
        request: &RestoreWitnessRequest,
    ) -> Result<RestoreWitnessReceipt, WitnessError>;

    /// Checks `receipt`'s signature with the witness's public key.
    ///
    /// # Errors
    ///
    /// [`WitnessError::BadSignature`] when it does not verify.
    fn verify(&self, receipt: &RestoreWitnessReceipt) -> Result<(), WitnessError>;
}

#[cfg(test)]
mod tests;
