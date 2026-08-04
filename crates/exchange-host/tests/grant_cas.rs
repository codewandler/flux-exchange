use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use exchange_host::{
    Effect, Grant, GrantApplyReceipt, GrantCandidate, GrantProposalDigest, GrantReceiptId,
    GrantSelector, GrantStore, GrantTransactionRefusal, GrantTransactions, Grants, Idempotency,
    InboundGrant, Risk, Selector, StoreRevision, Tenant,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "flux-exchange-x134-grant-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id(),
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir(&path).expect("create owner scratch");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .expect("owner-only scratch");
        }
        Self(path)
    }

    fn store(&self) -> PathBuf {
        self.0.join("grants")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tenant() -> Tenant {
    Tenant::new("local").expect("tenant")
}

fn selector(max_risk: Option<Risk>) -> GrantSelector {
    GrantSelector {
        effects_within: None,
        idempotency: None,
        max_risk,
    }
}

fn receipt(byte: u8) -> GrantReceiptId {
    GrantReceiptId::from_protocol_bytes([byte; 32]).expect("nonzero receipt")
}

fn write_legacy(path: &Path, grants: &[Grant]) {
    let document = serde_json::json!({ "local": grants });
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&document).expect("legacy JSON"),
    )
    .expect("legacy store");
    owner_only_file(path);
}

fn owner_only_file(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .expect("owner-only store");
    }
}

#[test]
fn grant_digest_uses_the_exact_domain_separated_canonical_preimage() {
    let candidate = GrantCandidate {
        connector: "asterisk".into(),
        inbound: vec![exchange_host::GrantCandidateInbound {
            binding: "ari-events".into(),
            events: BTreeSet::from([
                "channel-destroyed".to_string(),
                "channel-created".to_string(),
            ]),
        }],
        selector: GrantSelector {
            effects_within: Some(BTreeSet::from([Effect::Process, Effect::Network])),
            idempotency: Some(Idempotency::Idempotent),
            max_risk: Some(Risk::Low),
        },
    };
    let revision = StoreRevision::new(7).expect("nonzero revision");

    assert_eq!(
        String::from_utf8(
            candidate
                .proposal_input(&revision)
                .expect("canonical input")
        )
        .expect("UTF-8"),
        concat!(
            "{\"candidate\":{\"connector\":\"asterisk\",\"inbound\":[",
            "{\"binding\":\"ari-events\",\"events\":[\"channel-created\",",
            "\"channel-destroyed\"]}],\"selector\":{\"effects_within\":[",
            "\"network\",\"process\"],\"idempotency\":\"idempotent\",",
            "\"max_risk\":\"low\"}},\"revision\":\"7\"}"
        )
    );
    assert_eq!(
        candidate
            .proposal_digest(&revision)
            .expect("proposal digest")
            .to_string(),
        "f59e71d8ae1d2949ebcf24418b138d8be812294a4b6280f0e795ec3514747cb0"
    );
}

#[test]
fn legacy_initialization_apply_replay_query_and_restart_share_one_high_water_mark() {
    let scratch = Scratch::new("lifecycle");
    let path = scratch.store();
    let original = Grant::for_connector("github", Selector::at_most(Risk::Low));
    write_legacy(&path, std::slice::from_ref(&original));

    let store = GrantStore::bind(&path).expect("legacy store binds");
    let preview = store
        .preview(&tenant(), "github", selector(Some(Risk::High)))
        .expect("legacy migration and preview");
    assert_eq!(
        preview.revision,
        StoreRevision::new(1).expect("revision one")
    );

    let first = store
        .apply(
            &tenant(),
            &preview.candidate,
            preview.revision,
            preview.proposal_digest,
            receipt(1),
        )
        .expect("atomic CAS apply");
    assert_eq!(first.revision, StoreRevision::new(2).expect("revision two"));
    assert!(!first.replayed);

    let replay = store
        .apply(
            &tenant(),
            &preview.candidate,
            preview.revision,
            preview.proposal_digest,
            receipt(2),
        )
        .expect("same proposal replays");
    assert_eq!(
        replay,
        GrantApplyReceipt {
            replayed: true,
            ..first.clone()
        }
    );
    assert_eq!(
        store
            .query(&tenant(), first.receipt_id)
            .expect("query")
            .expect("known receipt"),
        replay
    );

    let reopened = GrantStore::bind(&path).expect("restart");
    let after_restart = reopened
        .preview(&tenant(), "github", selector(Some(Risk::High)))
        .expect("restart-stable preview");
    assert_eq!(
        after_restart.revision,
        StoreRevision::new(2).expect("revision two")
    );
    assert_eq!(
        reopened
            .query(&tenant(), first.receipt_id)
            .expect("query after restart")
            .expect("durable receipt"),
        replay
    );
}

