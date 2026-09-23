//! The authority witness contract suite.

use super::{AuthorityWitness, RestoreWitnessReceipt, RestoreWitnessRequest, WitnessError};
use crate::contract::{Checker, SuiteResult};
use crate::text::InstallationId;
use autobot_kernel::types::Digest;

/// What the witness suite needs to drive a witness client.
///
/// A fresh witness signs every well-formed request while it is available.
pub trait WitnessHarness {
    /// The witness client under test.
    type Witness: AuthorityWitness;

    /// A client of a fresh witness.
    fn witness(&mut self) -> Self::Witness;

    /// Makes the witness behind `witness` available or unavailable.
    fn set_available(&mut self, witness: &mut Self::Witness, available: bool);
}

/// The rules of the witness suite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WitnessRule {
    /// A receipt carries every field of its request unchanged.
    ReceiptAnswersRequest,
    /// A receipt the witness issued verifies.
    ReceiptVerifies,
    /// A receipt with any one field or its signature changed fails verification.
    TamperDetected,
    /// Each receipt has a higher witness generation than the one before it.
    GenerationAdvances,
    /// An unavailable witness issues no receipt and answers [`WitnessError::Unavailable`].
    UnavailableFailsClosed,
    /// A receipt verifies while the witness is unavailable.
    VerifiesWithoutService,
}

/// Runs the witness suite against the witnesses `harness` makes.
///
/// # Errors
///
/// Every [`WitnessRule`] the witnesses broke.
pub fn run<H: WitnessHarness>(harness: &mut H) -> SuiteResult<WitnessRule> {
    let mut c = Checker::new();
    // The literals are not empty, so the else branch is never taken.
    let (Ok(installation), Ok(other)) = (
        InstallationId::new("contract-installation-2"),
        InstallationId::new("contract-installation-3"),
    ) else {
        return c.finish();
    };
    let request = |restore_generation| RestoreWitnessRequest {
        installation_id: installation.clone(),
        restore_generation,
        old_installation_fence_evidence: Digest::from_bytes([0x91; 32]),
        ambiguous_operation_set_digest: Digest::from_bytes([0x92; 32]),
        identity_mapping_digest: Digest::from_bytes([0x93; 32]),
        old_grant_expiry: 1_700_000_600,
    };

    let mut witness = harness.witness();
    let mut receipts = Vec::new();
    for generation in [1, 2] {
        let req = request(generation);
        match witness.request_receipt(&req) {
            Ok(receipt) => {
                c.check(
                    receipt.answers(&req),
                    WitnessRule::ReceiptAnswersRequest,
                    || format!("{req:?} was answered by {receipt:?}"),
                );
                let verified = witness.verify(&receipt);
                c.check(verified.is_ok(), WitnessRule::ReceiptVerifies, || {
                    format!("an issued receipt failed verification with {verified:?}")
                });
                receipts.push(receipt);
            }
            Err(e) => c.fail(
                WitnessRule::ReceiptVerifies,
                format!("an available witness answered {req:?} with {e:?}"),
            ),
        }
    }
    if let [first, second] = receipts.as_slice() {
        c.check(
            second.witness_generation > first.witness_generation,
            WitnessRule::GenerationAdvances,
            || {
                format!(
                    "witness generation {} followed {}",
                    second.witness_generation, first.witness_generation
                )
            },
        );
    }

    if let Some(receipt) = receipts.first() {
        for (field, tampered) in tampered(receipt, &other) {
            let verified = witness.verify(&tampered);
            c.check(
                verified == Err(WitnessError::BadSignature),
                WitnessRule::TamperDetected,
                || format!("a receipt with its {field} changed verified as {verified:?}"),
            );
        }
    }

    harness.set_available(&mut witness, false);
    let refused = witness.request_receipt(&request(3));
    c.check(
        refused == Err(WitnessError::Unavailable),
        WitnessRule::UnavailableFailsClosed,
        || format!("an unavailable witness answered {refused:?}"),
    );
    if let Some(receipt) = receipts.first() {
        let verified = witness.verify(receipt);
        c.check(
            verified.is_ok(),
            WitnessRule::VerifiesWithoutService,
            || format!("with the witness unavailable a receipt verified as {verified:?}"),
        );
    }
    c.finish()
}

/// `receipt` with each field in turn changed, named by the field.
fn tampered(
    receipt: &RestoreWitnessReceipt,
    other: &InstallationId,
) -> Vec<(&'static str, RestoreWitnessReceipt)> {
    let flip = |d: Digest| {
        let mut bytes = *d.as_bytes();
        bytes[0] ^= 1;
        Digest::from_bytes(bytes)
    };
    let with = |f: &dyn Fn(&mut RestoreWitnessReceipt)| {
        let mut r = receipt.clone();
        f(&mut r);
        r
    };
    vec![
        (
            "installation_id",
            with(&|r| r.installation_id = other.clone()),
        ),
        ("restore_generation", with(&|r| r.restore_generation ^= 1)),
        ("witness_generation", with(&|r| r.witness_generation ^= 1)),
        (
            "old_installation_fence_evidence",
            with(&|r| r.old_installation_fence_evidence = flip(r.old_installation_fence_evidence)),
        ),
        (
            "ambiguous_operation_set_digest",
            with(&|r| r.ambiguous_operation_set_digest = flip(r.ambiguous_operation_set_digest)),
        ),
        (
            "identity_mapping_digest",
            with(&|r| r.identity_mapping_digest = flip(r.identity_mapping_digest)),
        ),
        ("old_grant_expiry", with(&|r| r.old_grant_expiry ^= 1)),
        (
            "signature",
            with(&|r| match r.signature.first_mut() {
                Some(b) => *b ^= 1,
                None => r.signature.push(1),
            }),
        ),
    ]
}
