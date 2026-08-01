//! The compiled-in catalogue, as it appears on the wire.
//!
//! Pure data: every function here reads `connector_catalog` and returns a serialisable value. There
//! is no transport, no state and no principal, which is what makes the whole response contract
//! testable without binding a port.
//!
//! # Why the metadata is the point
//!
//! A [`Grant`](exchange_host::Grant) selects operations by their declared `risk`, `effects` and
//! `idempotency` rather than by name. If the catalogue is served without those three, a client can
//! see *which* operations exist but cannot predict which ones its own
//! [`Selector`](exchange_host::Selector) admits — and the grant model becomes something only the
//! server can evaluate. So [`OperationView`] carries an
//! [`OperationFacts`](exchange_host::OperationFacts) verbatim, flattened into the operation object:
//! the JSON a client reads *is* the value a `Selector` is evaluated over, and it deserialises back
//! into the same type the host decides with.
//!
//! # Three honesty rules this module keeps
//!
//! 1. **Nothing is enumerated here.** Connectors and operations come from `catalog::providers()`, so
//!    a connector added upstream is served the day the dependency moves, with no edit here.
//! 2. **A derived fact is never presented as a declared one** — see [`effects`].
//! 3. **Nothing is filtered.** The catalogue answers *what exists*; it is not a permission answer.
//!    See [`OperationView::admitted`].

use std::collections::BTreeSet;

use exchange_host::{Effect, Idempotency, OperationFacts, Risk};
use serde::Serialize;

// The dependency is keyed `connector-catalog` in the workspace manifest, so Cargo links the crate
// under *that* name and not under its own `catalog` lib name. The alias restores the vocabulary the
// crate documents itself in.
use connector_catalog as catalog;

/// The body of `GET /api/catalogue/connectors`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectorList {
    /// Every connector the catalogue carries, in the catalogue's own (id-sorted) order.
    pub connectors: Vec<ConnectorEntry>,
}

/// One connector in the listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectorEntry {
    /// The connector id, as the catalogue spells it — the segment
    /// `/api/catalogue/connectors/{id}/operations` takes.
    pub id: String,
    /// How many operations it publishes. A count, not the operations: the listing is a directory,
    /// and a client that wants the metadata asks for the connector it is interested in.
    pub operation_count: usize,
}

/// The body of `GET /api/catalogue/connectors/{id}/operations`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectorOperations {
    /// The connector these operations belong to, echoed so the body stands on its own.
    pub connector: String,
    /// Every operation the connector publishes. **Never filtered** — see
    /// [`OperationView::admitted`].
    pub operations: Vec<OperationView>,
}

/// One operation, with the metadata a `Selector` reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperationView {
    /// `id`, `risk`, `idempotency` and `effects`, flattened into this object.
    ///
    /// Flattened rather than nested, and reusing the host's own type rather than restating it:
    /// `{"id":…,"risk":…,"idempotency":…,"effects":[…]}` is exactly what
    /// [`OperationFacts`](exchange_host::OperationFacts) serialises to, so a client can deserialise
    /// the operation object straight back into one and evaluate its own `Selector` against it.
    ///
    /// One caveat, and it is the reason [`effects_derived`](Self::effects_derived) exists beside it:
    /// the field is documented upstream as the operation's *declared* effects, and the catalogue
    /// declares none. See [`effects`].
    #[serde(flatten)]
    pub facts: OperationFacts,
    /// The service this operation belongs to — `default` for a connector with one API surface.
    pub service: String,
    /// What the operation does, in one line: the same text a model sees as the tool description.
    pub description: String,
    /// **Always `true`, and it is not decoration.** `effects` above was inferred by this host, not
    /// declared by the connector. A client that treats an inferred effect as a declared one is
    /// trusting a guess; this field is how it can tell. See [`effects`].
    pub effects_derived: bool,
    /// Whether *this* principal may call the operation — and `null` is a third state, not a `false`.
    ///
    /// `null` means **no principal was resolved**, so the question was not asked: this is the
    /// catalogue, not your permissions. `false` would mean it was asked and answered no, which is a
    /// different fact and one nothing here can produce yet — there is no identity in this server
    /// until X-03 lands.
    ///
    /// It is `null` for every operation today, and it is *present* on every operation deliberately:
    /// an absent key would let a client conclude the catalogue had already been filtered for it.
    /// Nothing is ever omitted from [`ConnectorOperations::operations`] for want of a grant, because
    /// an agent that cannot see an operation it lacks cannot report that it was refused — it can
    /// only report that the operation does not exist, which is false.
    pub admitted: Option<bool>,
}

