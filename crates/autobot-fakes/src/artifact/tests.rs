use super::*;
use autobot_adapters::artifact::{ArtifactHarness, run};
use autobot_kernel::profile::Profile;

struct Harness;

impl ArtifactHarness for Harness {
    type Store = MemoryArtifactStore;

    fn store(&mut self) -> MemoryArtifactStore {
        MemoryArtifactStore::new(1024)
    }

    fn client(&mut self, store: &MemoryArtifactStore) -> MemoryArtifactStore {
        store.client()
    }

    fn fault_next_put(&mut self, store: &mut MemoryArtifactStore, fault: StoreFault) {
        store.fault_next_put(fault);
    }

    fn set_available(&mut self, store: &mut MemoryArtifactStore, available: bool) {
        store.set_available(available);
    }
}

fn key(text: &str) -> ArtifactKey {
    ArtifactKey::new(text).unwrap()
}

#[test]
fn the_fake_passes_the_artifact_contract_suite() {
    assert_eq!(run(&mut Harness), Ok(()));
}

#[test]
fn the_bound_is_the_profiles_per_workspace_bound() {
    let profile = Profile::parse(include_str!("../../../../profiles/m0.toml")).unwrap();
    let artifacts = &profile.values().artifacts;
    let store = MemoryArtifactStore::from_profile(artifacts);
    assert_eq!(
        store.max_bytes(),
        u64::from(artifacts.workspace_max_gib.get()) * 1024 * 1024 * 1024
    );
    assert_eq!(store.client().max_bytes(), store.max_bytes());
}

#[test]
fn a_put_over_the_bound_stores_nothing() {
    let mut store = MemoryArtifactStore::new(4);
    let k = key("w/manifest");
    assert_eq!(store.put(&k, b"12345"), Err(StoreError::Unavailable));
    assert_eq!(store.lookup(&k, &sha256(b"12345")), Ok(None));
    let at_bound = store.put(&k, b"1234").unwrap();
    assert_eq!(at_bound.version, 0);
    assert_eq!(store.get(&at_bound).unwrap(), b"1234");
}

#[test]
fn a_fault_applies_to_its_own_client_once() {
    let mut store = MemoryArtifactStore::new(64);
    let mut other = store.client();
    let k = key("w/manifest");
    store.fault_next_put(StoreFault::DroppedUpload);
    let theirs = other.put(&k, b"other").unwrap();
    assert_eq!(theirs.version, 0);
    assert_eq!(store.put(&k, b"dropped"), Err(StoreError::Uncertain));
    assert_eq!(other.lookup(&k, &sha256(b"dropped")), Ok(None));
    let next = store.put(&k, b"kept").unwrap();
    assert_eq!(next.version, 1);
    assert_eq!(other.get(&next).unwrap(), b"kept");
}

#[test]
fn a_fault_waits_out_an_unavailable_store() {
    let mut store = MemoryArtifactStore::new(64);
    let k = key("w/manifest");
    store.fault_next_put(StoreFault::LostAcknowledgement);
    store.set_available(false);
    assert_eq!(store.put(&k, b"x"), Err(StoreError::Unavailable));
    store.set_available(true);
    assert_eq!(store.put(&k, b"x"), Err(StoreError::Uncertain));
    let settled = store.lookup(&k, &sha256(b"x")).unwrap().unwrap();
    assert_eq!(settled.version, 0);
    assert_eq!(store.put(&k, b"y").unwrap().version, 1);
}

#[test]
fn a_get_names_the_version_and_its_digest() {
    let mut store = MemoryArtifactStore::new(64);
    let k = key("w/manifest");
    let first = store.put(&k, b"one").unwrap();
    let wrong_digest = StoredArtifact {
        digest: sha256(b"two"),
        ..first.clone()
    };
    assert_eq!(store.get(&wrong_digest), Err(StoreError::NotFound));
    let other_key = StoredArtifact {
        key: key("w/other"),
        ..first
    };
    assert_eq!(store.get(&other_key), Err(StoreError::NotFound));
}

#[test]
fn a_lookup_answers_the_first_version_with_the_digest() {
    let mut store = MemoryArtifactStore::new(64);
    let k = key("w/manifest");
    store.put(&k, b"a").unwrap();
    store.put(&k, b"b").unwrap();
    store.put(&k, b"b").unwrap();
    let found = store.lookup(&k, &sha256(b"b")).unwrap().unwrap();
    assert_eq!(found.version, 1);
    assert_eq!(store.lookup(&key("w/none"), &sha256(b"b")), Ok(None));
}
