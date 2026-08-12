//! **A connection that verified still verifies, and one that did not still does not** (X-152).
//!
//! The connection-verification path is the one place in this crate where the connector's own
//! derivation decides an *operator's* answer rather than a developer's. `SettingsStore::bind`
//! builds its custom-origin policy by rehearsing each connector's declared verification operation
//! and asking whether that operation actually consumes the operator-approved origin field; a
//! proposal is then admitted or refused by rehearsing the same operation over the candidate value.
//! A silent difference there does not look like our bug. It looks like the operator typed the
//! origin wrong.
//!
//! So the pair below is pinned from **outside** the crate, through the public
//! [`ConnectionSettings`] surface, and it names no derivation at all — neither `Rehearsal` nor
//! `DocumentRehearsal`. It is written against the Flux-parsing derivation and must pass byte for
//! byte unchanged once the verification site reads the catalogue document instead. That is the
//! whole of what "the replacement is mechanical" means, expressed as something that can fail.
//!
//! GitLab is the subject because it is the only connector in this catalogue that declares an
//! `Approval::Operator` origin — [`only_gitlab_declares_an_operator_approved_origin`] holds that
//! to the catalogue rather than to this sentence, so a second one arriving is a red test here
//! rather than an untested path.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use exchange_host::{ConnectionSettings, DeclaredSetting, SettingsRefusal, SettingsStore, Tenant};

/// The tenant every proposal is made for. Which one it is does not matter; that it is a validated
/// [`Tenant`] rather than a caller-supplied string is the invariant, and it is asserted elsewhere.
const TENANT: &str = "acme";

/// An origin an operator could legitimately be running GitLab on, port and all.
const VERIFIES: &str = "https://gitlab.internal.example:8443";

/// The same host over plaintext. The connector's verification operation cannot compose a request
/// from it, and this refusal is the one an operator sees.
const DOES_NOT_VERIFY: &str = "http://gitlab.internal.example";

// ---------------------------------------------------------------------------------------------
// The composition
// ---------------------------------------------------------------------------------------------

