//! Decision 0001 is a repository contract, not a roadmap preference (X-124).
//!
//! The supported official-integration path has one execution placement: Exchange. These checks
//! deliberately pin the phrases that divide the first useful HTTP milestone from later runtime
//! lifecycle work. A story that quietly restores local Flux placement or makes streams and leases
//! prerequisites for one-shot invocation must fail in the ordinary workspace gate.

const EPIC: &str = include_str!("../../../docs/stories/X-111-rich-connector-runtimes-epic.md");
const ROADMAP_ALIGNMENT: &str =
    include_str!("../../../docs/stories/X-112-align-the-exchange-roadmap-with-rich-runtimes.md");
const HTTP_CONTRACT: &str =
    include_str!("../../../docs/stories/X-113-publish-the-remote-connector-protocol.md");
const DISPATCH: &str =
    include_str!("../../../docs/stories/X-114-dispatch-declared-runtime-plans.md");
const SINGLE_TENANT: &str =
    include_str!("../../../docs/stories/X-115-run-rich-runtimes-single-tenant.md");
const HOSTED_ISOLATION: &str =
    include_str!("../../../docs/stories/X-116-isolate-rich-runtimes-per-tenant.md");
const STREAMS: &str =
    include_str!("../../../docs/stories/X-117-stream-and-cancel-connector-operations.md");
const LEASES: &str =
    include_str!("../../../docs/stories/X-118-make-leases-own-runtime-resources.md");
const ARTIFACTS: &str =
    include_str!("../../../docs/stories/X-119-install-and-attest-runtime-artifacts.md");
const PROOF: &str = include_str!("../../../docs/stories/X-120-prove-rich-connectors-end-to-end.md");
const DESIGN: &str = include_str!("../../../docs/designs/rich-connector-runtimes.md");

fn require(document: &str, document_name: &str, claim: &str) {
    let document = document.split_whitespace().collect::<Vec<_>>().join(" ");
    let claim = claim.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        document.contains(&claim),
        "{document_name} must preserve the Decision 0001 contract phrase `{claim}`",
    );
}

fn forbid(document: &str, document_name: &str, claim: &str) {
    let document = document.split_whitespace().collect::<Vec<_>>().join(" ");
    let claim = claim.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        !document.contains(&claim),
        "{document_name} restores the superseded official-integration placement `{claim}`",
    );
}