#[test]
fn the_retained_whole_set_writer_advances_the_same_revision_once() {
    let scratch = Scratch::new("retained-writer-revision");
    let store = GrantStore::bind(scratch.store()).expect("store");
    let before = store
        .preview(&tenant(), "github", selector(Some(Risk::Low)))
        .expect("revision initialization");
    store
        .set(
            &tenant(),
            &[Grant::for_connector(
                "github",
                Selector::at_most(Risk::Medium),
            )],
        )
        .expect("retained writer");
    let after = store
        .preview(&tenant(), "github", selector(Some(Risk::High)))
        .expect("same revision space");
    assert_eq!(
        after.revision,
        before.revision.checked_next().expect("revision successor")
    );
}

#[test]
fn apply_refuses_stale_revision_and_changed_candidate_digest_before_write() {
    let scratch = Scratch::new("cas-refusals");
    let store = GrantStore::bind(scratch.store()).expect("store");
    let preview = store
        .preview(&tenant(), "github", selector(Some(Risk::Low)))
        .expect("preview");
    store
        .apply(
            &tenant(),
            &preview.candidate,
            preview.revision,
            preview.proposal_digest,
            receipt(3),
        )
        .expect("first apply");

    let stale = store
        .apply(
            &tenant(),
            &preview.candidate,
            preview.revision,
            preview.proposal_digest,
            receipt(4),
        )
        .expect("same digest is replay even after the head advances");
    assert!(stale.replayed);

    let mut changed = preview.candidate.clone();
    changed.selector.max_risk = Some(Risk::High);
    assert!(matches!(
        store.apply(
            &tenant(),
            &changed,
            preview.revision,
            preview.proposal_digest,
            receipt(5),
        ),
        Err(GrantTransactionRefusal::DigestMismatch)
    ));

    let changed_digest = changed
        .proposal_digest(&preview.revision)
        .expect("changed digest");
    assert!(matches!(
        store.apply(
            &tenant(),
            &changed,
            preview.revision,
            changed_digest,
            receipt(6),
        ),
        Err(GrantTransactionRefusal::Stale { .. })
    ));
}

#[test]
fn concurrent_compare_and_swap_admits_exactly_one_revision() {
    let scratch = Scratch::new("concurrent-cas");
    let store = std::sync::Arc::new(GrantStore::bind(scratch.store()).expect("store"));
    let low = store
        .preview(&tenant(), "github", selector(Some(Risk::Low)))
        .expect("low preview");
    let high = store
        .preview(&tenant(), "github", selector(Some(Risk::High)))
        .expect("high preview");
    assert_eq!(low.revision, high.revision);

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let joins = [(low, receipt(40)), (high, receipt(41))].map(|(preview, receipt_id)| {
        let store = store.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            store.apply(
                &tenant(),
                &preview.candidate,
                preview.revision,
                preview.proposal_digest,
                receipt_id,
            )
        })
    });
    barrier.wait();
    let results = joins.map(|join| join.join().expect("CAS worker"));
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(GrantTransactionRefusal::Stale { .. })))
            .count(),
        1
    );
}

#[test]
fn selected_projection_preserves_nonlexical_inbound_order_and_lexical_events() {
    let scratch = Scratch::new("order");
    let path = scratch.store();
    let mut selected = Grant::for_connector("slack", Selector::at_most(Risk::Low));
    selected.inbound = vec![
        InboundGrant {
            connector: "slack".into(),
            binding: "socket".into(),
            events: BTreeSet::from(["message".into(), "app_mention".into()]),
        },
        InboundGrant {
            connector: "slack".into(),
            binding: "events-api".into(),
            events: BTreeSet::from(["message".into()]),
        },
    ];
    write_legacy(&path, std::slice::from_ref(&selected));

    let store = GrantStore::bind(&path).expect("store");
    let preview = store
        .preview(&tenant(), "slack", selector(Some(Risk::High)))
        .expect("expressible projection");
    assert_eq!(
        preview
            .candidate
            .inbound
            .iter()
            .map(|entry| entry.binding.as_str())
            .collect::<Vec<_>>(),
        ["socket", "events-api"]
    );
    assert_eq!(
        preview.candidate.inbound[0]
            .events
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["app_mention", "message"]
    );
    store
        .apply(
            &tenant(),
            &preview.candidate,
            preview.revision,
            preview.proposal_digest,
            receipt(7),
        )
        .expect("apply");
    assert_eq!(store.held(&tenant())[0].inbound, selected.inbound);
}