/// A scratch directory under the system temporary directory, removed on drop.
///
/// The same shape `connection_settings.rs` uses, and for the same reason: `SettingsStore::bind`
/// refuses a path inside a working tree, so a store cannot live under this checkout.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "exchange-host-verification-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("a scratch directory");
        Self(path.canonicalize().expect("a resolvable scratch directory"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The **production** settings store: policy derived from the released catalogue, not a test double.
///
/// `SettingsStore::bind` is the constructor that reads the catalogue; the unit-test seam that takes
/// a policy is deliberately not used here, because the derivation is the thing under test.
fn production_store(label: &str) -> (Scratch, SettingsStore) {
    let scratch = Scratch::new(label);
    let store = SettingsStore::bind(scratch.0.join("state").join("settings"))
        .expect("the released catalogue yields a readable custom-origin policy");
    (scratch, store)
}

fn tenant() -> Tenant {
    Tenant::new(TENANT).expect("`acme` is a usable tenant")
}

/// GitLab's operator-approved origin declaration, by its `binds` spelling.
fn gitlab_origin() -> DeclaredSetting {
    DeclaredSetting::parse("default", "endpoint.origin")
        .expect("`endpoint.origin` is a binds target")
}

// ---------------------------------------------------------------------------------------------
// The pair
// ---------------------------------------------------------------------------------------------

/// **The connector this catalogue asks an operator to approve an origin for**, counted.
///
/// The pair below is one connector's, and that is only sound while one connector declares one. A
/// second `Approval::Operator` field arriving without a test is a verification path nothing covers,
/// so this fails rather than the coverage quietly narrowing.
#[test]
fn only_gitlab_declares_an_operator_approved_origin() {
    let declaring: Vec<&str> = connector_catalog::providers()
        .iter()
        .filter(|provider| {
            provider
                .config
                .iter()
                .any(|field| matches!(field.approval, connector_catalog::Approval::Operator))
        })
        .map(|provider| provider.id)
        .collect();

    assert_eq!(
        declaring,
        vec!["gitlab"],
        "the operator-approved origin surface is no longer one connector's; every connector listed \
         here has a verification path, and this file covers one of them",
    );
}

/// **The released catalogue is what decides this is a custom origin**, not a list written here.
#[test]
fn the_released_catalogue_marks_the_gitlab_origin_as_operator_approved() {
    let (_scratch, store) = production_store("is-custom");

    assert!(
        store.is_custom_origin("gitlab", &gitlab_origin()),
        "the production policy no longer recognises GitLab's origin as operator-approved, so every \
         proposal below is being answered by the default policy rather than by the connector's own \
         verification operation",
    );
}

/// **A connection that verified before still verifies.**
///
/// The proposal is admitted because GitLab's declared verification operation composes a request
/// from this origin. Nothing here asserts the URL — that is the pack's answer and
/// `settings.rs`'s own unit test pins it — only that the operator's value was accepted, which is
/// what the operator sees.
#[test]
fn an_origin_the_verification_operation_composes_is_admitted() {
    let (_scratch, store) = production_store("admits");

    let status = store
        .propose_authority_for_instance(&tenant(), "gitlab", None, &gitlab_origin(), VERIFIES, None)
        .expect("the verification operation composes a request from this origin");

    assert!(
        status.revision.is_some(),
        "an admitted proposal carries the revision an approval compare-and-swaps against",
    );
}

/// **A connection that did not verify still does not**, and refuses with the same distinction.
///
/// Two refusals rather than one, because they are not the same event to an operator:
/// `OriginSchemeUnsupported` says *this deployment will not talk plaintext to your GitLab*, and
/// `MalformedOrigin` says *that is not an origin*. Collapsing them would be a regression this pair
/// would not otherwise catch.
#[test]
fn an_origin_the_verification_operation_refuses_is_not_admitted() {
    let (_scratch, store) = production_store("refuses");

    let plaintext = store
        .propose_authority_for_instance(
            &tenant(),
            "gitlab",
            None,
            &gitlab_origin(),
            DOES_NOT_VERIFY,
            None,
        )
        .expect_err("a plaintext origin does not verify");
    assert!(
        matches!(plaintext, SettingsRefusal::OriginSchemeUnsupported { .. }),
        "a plaintext origin was refused as something other than an unsupported scheme: {plaintext}",
    );

    let malformed = store
        .propose_authority_for_instance(
            &tenant(),
            "gitlab",
            None,
            &gitlab_origin(),
            "https://gitlab.internal.example/api/v4",
            None,
        )
        .expect_err("an origin carrying a path does not verify");
    assert!(
        matches!(malformed, SettingsRefusal::MalformedOrigin { .. }),
        "an origin carrying a path was refused as something other than malformed: {malformed}",
    );
}

/// **An admitted origin becomes the tenant's value only after approval**, and then resolves exactly.
///
/// The whole lifecycle in one pass, because "still verifies" is a claim about the end state rather
/// than about the first call: a proposal that was admitted and then could not be approved is a
/// connection that does not work, reported as one that does.
#[test]
fn an_approved_origin_is_what_the_tenant_resolves() {
    let (_scratch, store) = production_store("approves");
    let tenant = tenant();
    let declared = gitlab_origin();

    let proposal = store
        .propose_authority_for_instance(&tenant, "gitlab", None, &declared, VERIFIES, None)
        .expect("the origin verifies");
    let revision = proposal
        .revision
        .expect("an admitted proposal has a revision");

    assert!(
        !store.is_set(&tenant, "gitlab", &declared),
        "a proposal is not yet a value; an unapproved origin that resolved would be an operator \
         approval that decides nothing",
    );

    store
        .approve_authority_for_instance(&tenant, "gitlab", None, &declared, revision)
        .expect("the exact proposed revision is approvable");

    assert!(
        store.is_set(&tenant, "gitlab", &declared),
        "an approved origin is not the tenant's value",
    );
}
