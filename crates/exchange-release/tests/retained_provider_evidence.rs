//! X-138 — the retained connector-secrets 0.20 recovery, replay and lease evidence is exact.
//!
//! X-139 owns the final authority document; this contract owns what that document may never lose.
//! Publication consumes exactly two Exchange-owned process proofs — production connect crash
//! recovery/replay and the retained `FileStore` process lease — plus the five applicable published
//! C-515 tests Exchange inherits rather than re-proves. Substituting or dropping any of them, or
//! carrying a process binding the retained-lease assertion does not join, must refuse rather than
//! read as a green report.

use flux_exchange_release::native_evidence::NativeEvidenceAuthority;

const SOURCE_COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

const EXCHANGE_PROCESS_EVIDENCE: [&str; 2] = [
    "real_server_retains_the_c515_lease_through_recovery_and_readiness",
    "unix_connect_crashes_recover_before_readiness_and_replay_one_receipt",
];

const INHERITED_PROVIDER_EVIDENCE: [&str; 5] = [
    "every_durable_transaction_boundary_recovers_one_complete_state",
    "file_store_holds_a_lifetime_non_blocking_lease",
    "native_upgrade_fixture_proves_legacy_quiescence_and_v2_refusal",
    "two_children_prove_lease_refusal_and_abrupt_release",
    "unix_lease_metadata_is_owner_only_one_link_and_never_repaired",
];

const LINUX_RUNNERS: [(&str, &str); 2] = [
    ("aarch64-unknown-linux-gnu", "ubuntu-24.04-arm"),
    ("x86_64-unknown-linux-gnu", "ubuntu-24.04"),
];

type AuthorityMutation = (&'static str, Box<dyn Fn(&mut NativeEvidenceAuthority)>);

#[test]
fn retained_provider_recovery_and_lease_evidence_is_exact_and_joined() {
    let authority = NativeEvidenceAuthority::bundled().expect("canonical native authority");

    // Both supported Linux runners project each named Exchange proof exactly once, and every
    // projected row is one terminal pass — never a skipped, filtered or gap row.
    for (target, runner) in LINUX_RUNNERS {
        let report = authority
            .report(target, SOURCE_COMMIT)
            .expect("target report");
        assert_eq!(report.runner, runner);
        assert!(
            report
                .bindings
                .iter()
                .all(|binding| binding.result == "one-passed-zero-ignored-zero-filtered"),
            "{target} projected a non-terminal binding row"
        );
        let runnable = authority
            .runnable_bindings(target)
            .expect("runnable bindings");
        for exact in EXCHANGE_PROCESS_EVIDENCE {
            assert_eq!(
                runnable
                    .iter()
                    .filter(|binding| binding.exact_test == exact)
                    .count(),
                1,
                "{target} does not select {exact} exactly once"
            );
        }
        for id in &report.inherited_release_evidence {
            assert!(
                authority.release_evidence.contains_key(id),
                "{target} inherited unknown release evidence {id}"
            );
        }
    }

    // Windows and macOS process rows are absent, not carried as skipped or optional evidence.
    for id in authority
        .bindings
        .keys()
        .chain(authority.obligations.keys())
        .chain(authority.target_sets.keys())
    {
        for absent in ["windows", "macos", "darwin", "msvc"] {
            assert!(
                !id.contains(absent),
                "authority retains a {absent} row: {id}"
            );
        }
    }
    let inherited = authority
        .release_evidence
        .values()
        .map(|evidence| {
            evidence
                .exact_test
                .rsplit("::")
                .next()
                .expect("nonempty exact test")
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        inherited,
        INHERITED_PROVIDER_EVIDENCE.into_iter().collect(),
        "the inherited C-515 evidence is not the exact applicable published test set"
    );

    let mut mutations: Vec<AuthorityMutation> = Vec::new();
    mutations.push((
        "retained lease process test",
        Box::new(|authority| {
            substitute_exact_test(
                authority,
                "real_server_retains_the_c515_lease_through_recovery_and_readiness",
            );
        }),
    ));
    mutations.push((
        "connect crash recovery/replay test",
        Box::new(|authority| {
            substitute_exact_test(
                authority,
                "unix_connect_crashes_recover_before_readiness_and_replay_one_receipt",
            );
        }),
    ));
    mutations.push((
        "inherited provider lease test",
        Box::new(|authority| {
            authority
                .release_evidence
                .values_mut()
                .find(|evidence| {
                    evidence.exact_test == "file_store_holds_a_lifetime_non_blocking_lease"
                })
                .expect("inherited lifetime-lease evidence")
                .exact_test = "file_store_holds_a_substituted_lease".to_owned();
        }),
    ));
    mutations.push((
        "retained lease obligation",
        Box::new(|authority| {
            authority
                .obligations
                .remove("x134-c515-retained-lease")
                .expect("retained lease obligation");
        }),
    ));
    mutations.push((
        "unjoined process binding class",
        Box::new(|authority| {
            authority
                .bindings
                .get_mut("x134-unix-recovery")
                .expect("connect recovery binding")
                .classes = vec!["unjoined-process".to_owned()];
        }),
    ));

    for (label, mutate) in mutations {
        let mut changed = authority.clone();
        mutate(&mut changed);
        assert!(
            changed.validate().is_err(),
            "{label} mutation retained publication authority"
        );
    }
}

fn substitute_exact_test(authority: &mut NativeEvidenceAuthority, exact: &str) {
    authority
        .bindings
        .values_mut()
        .find(|binding| binding.exact_test == exact)
        .unwrap_or_else(|| panic!("Exchange-owned binding for {exact}"))
        .exact_test = format!("{exact}_substituted");
}