#[test]
fn malformed_selected_authority_is_unexpressible_but_unrelated_rows_are_preserved() {
    let sixty_five_bindings: Vec<InboundGrant> = (0..65)
        .map(|index| InboundGrant {
            connector: "slack".into(),
            binding: format!("binding-{index}"),
            events: BTreeSet::from(["message".into()]),
        })
        .collect();
    let two_hundred_fifty_seven_events: BTreeSet<String> =
        (0..257).map(|index| format!("event-{index}")).collect();
    let bad_shapes = vec![
        {
            let mut grant = Grant::for_connector("slack", Selector::any().allow("manual-id"));
            grant.inbound = Vec::new();
            grant
        },
        {
            let mut grant = Grant::for_connector("slack", Selector::any().deny("manual-id"));
            grant.inbound = Vec::new();
            grant
        },
        {
            let mut grant = Grant::for_connector("slack", Selector::any());
            grant.inbound = vec![InboundGrant {
                connector: "slack".into(),
                binding: "socket".into(),
                events: BTreeSet::new(),
            }];
            grant
        },
        {
            let mut grant = Grant::for_connector("slack", Selector::any());
            grant.inbound = vec![InboundGrant {
                connector: "slack".into(),
                binding: "not-released".into(),
                events: BTreeSet::from(["message".into()]),
            }];
            grant
        },
        {
            let mut grant = Grant::for_connector("slack", Selector::any());
            grant.inbound = vec![InboundGrant {
                connector: "slack".into(),
                binding: "socket".into(),
                events: BTreeSet::from(["not-released".into()]),
            }];
            grant
        },
        {
            let mut grant = Grant::for_connector("slack", Selector::any());
            grant.inbound = sixty_five_bindings.clone();
            grant
        },
        {
            let mut grant = Grant::for_connector("slack", Selector::any());
            grant.inbound = vec![InboundGrant {
                connector: "slack".into(),
                binding: "socket".into(),
                events: two_hundred_fifty_seven_events.clone(),
            }];
            grant
        },
        {
            let repeated = InboundGrant {
                connector: "slack".into(),
                binding: "socket".into(),
                events: BTreeSet::from(["message".into()]),
            };
            let mut grant = Grant::for_connector("slack", Selector::any());
            grant.inbound = vec![repeated.clone(), repeated];
            grant
        },
        {
            let mut grant = Grant::for_connector("slack", Selector::any());
            grant.inbound = vec![InboundGrant {
                connector: "other".into(),
                binding: "socket".into(),
                events: BTreeSet::from(["message".into()]),
            }];
            grant
        },
    ];

    for (index, malformed) in bad_shapes.into_iter().enumerate() {
        let scratch = Scratch::new(&format!("malformed-{index}"));
        write_legacy(&scratch.store(), std::slice::from_ref(&malformed));
        let store = GrantStore::bind(scratch.store()).expect("typed malformed legacy store binds");
        assert!(matches!(
            store.preview(&tenant(), "slack", selector(Some(Risk::Low))),
            Err(GrantTransactionRefusal::Unexpressible)
        ));
        assert_eq!(store.held(&tenant()), vec![malformed]);
    }

    let scratch = Scratch::new("unrelated-preservation");
    let path = scratch.store();
    let selected = Grant::for_connector("github", Selector::at_most(Risk::Low));
    let unrelated = Grant {
        connector: "slack".into(),
        selector: Selector::any().deny("legacy-manual-deny"),
        inbound: vec![
            InboundGrant {
                connector: "wrong-but-preserved".into(),
                binding: "duplicate".into(),
                events: BTreeSet::new(),
            },
            InboundGrant {
                connector: "wrong-but-preserved".into(),
                binding: "duplicate".into(),
                events: BTreeSet::from(["unknown".into()]),
            },
        ],
    };
    let duplicate = Grant::for_connector("slack", Selector::any());
    let before = vec![unrelated.clone(), selected, duplicate.clone()];
    write_legacy(&path, &before);

    let store = GrantStore::bind(&path).expect("legacy store");
    let preview = store
        .preview(&tenant(), "github", selector(Some(Risk::High)))
        .expect("only the selected connector is projected");
    store
        .apply(
            &tenant(),
            &preview.candidate,
            preview.revision,
            preview.proposal_digest,
            receipt(8),
        )
        .expect("selected CAS");
    let after = store.held(&tenant());
    assert_eq!(after[0], unrelated);
    assert_eq!(after[2], duplicate);

    for (index, malformed) in [
        Grant {
            connector: "slack".into(),
            selector: Selector::any(),
            inbound: sixty_five_bindings,
        },
        Grant {
            connector: "slack".into(),
            selector: Selector::any(),
            inbound: vec![InboundGrant {
                connector: "slack".into(),
                binding: "socket".into(),
                events: two_hundred_fifty_seven_events,
            }],
        },
    ]
    .into_iter()
    .enumerate()
    {
        let scratch = Scratch::new(&format!("unrelated-overbound-{index}"));
        let before = vec![
            malformed.clone(),
            Grant::for_connector("github", Selector::at_most(Risk::Low)),
        ];
        write_legacy(&scratch.store(), &before);
        let store = GrantStore::bind(scratch.store()).expect("typed legacy store");
        let preview = store
            .preview(&tenant(), "github", selector(Some(Risk::High)))
            .expect("unrelated over-bound authority is opaque");
        store
            .apply(
                &tenant(),
                &preview.candidate,
                preview.revision,
                preview.proposal_digest,
                receipt(20 + index as u8),
            )
            .expect("selected replacement");
        assert_eq!(store.held(&tenant())[0], malformed);
    }

    let scratch = Scratch::new("legacy-omitted-inbound");
    let path = scratch.store();
    std::fs::write(
        &path,
        br#"{"local":[{"connector":"slack","selector":{"allow_ids":[],"deny_ids":[],"effects_within":null,"idempotency":null,"max_risk":"low"}},{"connector":"github","selector":{"allow_ids":[],"deny_ids":[],"effects_within":null,"idempotency":null,"max_risk":"low"},"inbound":[]}]}"#,
    )
    .expect("legacy omitted inbound");
    owner_only_file(&path);
    let store = GrantStore::bind(&path).expect("legacy omitted inbound decodes as empty");
    let preserved = store.held(&tenant())[0].clone();
    let preview = store
        .preview(&tenant(), "github", selector(Some(Risk::High)))
        .expect("selected preview");
    store
        .apply(
            &tenant(),
            &preview.candidate,
            preview.revision,
            preview.proposal_digest,
            receipt(30),
        )
        .expect("selected apply");
    assert_eq!(store.held(&tenant())[0], preserved);
    assert!(preserved.inbound.is_empty());
}