/// The body served when the connector id names nothing.
///
/// A `404` with this, never an empty `200`: "no such connector" and "a connector with no operations"
/// are different answers, and a client that cannot distinguish them cannot tell a typo from a gap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnknownConnector {
    /// A stable, machine-readable reason.
    pub error: &'static str,
    /// The id that was asked for, echoed back so the diagnostic names the thing that failed.
    pub connector: String,
}

impl UnknownConnector {
    /// Refuse the id that was asked for, naming it.
    pub fn new(connector: &str) -> Self {
        Self {
            error: "unknown connector",
            connector: connector.to_string(),
        }
    }
}

/// Every connector in the catalogue.
///
/// Read from `catalog::providers()` on every call rather than assembled once: the catalogue is
/// `&'static` data with no initialisation to amortise, and a cache would be a second copy of a
/// constant that could go stale against it.
pub fn connectors() -> ConnectorList {
    ConnectorList {
        connectors: catalog::providers()
            .iter()
            .map(|provider| ConnectorEntry {
                id: provider.id.to_string(),
                operation_count: provider.operations.len(),
            })
            .collect(),
    }
}

/// One connector's operations, or `None` when the catalogue carries no such connector.
///
/// Deliberately `catalog::provider(…)` and not `catalog::operations_of(…)`: the latter answers an
/// unknown id with an empty slice, which would turn "no such connector" into "a connector with
/// nothing in it" — the exact `404`-into-empty-`200` collapse the contract forbids.
pub fn connector_operations(connector: &str) -> Option<ConnectorOperations> {
    let provider = catalog::provider(catalog::ProviderKey::id(connector))?;

    Some(ConnectorOperations {
        // The catalogue's spelling, not the caller's string. They are equal today because the
        // lookup is exact, and sourcing it from the catalogue is what keeps that true if the
        // lookup ever stops being.
        connector: provider.id.to_string(),
        operations: provider.operations.iter().map(view).collect(),
    })
}

/// One catalogue operation, as the wire carries it.
fn view(operation: &catalog::Operation) -> OperationView {
    OperationView {
        facts: OperationFacts {
            id: operation.id.to_string(),
            risk: risk(operation.risk),
            idempotency: idempotency(operation.idempotency),
            effects: effects(operation),
        },
        service: operation.service.to_string(),
        description: operation.description.to_string(),
        // Every effect above was inferred by `effects`, and saying so is not optional.
        effects_derived: true,
        // No identity resolves a principal in this binary yet, so the question was never asked.
        admitted: None,
    }
}

/// The catalogue's risk vocabulary in the host's.
///
/// An exhaustive `match` rather than a `_ =>` arm, in both directions: the two enums are
/// independent mirrors of flux's vocabulary, and a variant added to either must be a compile error
/// here. A catch-all would answer a risk it had never heard of with a plausible wrong level, and a
/// `Selector::at_most` evaluated against that admits an operation nobody decided to admit.
fn risk(risk: catalog::Risk) -> Risk {
    match risk {
        catalog::Risk::Low => Risk::Low,
        catalog::Risk::Medium => Risk::Medium,
        catalog::Risk::High => Risk::High,
        catalog::Risk::Destructive => Risk::Destructive,
    }
}

/// The catalogue's idempotency vocabulary in the host's.
///
/// The two spell one variant differently — the catalogue's `NonIdempotent` is the host's
/// `NotIdempotent` — which is precisely why this is a `match` and not a string passed through.
/// Exhaustive for the reason [`risk`] is.
fn idempotency(idempotency: catalog::Idempotency) -> Idempotency {
    match idempotency {
        catalog::Idempotency::Idempotent => Idempotency::Idempotent,
        catalog::Idempotency::NonIdempotent => Idempotency::NotIdempotent,
        catalog::Idempotency::Conditional => Idempotency::Conditional,
    }
}

