#[path = "../src/credential_head.rs"]
mod credential_head;

use credential_head::{CredentialHeadError, CredentialHeadKey, CredentialHeadStore};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};

static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

struct ScratchRoot(PathBuf);

impl ScratchRoot {
    fn new(name: &str) -> Self {
        let suffix = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "flux-exchange-credential-head-{name}-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create scratch root");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchRoot {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove scratch root");
    }
}

fn key(label: &str) -> CredentialHeadKey {
    CredentialHeadKey::new("local", "github", label).expect("valid key")
}

fn image(root: &ScratchRoot) -> Vec<u8> {
    fs::read(root.path().join("credential-heads-v1/image.json")).expect("read durable image")
}

#[test]
fn legacy_migration_is_restart_stable_and_presence_independent() {
    let root = ScratchRoot::new("restart");
    let empty_credential_label = key("settings-only");
    let populated_credential_label = key("with-secret");

    // There deliberately is no presence argument: both held labels receive indistinguishable
    // opaque heads from the same complete migration image.
    let store = CredentialHeadStore::migrate_legacy(
        root.path(),
        &[
            empty_credential_label.clone(),
            populated_credential_label.clone(),
        ],
    )
    .expect("migrate legacy labels");
    let empty_head = store
        .current(&empty_credential_label)
        .expect("zero-secret label head");
    let populated_head = store
        .current(&populated_credential_label)
        .expect("populated label head");
    assert_ne!(empty_head, populated_head);
    drop(store);

    let reopened = CredentialHeadStore::open(root.path()).expect("open marked image");
    assert_eq!(
        reopened.current(&empty_credential_label).unwrap(),
        empty_head
    );
    assert_eq!(
        reopened.current(&populated_credential_label).unwrap(),
        populated_head
    );
}

#[test]
fn successive_advances_publish_distinct_heads() {
    let root = ScratchRoot::new("advance");
    let key = key("primary");
    let store =
        CredentialHeadStore::migrate_legacy(root.path(), std::slice::from_ref(&key)).unwrap();
    let initial = store.current(&key).unwrap();

    let next = store.allocate_next(&key).unwrap();
    store
        .compare_and_advance(&key, &initial, next.clone())
        .expect("first advance");
    let later = store.allocate_next(&key).unwrap();
    store
        .compare_and_advance(&key, &next, later.clone())
        .expect("second advance");

    assert_ne!(initial, next);
    assert_ne!(next, later);
    assert_ne!(initial, later);
    assert_eq!(store.current(&key).unwrap(), later);
}

#[test]
fn a_new_zero_secret_connection_receives_a_head() {
    let root = ScratchRoot::new("new-zero-secret");
    let store = CredentialHeadStore::migrate_legacy(root.path(), &[]).unwrap();
    let label = key("settings-only");

    // The store has no API through which this caller could supply secret needs or presence.
    let initial = store.allocate_new(&label).unwrap();
    store.insert_new(label.clone(), initial.clone()).unwrap();

    assert_eq!(store.current(&label).unwrap(), initial);
    assert!(matches!(
        store.allocate_new(&label),
        Err(CredentialHeadError::AlreadyExists)
    ));
}