#[test]
fn duplicate_selected_connector_is_never_picked_or_merged() {
    let scratch = Scratch::new("duplicate-selected");
    let duplicates = vec![
        Grant::for_connector("github", Selector::at_most(Risk::Low)),
        Grant::for_connector("github", Selector::at_most(Risk::High)),
    ];
    write_legacy(&scratch.store(), &duplicates);
    let store = GrantStore::bind(scratch.store()).expect("typed legacy store");
    assert!(matches!(
        store.preview(&tenant(), "github", selector(None)),
        Err(GrantTransactionRefusal::Unexpressible)
    ));
    assert_eq!(store.held(&tenant()), duplicates);
}

#[test]
fn post_marker_missing_or_corrupt_revision_refuses_without_legacy_reset() {
    for revision in [
        serde_json::Value::Null,
        serde_json::json!("0"),
        serde_json::json!("01"),
    ] {
        let scratch = Scratch::new("corrupt-revision");
        let path = scratch.store();
        let document = serde_json::json!({
            "format": "exchange.grant-store.v1",
            "tenants": {
                "local": {
                    "grants": [],
                    "receipts": [],
                    "revision": revision
                }
            }
        });
        std::fs::write(&path, serde_json::to_vec_pretty(&document).expect("JSON"))
            .expect("corrupt versioned store");
        owner_only_file(&path);
        assert!(
            GrantStore::bind(&path).is_err(),
            "a marker forbids legacy initialization of {document}"
        );
    }

    let scratch = Scratch::new("missing-revision");
    let path = scratch.store();
    std::fs::write(
        &path,
        br#"{"format":"exchange.grant-store.v1","tenants":{"local":{"grants":[],"receipts":[]}}}"#,
    )
    .expect("missing revision store");
    owner_only_file(&path);
    assert!(GrantStore::bind(path).is_err());
}

#[test]
fn zero_receipt_and_revision_identities_refuse() {
    assert!(StoreRevision::new(0).is_none());
    assert!(GrantReceiptId::from_protocol_bytes([0; 32]).is_none());
    assert!(GrantProposalDigest::parse("00").is_none());
}