/// What an operation touches — **derived here, not declared upstream**.
///
/// `catalog::Operation` has no `effects` field (checked against 0.8.0): the catalogue declares
/// `risk`, `idempotency`, `credentials` and `hosts`, and nothing else that bears on this. A
/// `Selector` selects on effects, so serving the catalogue without them would leave a client unable
/// to evaluate one of the three axes it decides by — hence a derivation, and hence
/// [`OperationView::effects_derived`] marking every answer as one.
///
/// The rule is the most the source data supports and not one step further: **an operation the
/// catalogue gives a host to reach touches the network.** `workspace_write` and `process` are never
/// emitted, because nothing in the catalogue speaks to either — every operation it carries is an
/// HTTP call against a declared `base_url`. Inferring them from an operation's name or description
/// would be a guess wearing the clothes of a declaration, and a grant would then be decided against
/// fiction.
///
/// If the catalogue ever declares effects, this function is what is deleted, and `effects_derived`
/// becomes `false` rather than disappearing — a client that learned to check it should keep
/// getting an answer.
fn effects(operation: &catalog::Operation) -> BTreeSet<Effect> {
    let mut effects = BTreeSet::new();

    if !operation.hosts.is_empty() {
        effects.insert(Effect::Network);
    }

    effects
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::{Map, Value};

    /// Serialise `connector`'s operations and hand back the array, so a test asserts against the
    /// bytes a client actually receives rather than against the Rust value behind them.
    fn operation_objects(connector: &str) -> Vec<Map<String, Value>> {
        let response = connector_operations(connector)
            .unwrap_or_else(|| panic!("the catalogue carries `{connector}`"));
        let Value::Object(mut body) = serde_json::to_value(&response).expect("serialises") else {
            panic!("the response body is a JSON object");
        };
        let Some(Value::Array(operations)) = body.remove("operations") else {
            panic!("`operations` is a JSON array");
        };
        operations
            .into_iter()
            .map(|operation| match operation {
                Value::Object(fields) => fields,
                other => panic!("an operation is a JSON object, got {other}"),
            })
            .collect()
    }

    /// **The story's failing-first test.** Without these three a client can see which operations
    /// exist but cannot predict which ones its own `Selector` admits, and the grant model becomes
    /// server-only folklore.
    ///
    /// Asserted over the *whole* catalogue rather than one connector: 299 operations across 53
    /// connectors exercise every `risk` and every `idempotency` variant, so a mapping arm that is
    /// wrong for one value cannot hide behind a well-chosen example.
    #[test]
    fn every_operation_carries_risk_effects_and_idempotency() {
        const RISKS: [&str; 4] = ["low", "medium", "high", "destructive"];
        const IDEMPOTENCIES: [&str; 3] = ["idempotent", "not_idempotent", "conditional"];

        let mut seen = 0usize;

        for provider in catalog::providers() {
            let operations = operation_objects(provider.id);
            assert!(
                !operations.is_empty(),
                "connector `{}` served no operations",
                provider.id,
            );

            for operation in operations {
                let id = operation
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| panic!("an operation of `{}` has no id", provider.id));

                let risk = operation
                    .get("risk")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| panic!("`{id}` carries no risk"));
                assert!(RISKS.contains(&risk), "`{id}` has unknown risk `{risk}`");

                let idempotency = operation
                    .get("idempotency")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| panic!("`{id}` carries no idempotency"));
                assert!(
                    IDEMPOTENCIES.contains(&idempotency),
                    "`{id}` has unknown idempotency `{idempotency}`",
                );

                assert!(
                    operation.get("effects").and_then(Value::as_array).is_some(),
                    "`{id}` carries no effects",
                );

                seen += 1;
            }
        }

        assert_eq!(
            seen,
            catalog::operations().count(),
            "every catalogue operation must be served, and none twice",
        );
    }

    /// The route is a projection of the catalogue, never a list maintained beside it: adding a
    /// connector upstream must change nothing in this file.
    #[test]
    fn the_listing_is_the_catalogue_and_not_a_list_kept_here() {
        let expected: Vec<ConnectorEntry> = catalog::providers()
            .iter()
            .map(|provider| ConnectorEntry {
                id: provider.id.to_string(),
                operation_count: provider.operations.len(),
            })
            .collect();

        assert!(
            !expected.is_empty(),
            "an empty catalogue would vacuously pass"
        );
        assert_eq!(connectors().connectors, expected);
    }

    /// `404`, never an empty `200`. `catalog::operations_of` answers an unknown id with an empty
    /// slice, which is exactly the collapse this must not inherit.
    #[test]
    fn an_unknown_connector_is_none_rather_than_an_empty_listing() {
        const NO_SUCH_CONNECTOR: &str = "no-such-vendor";
        assert!(
            catalog::provider(catalog::ProviderKey::id(NO_SUCH_CONNECTOR)).is_none(),
            "the sentinel must not name a shipped connector",
        );

        assert_eq!(connector_operations(NO_SUCH_CONNECTOR), None);
        assert_eq!(
            serde_json::to_value(UnknownConnector::new(NO_SUCH_CONNECTOR)).expect("serialises"),
            serde_json::json!({ "error": "unknown connector", "connector": NO_SUCH_CONNECTOR }),
        );
    }

    /// The one spelling the two vocabularies disagree on. The catalogue says `non_idempotent` and
    /// the host says `not_idempotent`; the wire speaks the host's, because that is the token a
    /// `Selector` round-trips through.
    #[test]
    fn idempotency_is_spelled_the_hosts_way() {
        assert_eq!(
            catalog::Idempotency::NonIdempotent.as_str(),
            "non_idempotent"
        );

        let mapped = serde_json::to_value(idempotency(catalog::Idempotency::NonIdempotent))
            .expect("serialises");
        assert_eq!(mapped, Value::String("not_idempotent".into()));

        for (from, to) in [
            (catalog::Idempotency::Idempotent, "idempotent"),
            (catalog::Idempotency::NonIdempotent, "not_idempotent"),
            (catalog::Idempotency::Conditional, "conditional"),
        ] {
            assert_eq!(
                serde_json::to_value(idempotency(from)).expect("serialises"),
                Value::String(to.into()),
            );
        }
    }

    #[test]
    fn risk_keeps_the_catalogues_own_spelling() {
        for (from, to) in [
            (catalog::Risk::Low, "low"),
            (catalog::Risk::Medium, "medium"),
            (catalog::Risk::High, "high"),
            (catalog::Risk::Destructive, "destructive"),
        ] {
            assert_eq!(
                serde_json::to_value(risk(from)).expect("serialises"),
                Value::String(to.into()),
            );
            assert_eq!(from.as_str(), to, "and the catalogue agrees");
        }
    }

    /// The derivation, stated as a rule rather than as today's answer: an operation reaches the
    /// network exactly when the catalogue gives it a host to reach. Nothing in the catalogue speaks
    /// to `workspace_write` or `process`, so neither may ever appear — claiming an effect the source
    /// data cannot support is worse than omitting it.
    #[test]
    fn effects_are_derived_from_hosts_and_never_claim_more_than_that() {
        for operation in catalog::operations() {
            let derived = effects(operation);

            assert_eq!(
                derived.contains(&Effect::Network),
                !operation.hosts.is_empty(),
                "`{}` reaches {:?}; the derived effects were {derived:?}",
                operation.id,
                operation.hosts,
            );
            assert!(
                !derived.contains(&Effect::WorkspaceWrite) && !derived.contains(&Effect::Process),
                "`{}` claims an effect the catalogue cannot support: {derived:?}",
                operation.id,
            );
        }
    }

    /// A derived fact must never be readable as a declared one.
    #[test]
    fn every_operation_says_its_effects_were_derived() {
        let operations = operation_objects("zendesk");
        assert!(
            !operations.is_empty(),
            "an empty listing would vacuously pass"
        );

        for operation in operations {
            assert_eq!(
                operation.get("effects_derived"),
                Some(&Value::Bool(true)),
                "an operation served its effects without saying they were inferred",
            );
        }
    }

    /// `admitted` is `null` — not absent, not `false` — and nothing is filtered out for want of a
    /// grant. This is what exists; it is not a permission answer.
    #[test]
    fn admitted_is_null_and_nothing_is_filtered_by_grant() {
        for provider in catalog::providers() {
            let operations = operation_objects(provider.id);

            assert_eq!(
                operations.len(),
                provider.operations.len(),
                "connector `{}` served fewer operations than it carries",
                provider.id,
            );

            for operation in operations {
                assert_eq!(
                    operation.get("admitted"),
                    Some(&Value::Null),
                    "`admitted` must be present and null while no principal is resolved",
                );
            }
        }
    }

    /// The response contract, pinned against one operation the console is being written against.
    /// The key *set* is asserted exactly, so a field added or dropped here is a failure rather than
    /// a surprise downstream.
    #[test]
    fn the_wire_shape_of_an_operation_is_the_agreed_contract() {
        let response = connector_operations("zendesk").expect("the catalogue carries zendesk");
        assert_eq!(response.connector, "zendesk");

        let body = serde_json::to_value(&response).expect("serialises");
        let operation = body["operations"]
            .as_array()
            .expect("an array")
            .iter()
            .find(|operation| operation["id"] == "zendesk-ticket-show")
            .expect("the catalogue carries zendesk-ticket-show")
            .as_object()
            .expect("an object");

        let mut keys: Vec<&str> = operation.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "admitted",
                "description",
                "effects",
                "effects_derived",
                "id",
                "idempotency",
                "risk",
                "service",
            ],
        );

        assert_eq!(operation["id"], "zendesk-ticket-show");
        assert_eq!(operation["service"], "default");
        assert_eq!(operation["risk"], "low");
        assert_eq!(operation["idempotency"], "idempotent");
        assert_eq!(operation["effects"], serde_json::json!(["network"]));
        assert_eq!(operation["effects_derived"], true);
        assert_eq!(operation["admitted"], Value::Null);
        assert!(
            operation["description"]
                .as_str()
                .is_some_and(|d| !d.is_empty()),
            "the description is what a model reads; an empty one is a broken tool",
        );
    }

    /// The listing's own shape.
    #[test]
    fn the_wire_shape_of_the_listing_is_the_agreed_contract() {
        let body = serde_json::to_value(connectors()).expect("serialises");
        let listed = body["connectors"].as_array().expect("an array");

        let zendesk = listed
            .iter()
            .find(|entry| entry["id"] == "zendesk")
            .expect("the catalogue carries zendesk")
            .as_object()
            .expect("an object");

        let mut keys: Vec<&str> = zendesk.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["id", "operation_count"]);

        assert_eq!(
            zendesk["operation_count"],
            serde_json::json!(catalog::operations_of(catalog::ProviderKey::id("zendesk")).len()),
        );
    }

    /// `view` is the single mapping every served operation goes through, so it is asserted directly
    /// as well as through the response.
    #[test]
    fn a_view_carries_the_operations_own_metadata() {
        let show = catalog::operation(catalog::OperationKey::id("zendesk-ticket-show"))
            .expect("the catalogue carries zendesk-ticket-show");

        let view = view(show);

        assert_eq!(view.facts.id, show.id);
        assert_eq!(view.service, show.service);
        assert_eq!(view.description, show.description);
        assert_eq!(view.facts.risk, Risk::Low);
        assert_eq!(view.facts.idempotency, Idempotency::Idempotent);
        assert_eq!(view.facts.effects, BTreeSet::from([Effect::Network]));
        assert!(view.effects_derived);
        assert_eq!(view.admitted, None);
    }

    /// Every level of both vocabularies actually reaches the wire.
    ///
    /// The `admitted`/no-filtering tests above compare counts, which would still pass if the
    /// catalogue happened to be uniformly low-risk and idempotent. This one is the positive form:
    /// the `destructive` operations are *there*, in a response, exactly as `low` ones are — because
    /// an agent must be able to see the operation it is about to be refused.
    #[test]
    fn every_risk_and_idempotency_level_is_served() {
        let served: Vec<OperationView> = catalog::providers()
            .iter()
            .flat_map(|provider| {
                connector_operations(provider.id)
                    .expect("every listed connector answers")
                    .operations
            })
            .collect();

        for level in [Risk::Low, Risk::Medium, Risk::High, Risk::Destructive] {
            assert!(
                served.iter().any(|operation| operation.facts.risk == level),
                "no served operation carries risk {level:?}",
            );
        }

        for spelling in [
            Idempotency::Idempotent,
            Idempotency::NotIdempotent,
            Idempotency::Conditional,
        ] {
            assert!(
                served
                    .iter()
                    .any(|operation| operation.facts.idempotency == spelling),
                "no served operation carries idempotency {spelling:?}",
            );
        }
    }
}
