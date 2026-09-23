use super::*;
use autobot_adapters::text::InstallationId;
use autobot_adapters::witness::{WitnessHarness, run};
use autobot_kernel::types::Digest;

/// Drives fresh fake witnesses through the adapter contract suite.
struct Harness;

impl WitnessHarness for Harness {
    type Witness = FakeWitness;

    fn witness(&mut self) -> FakeWitness {
        FakeWitness::new()
    }

    fn set_available(&mut self, witness: &mut FakeWitness, available: bool) {
        witness.set_mode(if available {
            WitnessMode::Present
        } else {
            WitnessMode::Absent
        });
    }
}

fn request(restore_generation: u64) -> RestoreWitnessRequest {
    RestoreWitnessRequest {
        installation_id: InstallationId::new("restored-installation")
            .expect("the literal is not empty"),
        restore_generation,
        old_installation_fence_evidence: Digest::from_bytes([0x11; 32]),
        ambiguous_operation_set_digest: Digest::from_bytes([0x22; 32]),
        identity_mapping_digest: Digest::from_bytes([0x33; 32]),
        old_grant_expiry: 1_800_000_000,
    }
}

#[test]
fn passes_the_witness_contract_suite() {
    assert_eq!(run(&mut Harness), Ok(()));
}

#[test]
fn a_present_witness_signs_receipts_that_answer_and_verify() {
    let mut witness = FakeWitness::new();
    let req = request(7);
    let receipt = witness
        .request_receipt(&req)
        .expect("a present witness signs");
    assert!(receipt.answers(&req));
    assert_eq!(receipt.witness_generation, 1);
    assert_eq!(
        receipt.signature,
        hmac_sha256(&TEST_KEY, &receipt.signed_bytes()).to_vec()
    );
    assert_eq!(witness.verify(&receipt), Ok(()));
    assert_eq!(witness.issued(), std::slice::from_ref(&receipt));
}

#[test]
fn a_changed_digest_or_signature_fails_verification() {
    let mut witness = FakeWitness::new();
    let receipt = witness.request_receipt(&request(1)).expect("present");

    let mut digest = receipt.clone();
    digest.ambiguous_operation_set_digest = Digest::from_bytes([0x23; 32]);
    assert_eq!(witness.verify(&digest), Err(WitnessError::BadSignature));

    let mut generation = receipt.clone();
    generation.witness_generation += 1;
    assert_eq!(witness.verify(&generation), Err(WitnessError::BadSignature));

    let mut signature = receipt.clone();
    signature.signature.truncate(31);
    assert_eq!(witness.verify(&signature), Err(WitnessError::BadSignature));

    let mut resigned = receipt;
    resigned.old_grant_expiry += 3600;
    resigned.signature = hmac_sha256(
        b"a key the code under test made up",
        &resigned.signed_bytes(),
    )
    .to_vec();
    assert_eq!(witness.verify(&resigned), Err(WitnessError::BadSignature));
}

#[test]
fn a_receipt_from_a_witness_with_another_key_fails_verification() {
    let mut other = FakeWitness::with_key([0x5a; 32]);
    let receipt = other.request_receipt(&request(1)).expect("present");
    assert_eq!(other.verify(&receipt), Ok(()));
    assert_eq!(
        FakeWitness::new().verify(&receipt),
        Err(WitnessError::BadSignature)
    );
}

#[test]
fn an_absent_witness_signs_nothing_and_still_verifies() {
    let mut witness = FakeWitness::new();
    let receipt = witness.request_receipt(&request(1)).expect("present");
    witness.set_mode(WitnessMode::Absent);
    for generation in 2..5 {
        assert_eq!(
            witness.request_receipt(&request(generation)),
            Err(WitnessError::Unavailable)
        );
    }
    assert_eq!(witness.mode(), WitnessMode::Absent);
    assert_eq!(witness.issued().len(), 1);
    assert_eq!(witness.verify(&receipt), Ok(()));
}

#[test]
fn a_late_witness_misses_its_requests_then_signs() {
    let mut witness = FakeWitness::new();
    witness.set_mode(WitnessMode::Late { misses: 2 });
    let req = request(4);
    assert_eq!(
        witness.request_receipt(&req),
        Err(WitnessError::Unavailable)
    );
    assert_eq!(witness.mode(), WitnessMode::Late { misses: 1 });
    assert_eq!(
        witness.request_receipt(&req),
        Err(WitnessError::Unavailable)
    );
    assert!(witness.issued().is_empty());

    let receipt = witness
        .request_receipt(&req)
        .expect("the late witness answers");
    assert_eq!(witness.mode(), WitnessMode::Present);
    assert!(receipt.answers(&req));
    assert_eq!(receipt.witness_generation, 1);
    assert_eq!(witness.verify(&receipt), Ok(()));

    let next = witness.request_receipt(&request(5)).expect("present");
    assert_eq!(next.witness_generation, 2);
}