/// Returns affirmative claims that give Flux an official integration execution path.
///
/// Required sentences alone are not a closed contract: a document could retain them and append a
/// contradictory fallback. This scanner therefore checks each sentence for Flux plus execution
/// vocabulary plus official integration scope. Its small negation list is deliberately phrased,
/// rather than accepting any sentence containing `no`: `Exchange is unavailable` in a fallback
/// claim must not accidentally look like a negation of Flux's execution.
fn second_placement_claims(document: &str) -> Vec<String> {
    let document = document
        .replace("\n## ", ". ## ")
        .replace("\n- ", ". - ")
        .replace('\n', " ");

    document
        .split(['.', '!', '?'])
        .map(|sentence| sentence.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|sentence| !sentence.is_empty())
        .filter(|sentence| {
            let sentence = sentence.to_ascii_lowercase();
            let names_flux = sentence
                .split(|character: char| {
                    !(character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
                })
                .any(|word| word == "flux");
            let words = sentence
                .split(|character: char| !character.is_ascii_alphanumeric())
                .filter(|word| !word.is_empty())
                .collect::<Vec<_>>();
            let claims_execution = words.iter().any(|word| {
                matches!(
                    *word,
                    "execute"
                        | "executes"
                        | "executing"
                        | "execution"
                        | "run"
                        | "runs"
                        | "invoke"
                        | "invokes"
                        | "invocation"
                        | "call"
                        | "calls"
                        | "host"
                        | "hosts"
                        | "fallback"
                )
            });
            let names_official_scope = words.iter().any(|word| {
                matches!(
                    *word,
                    "official"
                        | "integration"
                        | "integrations"
                        | "connector"
                        | "connectors"
                        | "vendor"
                        | "vendors"
                        | "plugin"
                        | "plugins"
                        | "placement"
                        | "fallback"
                )
            });
            let negates_flux_placement = [
                "there is no local flux",
                "no local flux",
                "not a second official",
                "no second official",
                "sole official-integration execution placement",
                "flux has no",
                "flux does not",
                "flux never",
                "never a flux",
                "never falls back to a local flux",
                "execute through exchange",
                "executes through exchange",
                "without any local integration fallback in flux",
                "no artifact becomes a flux",
                "without becoming a second",
            ]
            .iter()
            .any(|negation| sentence.contains(negation));
            let makes_exchange_optional = sentence.contains("exchange is optional")
                || sentence.contains("exchange remains optional");
            let falls_back_to_flux = sentence.contains("fallback to flux")
                || sentence.contains("fall back to flux")
                || sentence.contains("falls back to flux");

            ((names_flux && claims_execution) || makes_exchange_optional || falls_back_to_flux)
                && names_official_scope
                && !negates_flux_placement
        })
        .collect()
}

fn assert_no_second_placement(document: &str, document_name: &str) {
    let violations = second_placement_claims(document);
    assert!(
        violations.is_empty(),
        "{document_name} gives Flux a second official integration execution placement: {violations:?}",
    );
}

fn assert_exchange_only(document: &str, document_name: &str) {
    require(
        document,
        document_name,
        "Every official external integration executes through Exchange.",
    );
    require(
        document,
        document_name,
        "There is no local Flux execution placement or local vendor/plugin fallback.",
    );
    forbid(document, document_name, "local-first Flux");
    forbid(
        document,
        document_name,
        "same connector bundle Flux can execute locally",
    );
    assert_no_second_placement(document, document_name);
}

#[test]
fn the_placement_scanner_catches_what_it_claims_to() {
    for accepted in [
        "There is no local Flux execution placement or local vendor/plugin fallback.",
        "Flux contributes guarded runtime substrate, not a second official-integration execution placement.",
        "Flux has no vendor fallback.",
    ] {
        assert!(
            second_placement_claims(accepted).is_empty(),
            "the scanner rejected an Exchange-only contract: {accepted}",
        );
    }

    for rejected in [
        "Flux may also execute every official connector directly when Exchange is unavailable.",
        "Flux runs official vendor integrations locally.",
        "flux runs official vendor integrations locally.",
        "Flux hosts official external integrations.",
        "Official integrations execute locally in Flux.",
        "A local Flux execution placement is the fallback.",
        "The Flux binary can invoke vendor plugins without Exchange.",
        "Exchange is optional for official external integrations.",
        "Official integrations fall back to Flux.",
    ] {
        assert_eq!(
            second_placement_claims(rejected),
            [rejected.trim_end_matches('.').to_owned()],
            "the scanner accepted a second official integration placement",
        );

        let mutated = format!("{EPIC}\n\n{rejected}");
        assert!(
            std::panic::catch_unwind(|| assert_exchange_only(&mutated, "mutated X-111")).is_err(),
            "the complete Exchange-only assertion accepted this mutation: {rejected}",
        );

        let mutated_child = format!("{SINGLE_TENANT}\n\n{rejected}");
        assert!(
            std::panic::catch_unwind(|| {
                assert_no_second_placement(&mutated_child, "mutated X-115")
            })
            .is_err(),
            "the child-story assertion accepted this mutation: {rejected}",
        );
    }
}

#[test]
fn official_integrations_have_exactly_one_execution_placement() {
    for (name, document) in [("X-111", EPIC), ("rich-connector-runtimes design", DESIGN)] {
        assert_exchange_only(document, name);
    }

    for (name, document) in [
        ("X-111", EPIC),
        ("X-112", ROADMAP_ALIGNMENT),
        ("X-113", HTTP_CONTRACT),
        ("X-114", DISPATCH),
        ("X-115", SINGLE_TENANT),
        ("X-116", HOSTED_ISOLATION),
        ("X-117", STREAMS),
        ("X-118", LEASES),
        ("X-119", ARTIFACTS),
        ("X-120", PROOF),
        ("rich-connector-runtimes design", DESIGN),
    ] {
        assert_no_second_placement(document, name);
    }
}

#[test]
fn milestone_one_is_the_effective_catalogue_and_existing_http_invoke() {
    for claim in [
        "authenticated effective Service Account catalogue",
        "connected and granted operations",
        "stable generation identity",
        "existing one-shot HTTP invocation",
        "tenant and grants come from the resolved Service Account",
        "no credential or caller-selected authority",
        "Streams, cancellation and terminal outcomes remain X-117; leases remain X-118.",
    ] {
        require(HTTP_CONTRACT, "X-113", claim);
    }

    for superseded_scope in [
        "Publish the complete remote connector protocol",
        "one authenticated WebSocket covers events, streams, cancellation and lease liveness",
    ] {
        forbid(HTTP_CONTRACT, "X-113", superseded_scope);
    }
}

#[test]
fn exchange_installs_and_executes_connector_declared_runtime_plans() {
    for (name, document) in [
        ("X-114", DISPATCH),
        ("X-115", SINGLE_TENANT),
        ("X-119", ARTIFACTS),
    ] {
        require(
            document,
            name,
            "Flux contributes guarded runtime substrate, not a second official-integration execution placement.",
        );
    }

    require(
        DISPATCH,
        "X-114",
        "Exchange dispatches the connector-declared runtime plan",
    );
    require(
        SINGLE_TENANT,
        "X-115",
        "Exchange binds Flux's guarded substrate",
    );
    require(
        ARTIFACTS,
        "X-119",
        "Exchange installs and executes only attested connector runtime artifacts",
    );
}

#[test]
fn migration_proof_uses_local_single_tenant_exchange() {
    for claim in [
        "local single-tenant Exchange",
        "Hosted multi-tenant isolation remains X-116",
        "accumulated migration corpus",
    ] {
        require(PROOF, "X-120", claim);
    }

    forbid(PROOF, "X-120", "local and hosted placements");
    forbid(PROOF, "X-120", "local Flux");
}
