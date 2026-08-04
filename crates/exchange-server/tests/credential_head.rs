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
