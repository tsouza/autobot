use super::*;
use crate::artifact::sha256;
use crate::contract::testing::assert_breaks;

/// One way a broken double departs from the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Break {
    AltersRequest,
    VerifiesWithAnotherKey,
    SignsOnlyTheInstallation,
    RepeatsGeneration,
    SignsWhenUnavailable,
    VerifyNeedsService,
}

/// A keyed-hash witness: enough to tell a changed receipt from an issued one.
struct Double {
    broken: Option<Break>,
    available: bool,
    generation: u64,
}

impl Double {
    fn is(&self, b: Break) -> bool {
        self.broken == Some(b)
    }

    fn sign(&self, receipt: &RestoreWitnessReceipt, key: &[u8]) -> Vec<u8> {
        let mut input = key.to_vec();
        if self.is(Break::SignsOnlyTheInstallation) {
            input.extend_from_slice(receipt.installation_id.as_str().as_bytes());
        } else {
            input.extend_from_slice(&receipt.signed_bytes());
        }
        sha256(&input).as_bytes().to_vec()
    }
}

impl AuthorityWitness for Double {
    fn request_receipt(
        &mut self,
        request: &RestoreWitnessRequest,
    ) -> Result<RestoreWitnessReceipt, WitnessError> {
        if !self.available && !self.is(Break::SignsWhenUnavailable) {
            return Err(WitnessError::Unavailable);
        }
        if !self.is(Break::RepeatsGeneration) {
            self.generation += 1;
        }
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
        if self.is(Break::AltersRequest) {
            receipt.old_grant_expiry += 3600;
        }
        receipt.signature = self.sign(&receipt, b"witness key");
        Ok(receipt)
    }

    fn verify(&self, receipt: &RestoreWitnessReceipt) -> Result<(), WitnessError> {
        if !self.available && self.is(Break::VerifyNeedsService) {
            return Err(WitnessError::Unavailable);
        }
        let key: &[u8] = if self.is(Break::VerifiesWithAnotherKey) {
            b"another key"
        } else {
            b"witness key"
        };
        if self.sign(receipt, key) == receipt.signature {
            Ok(())
        } else {
            Err(WitnessError::BadSignature)
        }
    }
}

struct Harness(Option<Break>);

impl WitnessHarness for Harness {
    type Witness = Double;

    fn witness(&mut self) -> Double {
        Double {
            broken: self.0,
            available: true,
            generation: 0,
        }
    }

    fn set_available(&mut self, witness: &mut Double, available: bool) {
        witness.available = available;
    }
}

#[test]
fn a_conforming_double_passes() {
    assert_eq!(run(&mut Harness(None)), Ok(()));
}

#[test]
fn each_broken_double_fails_its_rule() {
    let cases = [
        (Break::AltersRequest, WitnessRule::ReceiptAnswersRequest),
        (Break::VerifiesWithAnotherKey, WitnessRule::ReceiptVerifies),
        (Break::SignsOnlyTheInstallation, WitnessRule::TamperDetected),
        (Break::RepeatsGeneration, WitnessRule::GenerationAdvances),
        (
            Break::SignsWhenUnavailable,
            WitnessRule::UnavailableFailsClosed,
        ),
        (
            Break::VerifyNeedsService,
            WitnessRule::VerifiesWithoutService,
        ),
    ];
    for (broken, rule) in cases {
        assert_breaks(&run(&mut Harness(Some(broken))), &rule);
    }
}

#[test]
fn every_field_but_the_signature_is_signed() {
    let receipt = RestoreWitnessReceipt {
        installation_id: "i".parse().unwrap(),
        restore_generation: 1,
        witness_generation: 2,
        old_installation_fence_evidence: Digest::from_bytes([3; 32]),
        ambiguous_operation_set_digest: Digest::from_bytes([4; 32]),
        identity_mapping_digest: Digest::from_bytes([5; 32]),
        old_grant_expiry: 6,
        signature: vec![7],
    };
    let mut expected = Vec::new();
    expected.extend_from_slice(&1u64.to_be_bytes());
    expected.push(b'i');
    expected.extend_from_slice(&1u64.to_be_bytes());
    expected.extend_from_slice(&2u64.to_be_bytes());
    expected.extend_from_slice(&[3; 32]);
    expected.extend_from_slice(&[4; 32]);
    expected.extend_from_slice(&[5; 32]);
    expected.extend_from_slice(&6u64.to_be_bytes());
    assert_eq!(receipt.signed_bytes(), expected);

    let resigned = RestoreWitnessReceipt {
        signature: vec![8],
        ..receipt.clone()
    };
    assert_eq!(resigned.signed_bytes(), receipt.signed_bytes());
}