#[test]
fn a_marked_store_refuses_missing_reset_and_corrupt_state() {
    let missing_root = ScratchRoot::new("unmigrated");
    assert!(matches!(
        CredentialHeadStore::open(missing_root.path()),
        Err(CredentialHeadError::Unmigrated)
    ));

    let missing_image_root = ScratchRoot::new("missing-image");
    let label = key("missing-image");
    CredentialHeadStore::migrate_legacy(missing_image_root.path(), &[label]).unwrap();
    fs::remove_file(
        missing_image_root
            .path()
            .join("credential-heads-v1/image.json"),
    )
    .unwrap();
    assert!(matches!(
        CredentialHeadStore::open(missing_image_root.path()),
        Err(CredentialHeadError::Corrupt)
    ));

    let reset_root = ScratchRoot::new("reset");
    let label = key("reset");
    CredentialHeadStore::migrate_legacy(reset_root.path(), &[label]).unwrap();
    let image = reset_root.path().join("credential-heads-v1/image.json");
    let bytes = fs::read(&image).unwrap();
    let current = CredentialHeadStore::open(reset_root.path())
        .unwrap()
        .current(&key("reset"))
        .unwrap();
    let reset = String::from_utf8(bytes)
        .unwrap()
        .replace(current.as_str(), &"0".repeat(64));
    fs::write(&image, reset).unwrap();
    assert!(matches!(
        CredentialHeadStore::open(reset_root.path()),
        Err(CredentialHeadError::Corrupt)
    ));

    let malformed_root = ScratchRoot::new("malformed");
    let label = key("malformed");
    CredentialHeadStore::migrate_legacy(malformed_root.path(), &[label]).unwrap();
    fs::write(
        malformed_root.path().join("credential-heads-v1/image.json"),
        b"not-json",
    )
    .unwrap();
    assert!(matches!(
        CredentialHeadStore::open(malformed_root.path()),
        Err(CredentialHeadError::Corrupt)
    ));
}

#[test]
fn concurrent_compare_and_advance_has_one_winner() {
    let root = ScratchRoot::new("concurrent");
    let key = key("primary");
    let store = Arc::new(
        CredentialHeadStore::migrate_legacy(root.path(), std::slice::from_ref(&key)).unwrap(),
    );
    let expected = store.current(&key).unwrap();
    let barrier = Arc::new(Barrier::new(3));

    let attempts = (0..2)
        .map(|_| {
            let store = Arc::clone(&store);
            let key = key.clone();
            let expected = expected.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let candidate = store.allocate_next(&key).unwrap();
                barrier.wait();
                store.compare_and_advance(&key, &expected, candidate)
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();

    let results = attempts
        .into_iter()
        .map(|attempt| attempt.join().expect("join contender"))
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(CredentialHeadError::CompareFailed)))
            .count(),
        1
    );
}

#[test]
fn accepted_advance_is_exactly_once_and_stale_replay_is_restart_safe() {
    let root = ScratchRoot::new("accepted-replay");
    let key = key("primary");
    let store =
        CredentialHeadStore::migrate_legacy(root.path(), std::slice::from_ref(&key)).unwrap();
    let initial = store.current(&key).unwrap();
    let accepted = store.allocate_next(&key).unwrap();

    store
        .compare_and_advance(&key, &initial, accepted.clone())
        .expect("accepted mutation advances exactly once");
    drop(store);

    let before_replay = image(&root);
    let reopened = CredentialHeadStore::open(root.path()).expect("restart opens accepted image");
    assert_eq!(reopened.current(&key).unwrap(), accepted);
    assert!(matches!(
        reopened.compare_and_advance(&key, &initial, accepted.clone()),
        Err(CredentialHeadError::CompareFailed | CredentialHeadError::InvalidCandidate)
    ));
    assert_eq!(reopened.current(&key).unwrap(), accepted);
    drop(reopened);

    assert_eq!(image(&root), before_replay);
    assert_eq!(
        CredentialHeadStore::open(root.path())
            .unwrap()
            .current(&key)
            .unwrap(),
        accepted
    );
}

