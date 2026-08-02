//! **A named weakness in how a credential is obtained is a closed vocabulary** (X-73).
//!
//! This story has no behaviour. [`AuthHazard`] is the word [[X-74]]'s filter is written against, and
//! the whole of what can be got wrong here is spelling — so the tests are the deliverable, and they
//! are about the two ways a vocabulary stops being one.
//!
//! **A typo must not read as *no hazard declared*.** That is
//! [`an_unknown_hazard_spelling_refuses_at_deserialization`], and it is the reason this is an enum
//! and not a `hazard = "..."` string. A string makes the filter a string match: the near-miss
//! `"resource_owner_secret_sharing"` then matches no allow-list entry, carries no hazard, and is
//! admitted by a deployment that had explicitly refused the thing it names. A closed set makes the
//! same typo a refusal at load, which is the failure this vocabulary exists to convert.
//!
//! **A citation must stay checkable.** The variant's justification is three documents, and
//! [`the_doc_comment_carries_its_citations`] pins them to the file a reader is already in.
//!
//! It runs from outside the crate on purpose: `codewandler-flux-exchange-host` is published, so
//! "this type is reachable, and deserialises, from a consuming crate" is part of the deliverable
//! rather than an implementation detail. An inline `#[cfg(test)]` module could pass on a type no
//! consumer can name.

use std::collections::BTreeSet;

use exchange_host::{AuthHazard, Effect, Idempotency, OperationFacts, Risk, Selector};

/// **The failing-first test this story names.**
///
/// `"resource_owner_secret_sharing"` is one word away from the declared spelling and means the same
/// thing to a human reader, which is exactly what makes it dangerous: a vocabulary that shrugged at
/// it would hand [[X-74]]'s filter a connection carrying *no recognised hazard* and the filter would
/// admit it, on a deployment whose operator had refused that hazard by name. Nobody would have
/// decided to allow it.
///
/// So the assertion is that deserialisation **errs**, and the error names the offending spelling —
/// an operator who mistyped a hazard is debugging a typo, and a refusal that does not quote what it
/// read leaves them looking at the wrong line. There is no `#[serde(other)]` arm and no `Default`
/// to fall to, and this is the test that stops one being added.
#[test]
fn an_unknown_hazard_spelling_refuses_at_deserialization() {
    let refusal = serde_json::from_str::<AuthHazard>("\"resource_owner_secret_sharing\"")
        .expect_err("a near-miss spelling is not a hazard this vocabulary knows");

    let said = refusal.to_string();
    assert!(
        said.contains("unknown variant"),
        "the refusal should say the variant is unknown, said: {said}"
    );
    assert!(
        said.contains("resource_owner_secret_sharing"),
        "the refusal should quote the spelling it was given, said: {said}"
    );
}

/// The declared spelling round-trips, which is what keeps the test above from being vacuous.
///
/// A type that refused *everything* would satisfy the near-miss test and be useless. This pins the
/// serde representation as `snake_case` in both directions, so the wire word an operator writes and
/// the word a manifest declares are one string.
#[test]
fn the_declared_spelling_round_trips() {
    let json = serde_json::to_string(&AuthHazard::ResourceOwnerSecretShared)
        .expect("a fieldless variant serialises");
    assert_eq!(json, "\"resource_owner_secret_shared\"");

    let read: AuthHazard = serde_json::from_str(&json).expect("what we just wrote, read back");
    assert_eq!(read, AuthHazard::ResourceOwnerSecretShared);
}

/// **The citations stay in the file a reader is already looking at.**
///
/// The Acceptance is that a reviewer can check the claim without leaving the source, and a doc
/// comment is exactly the kind of thing that gets tidied by somebody who reads three RFC numbers as
/// clutter. RFC 9700 §2.4 is *why the grant is refused at all*, RFC 6749 §4.3 is *what the host owes
/// once it holds a token*, and CWE-522 is the entry an auditor's tooling will look for — losing any
/// one of them turns a citation into a coinage.
#[test]
fn the_doc_comment_carries_its_citations() {
    let source = include_str!("../src/acquisition.rs");

    for citation in ["RFC 9700", "2.4", "RFC 6749", "4.3", "CWE-522"] {
        assert!(
            source.contains(citation),
            "the hazard's doc comment must cite {citation} where a reviewer will read it"
        );
    }
}

/// **No existing grant's meaning moves** — the Acceptance item that is a statement as much as a
/// test.
///
/// `Risk` is an *ordered* ladder and [`Selector::at_most`] compares against it, so a hazard placed
/// on that ladder would be wrong in one direction or the other: a password grant buying a read-only
/// token is `Risk::Low` **and** hazardous, and a fifth rung high enough to catch it would drag every
/// destructive operation with it, while one low enough not to would be admitted by every
/// `at_most(Risk::High)` an operator has already written.
///
/// So `AuthHazard` is a separate type and this test says what stayed still. The two struct literals
/// are the load-bearing part rather than the assertion: they name **every** field of
/// [`OperationFacts`] and [`Selector`], so adding a hazard to either — the other tempting shortcut,
/// and the one that would restate one acquisition's property on 389 per-call rows — stops
/// compiling here.
#[test]
fn a_hazard_is_neither_a_rung_on_the_risk_ladder_nor_a_fact_about_an_operation() {
    let read_only = OperationFacts {
        id: "babelforce-call-list".to_owned(),
        risk: Risk::Low,
        idempotency: Idempotency::Idempotent,
        effects: BTreeSet::from([Effect::Network]),
    };

    let already_written = Selector {
        max_risk: Some(Risk::High),
        effects_within: None,
        idempotency: None,
        allow_ids: BTreeSet::new(),
        deny_ids: BTreeSet::new(),
    };

    assert_eq!(already_written, Selector::at_most(Risk::High));
    assert!(
        already_written.admits(&read_only),
        "a grant written before this story admits exactly what it admitted before it"
    );
}
