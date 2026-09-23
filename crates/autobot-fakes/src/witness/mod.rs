//! A fake external authority witness (`docs/design/AUTOBOT-KERNEL.md` §7).
//!
//! [`FakeWitness`] stands in for the witness's separate failure domain in tests: it holds a
//! test key, signs a [`RestoreWitnessReceipt`] for each [`RestoreWitnessRequest`] it answers
//! and verifies receipts with the same key. The code under test only calls
//! [`AuthorityWitness::request_receipt`] and [`AuthorityWitness::verify`] and records the
//! receipts; it never signs one. [`FakeWitness::issued`] is the ground truth of what the
//! witness signed.
//!
//! The witness answers in one of three [`WitnessMode`]s, the cases of G-RESTORE (M0 §3):
//!
//! - [`WitnessMode::Present`] signs every request.
//! - [`WitnessMode::Absent`] signs nothing and answers every request
//!   [`WitnessError::Unavailable`], so a restored installation stays read-only.
//! - [`WitnessMode::Late`] answers a set number of requests [`WitnessError::Unavailable`] and
//!   then signs, as [`WitnessMode::Present`].
//!
//! The receipt carries and signs only `ambiguous_operation_set_digest`, never the set itself,
//! as KERNEL §7 and FORMAL §2 state.
//!
//! Choices this module makes where the design is open:
//!
//! - The signature is HMAC-SHA256 (RFC 2104) over [`RestoreWitnessReceipt::signed_bytes`]
//!   under a 32-byte test key. A keyed hash needs no dependency beyond `sha2`, and a fake has no
//!   separate failure domain whose public key would need protecting: here the verifying key is
//!   the signing key, held only by the fake. A real witness, with an asymmetric scheme, arrives
//!   with the witness control plane at G-FENCE-CUSTODY (M0 §5).
//! - A late witness is late in answering, not in delivering: the trait is synchronous, so a
//!   late reply is modelled as the witness being unreachable for its first requests. A
//!   receipt it then signs carries the generation it signs it at, like any other.
//! - Verification never depends on the mode: a receipt verifies while the witness is absent.
//! - `witness_generation` starts at 1 and advances by one for each receipt signed.

use autobot_adapters::witness::{
    AuthorityWitness, RestoreWitnessReceipt, RestoreWitnessRequest, WitnessError,
};
use sha2::{Digest as _, Sha256};

/// The test key a [`FakeWitness::new`] signs with.
pub const TEST_KEY: [u8; 32] = *b"autobot fake witness test key 01";

/// How the fake witness answers requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WitnessMode {
    /// The witness signs every request.
    Present,
    /// The witness is unreachable and signs nothing.
    Absent,
    /// The witness answers the next `misses` requests [`WitnessError::Unavailable`] and then
    /// signs every request, as [`WitnessMode::Present`].
    Late {
        /// How many more requests go unanswered.
        misses: u32,
    },
}

/// A test-harness authority witness that signs restore receipts with a test key.
#[derive(Debug, Clone)]
pub struct FakeWitness {
    key: [u8; 32],
    mode: WitnessMode,
    generation: u64,
    issued: Vec<RestoreWitnessReceipt>,
}

impl FakeWitness {
    /// A present witness signing with [`TEST_KEY`].
    #[must_use]
    pub fn new() -> Self {
        Self::with_key(TEST_KEY)
    }

    /// A present witness signing with `key`; receipts another key signed fail to verify.
    #[must_use]
    pub fn with_key(key: [u8; 32]) -> Self {
        Self {
            key,
            mode: WitnessMode::Present,
            generation: 0,
            issued: Vec::new(),
        }
    }

    /// The witness's current mode.
    #[must_use]
    pub fn mode(&self) -> WitnessMode {
        self.mode
    }

    /// Makes the witness answer the following requests in `mode`.
    pub fn set_mode(&mut self, mode: WitnessMode) {
        self.mode = mode;
    }

    /// Every receipt the witness signed, in the order it signed them.
    #[must_use]
    pub fn issued(&self) -> &[RestoreWitnessReceipt] {
        &self.issued
    }

    /// The signature of `receipt` under this witness's key.
    fn sign(&self, receipt: &RestoreWitnessReceipt) -> Vec<u8> {
        hmac_sha256(&self.key, &receipt.signed_bytes()).to_vec()
    }
}

impl Default for FakeWitness {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthorityWitness for FakeWitness {
    fn request_receipt(
        &mut self,
        request: &RestoreWitnessRequest,
    ) -> Result<RestoreWitnessReceipt, WitnessError> {
        match self.mode {
            WitnessMode::Present => {}
            WitnessMode::Absent => return Err(WitnessError::Unavailable),
            WitnessMode::Late { misses: 0 } => self.mode = WitnessMode::Present,
            WitnessMode::Late { misses } => {
                self.mode = WitnessMode::Late { misses: misses - 1 };
                return Err(WitnessError::Unavailable);
            }
        }
        self.generation += 1;
        let mut receipt = RestoreWitnessReceipt {
            installation_id: request.installation_id.clone(),
            restore_generation: request.restore_generation,
            witness_generation: self.generation,
            old_installation_fence_evidence: request.old_installation_fence_evidence,
            ambiguous_operation_set_digest: request.ambiguous_operation_set_digest,
            identity_mapping_digest: request.identity_mapping_digest,
            old_grant_expiry: request.old_grant_expiry,
            signature: Vec::new(),
        };
        receipt.signature = self.sign(&receipt);
        self.issued.push(receipt.clone());
        Ok(receipt)
    }

    fn verify(&self, receipt: &RestoreWitnessReceipt) -> Result<(), WitnessError> {
        if self.sign(receipt) == receipt.signature {
            Ok(())
        } else {
            Err(WitnessError::BadSignature)
        }
    }
}

/// HMAC-SHA256 of `message` under `key` (RFC 2104, with SHA-256's 64-byte block).
fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut block = [0u8; BLOCK];
    if key.len() > BLOCK {
        let hashed: [u8; 32] = Sha256::digest(key).into();
        block[..32].copy_from_slice(&hashed);
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let inner: [u8; 32] = Sha256::new()
        .chain_update(block.map(|b| b ^ 0x36))
        .chain_update(message)
        .finalize()
        .into();
    Sha256::new()
        .chain_update(block.map(|b| b ^ 0x5c))
        .chain_update(inner)
        .finalize()
        .into()
}

#[cfg(test)]
mod tests;