#[test]
fn stale_expected_head_refuses_without_publishing_its_fresh_candidate() {
    let root = ScratchRoot::new("stale-candidate");
    let key = key("primary");
    let store =
        CredentialHeadStore::migrate_legacy(root.path(), std::slice::from_ref(&key)).unwrap();
    let stale = store.current(&key).unwrap();
    let accepted = store.allocate_next(&key).unwrap();
    store
        .compare_and_advance(&key, &stale, accepted.clone())
        .unwrap();
    let rejected = store.allocate_next(&key).unwrap();
    let before_replay = image(&root);

    assert!(matches!(
        store.compare_and_advance(&key, &stale, rejected.clone()),
        Err(CredentialHeadError::CompareFailed)
    ));
    assert_eq!(store.current(&key).unwrap(), accepted);
    drop(store);

    assert_eq!(image(&root), before_replay);
    let reopened = CredentialHeadStore::open(root.path()).unwrap();
    assert_eq!(reopened.current(&key).unwrap(), accepted);
    assert!(matches!(
        reopened.compare_and_advance(&key, &accepted, rejected),
        Ok(())
    ));
}

#[test]
fn retired_heads_remain_reserved_after_restart() {
    let root = ScratchRoot::new("retired");
    let key = key("primary");
    let store =
        CredentialHeadStore::migrate_legacy(root.path(), std::slice::from_ref(&key)).unwrap();
    let initial = store.current(&key).unwrap();
    let next = store.allocate_next(&key).unwrap();
    store
        .compare_and_advance(&key, &initial, next.clone())
        .unwrap();
    drop(store);

    let reopened = CredentialHeadStore::open(root.path()).unwrap();
    assert!(matches!(
        reopened.compare_and_advance(&key, &next, initial),
        Err(CredentialHeadError::InvalidCandidate)
    ));
    assert_eq!(reopened.current(&key).unwrap(), next);
}

#[test]
fn independent_owner_roots_never_share_heads_or_mutations() {
    let owner_a = ScratchRoot::new("owner-a");
    let owner_b = ScratchRoot::new("owner-b");
    let key = key("primary");
    let store_a =
        CredentialHeadStore::migrate_legacy(owner_a.path(), std::slice::from_ref(&key)).unwrap();
    let store_b =
        CredentialHeadStore::migrate_legacy(owner_b.path(), std::slice::from_ref(&key)).unwrap();
    let initial_a = store_a.current(&key).unwrap();
    let initial_b = store_b.current(&key).unwrap();
    assert_ne!(initial_a, initial_b);

    let next_a = store_a.allocate_next(&key).unwrap();
    store_a
        .compare_and_advance(&key, &initial_a, next_a.clone())
        .unwrap();
    drop(store_a);
    drop(store_b);

    assert_eq!(
        CredentialHeadStore::open(owner_a.path())
            .unwrap()
            .current(&key)
            .unwrap(),
        next_a
    );
    assert_eq!(
        CredentialHeadStore::open(owner_b.path())
            .unwrap()
            .current(&key)
            .unwrap(),
        initial_b
    );
}

#[test]
fn durable_head_image_has_only_value_free_identity_and_revision_state() {
    let root = ScratchRoot::new("value-free");
    let key = key("settings-only");
    let store = CredentialHeadStore::migrate_legacy(root.path(), &[]).unwrap();
    let head = store.allocate_new(&key).unwrap();
    store.insert_new(key, head).unwrap();
    drop(store);

    let bytes = image(&root);
    let document: serde_json::Value = serde_json::from_slice(&bytes).expect("canonical JSON image");
    let object = document.as_object().expect("image object");
    let mut members = object.keys().map(String::as_str).collect::<Vec<_>>();
    members.sort_unstable();
    assert_eq!(members, ["heads", "migration_complete", "schema"]);

    let entry = object["heads"].as_array().unwrap()[0].as_object().unwrap();
    let mut members = entry.keys().map(String::as_str).collect::<Vec<_>>();
    members.sort_unstable();
    assert_eq!(members, ["current", "key", "retired"]);
    assert_eq!(entry["retired"], serde_json::json!([]));

    let encoded = String::from_utf8(bytes).unwrap();
    for forbidden in [
        "secret",
        "password",
        "token",
        "credential_present",
        "credential_count",
        "created_at",
        "updated_at",
    ] {
        assert!(!encoded.contains(forbidden), "persisted {forbidden}");
    }
}
