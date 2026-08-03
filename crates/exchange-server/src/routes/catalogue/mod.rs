//! The connector catalogue, served.
//!
//! `GET /api/catalogue/connectors` lists what this binary was compiled with,
//! `GET /api/catalogue/connectors/{id}/operations` returns one connector's operations with the
//! `risk`, `effects` and `idempotency` a [`Selector`](exchange_host::Selector) is written over, and
//! `GET /api/catalogue/connectors/{id}/credentials` returns what that connector **declares** it
//! needs in order to be connected.
//!
//! # Three things this answers, and two it does not
//!
//! It answers **what exists**, **what each operation declares** and **what each connector
//! declares**. It does *not* answer what the caller may run: every operation carries
//! `admitted: null`, and nothing is ever filtered out for want of a grant
//! ([`view::OperationView::admitted`] has the argument). Nor does it answer whether anyone *holds*
//! a declared credential — that is per-tenant state, it lives on `GET /api/connections`, and
//! [`view::ConnectorCredentials`] is where the line is drawn.
//!
//! [`view`] holds the whole response contract as pure data, so the shape is tested without a
//! transport. The handlers below are a thin projection of it, and have only a status code to get
//! right.
//!
//! # Why these routes are anonymous
//!
//! All three are [`Access::Anonymous`], which made them the first routes besides `/health` that
//! answer a caller this host has not identified. That is a decision, not an oversight, so here is
//! the argument — and `super::tests::the_anonymous_surface_is_only_what_was_declared_anonymous` is
//! what holds anyone to it.
//!
//! The catalogue is `&'static` data compiled in from `codewandler-connector-catalog`, a published
//! crates.io package. It is byte-identical in every deployment of this version, and anyone who can
//! run `cargo add` already has all of it. It names no tenant, no principal and no connection, and
//! it carries no credential **value** — this module serves a connector's id, vendor, description
//! and operation count; an operation's id, service, description and declared metadata; and a
//! connector's declared credential *names*, and nothing else. Most of all it is **structurally
//! incapable** of leaking a permission: it never reads a principal, never consults a grant and
//! never filters, which is the same property `admitted: null` states on the wire.
//!
//! What it does disclose is which catalogue this deployment was built against. That is a
//! fingerprint, and an operator who wants it closed should be able to close it — but that is a
//! deployment policy. A route that answered `401` unconditionally (there is no identity provider to
//! bind until X-03) would not be a stricter catalogue; it would be no catalogue at all, and the
//! console being written against this contract would have nothing to read.
//!
//! The routes that must *not* be anonymous are the ones this one is deliberately unlike:
//! connections, leases and invocation are per tenant and reach credentials. This one is a
//! directory.

use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, MethodRouter};
use axum::Json;

use super::{Access, Module, Route};
use crate::state::AppState;

mod view;

/// This module's contribution to the surface.
pub(super) const MODULE: Module = Module {
    name: "catalogue",
    routes: &[
        Route {
            path: "/api/catalogue/connectors",
            access: Access::Anonymous,
            method_router: connectors_route,
        },
        Route {
            path: "/api/catalogue/connectors/{id}/operations",
            access: Access::Anonymous,
            method_router: operations_route,
        },
        Route {
            path: "/api/catalogue/connectors/{id}/credentials",
            access: Access::Anonymous,
            method_router: credentials_route,
        },
        Route {
            path: "/api/catalogue/connectors/{id}/channels",
            access: Access::Anonymous,
            method_router: channels_route,
        },
    ],
};

fn connectors_route() -> MethodRouter<AppState> {
    get(connectors)
}

fn operations_route() -> MethodRouter<AppState> {
    get(operations)
}

fn credentials_route() -> MethodRouter<AppState> {
    get(credentials)
}

fn channels_route() -> MethodRouter<AppState> {
    get(channels)
}

/// Every connector this binary carries.
async fn connectors() -> Json<view::ConnectorList> {
    Json(view::connectors())
}

/// One connector's operations, or a refusal naming the id that was asked for.
async fn operations(Path(connector): Path<String>) -> Response {
    match view::connector_operations(&connector) {
        Ok(Some(operations)) => Json(operations).into_response(),
        // A 404 naming the id, never an empty 200: a client that cannot tell "no such connector"
        // from "a connector with nothing in it" cannot tell a typo from a gap in the catalogue.
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(view::UnknownConnector::new(&connector)),
        )
            .into_response(),
        Err(error) => {
            tracing::error!(%error, connector, "the compiled catalogue could not be projected");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "catalogue projection failed",
                    "connector": connector,
                })),
            )
                .into_response()
        }
    }
}

/// One connector's declared credentials, or a refusal naming the id that was asked for.
///
/// **The declaration, never a tenant's state.** This says which credentials `slack` declares; it
/// says nothing about whether anyone has stored one, which is `GET /api/connections` and is
/// `Access::Principal`. See [`view::ConnectorCredentials`] for why that distinction is what keeps
/// this route on the anonymous surface, and for why it is a sibling of `/operations` rather than a
/// field on it.
async fn credentials(Path(connector): Path<String>) -> Response {
    match view::connector_credentials(&connector) {
        Some(credentials) => Json(credentials).into_response(),
        // The same `404`, and for the same reason: "no such connector" and "a connector that
        // declares nothing" are different answers, and `freshdesk` is genuinely the second.
        None => (
            StatusCode::NOT_FOUND,
            Json(view::UnknownConnector::new(&connector)),
        )
            .into_response(),
    }
}

/// One connector's generated channel declarations, never its runtime connection material.
async fn channels(Path(connector): Path<String>) -> Response {
    match view::connector_channels(&connector) {
        Some(channels) => Json(channels).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(view::UnknownConnector::new(&connector)),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    use axum::body::Body;
    use axum::http::Request;
    use serde_json::Value;
    use tower::ServiceExt;

    /// Drive one `GET` through the **assembled app** — not this module's router in isolation, so
    /// that the entry in `super::MODULES` is part of what these tests prove.
    ///
    /// The status is returned beside the raw bytes rather than beside a parsed body, because a path
    /// nothing is mounted at answers `404` with no body at all: a helper that parsed first would
    /// report an unmounted route as "the body is not JSON", which is a true sentence about the
    /// wrong thing.
    async fn get_raw(path: &str) -> (StatusCode, Vec<u8>) {
        let response = crate::routes::app(AppState::without_identity())
            .oneshot(
                Request::builder()
                    .uri(path)
                    .body(Body::empty())
                    .expect("a well-formed request"),
            )
            .await
            .expect("a router is infallible");

        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("the body is fully readable");

        (status, body.to_vec())
    }

    /// [`get_raw`], with the body parsed — every route in this module answers JSON.
    async fn get_json(path: &str) -> (StatusCode, Value) {
        let (status, body) = get_raw(path).await;

        (
            status,
            serde_json::from_slice(&body).expect("every response body here is JSON"),
        )
    }

    #[tokio::test]
    async fn the_listing_answers_with_the_whole_catalogue() {
        let (status, body) = get_json("/api/catalogue/connectors").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            serde_json::to_value(view::connectors()).expect("serialises"),
        );
        assert!(
            !body["connectors"].as_array().expect("an array").is_empty(),
            "an empty listing would pass every other assertion here",
        );
    }

    /// The story's Acceptance over the transport this time: the metadata a `Selector` reads is on
    /// every operation of a response a client actually receives.
    #[tokio::test]
    async fn a_connectors_operations_answer_with_their_metadata() {
        let (status, body) = get_json("/api/catalogue/connectors/zendesk/operations").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["connector"], "zendesk");

        let operations = body["operations"].as_array().expect("an array");
        assert!(!operations.is_empty(), "zendesk publishes operations");

        for operation in operations {
            let id = &operation["id"];
            assert!(operation["risk"].is_string(), "{id} carries no risk");
            assert!(
                operation["idempotency"].is_string(),
                "{id} carries no idempotency",
            );
            assert!(operation["effects"].is_array(), "{id} carries no effects");
            assert_eq!(
                operation["effects_derived"], true,
                "{id} served effects without saying they were inferred",
            );
            assert_eq!(
                operation["admitted"],
                Value::Null,
                "{id} answered a permission question nothing here can ask",
            );
        }
    }

    /// **The story's failing-first test.** A caller can read what a connector declares by asking
    /// for it, rather than by asking to do something it knows will be refused.
    ///
    /// Before this route existed the only place the service stated a declaration was the `422` a
    /// deliberately-empty `POST /api/connections/{connector}` returns, so the console read a
    /// capability fact out of an error body. Asserted over the transport, and against the
    /// catalogue rather than against a fixture, so a connector that gains a credential upstream is
    /// published the day the dependency moves.
    #[tokio::test]
    async fn a_connectors_declared_credentials_are_published() {
        let (status, body) = get_raw("/api/catalogue/connectors/slack/credentials").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "nothing publishes what a connector declares, so it can only be read out of a refusal",
        );

        let body: Value = serde_json::from_slice(&body).expect("a JSON body");
        assert_eq!(body["connector"], "slack");

        let published: Vec<&str> = body["credentials"]
            .as_array()
            .expect("an array")
            .iter()
            .map(|credential| credential["name"].as_str().expect("a name"))
            .collect();
        let declared: Vec<&str> =
            connector_catalog::provider(connector_catalog::ProviderKey::id("slack"))
                .expect("the catalogue carries slack")
                .auth
                .iter()
                .map(|credential| credential.name)
                .collect();

        assert!(
            !declared.is_empty(),
            "an empty declaration would pass every assertion here vacuously",
        );
        assert_eq!(
            published, declared,
            "the published declaration must be the catalogue's, in the connector's own order",
        );

        // The declaration, and never a tenant's state. This is the anonymous catalogue: it does
        // not know who is asking, so it must not carry a field that would only make sense if it
        // did.
        let serialised = body.to_string();
        for holding in ["held", "address", "tenant", "connection"] {
            assert!(
                !serialised.contains(holding),
                "the catalogue answered `{holding}`, which is a tenant's state and not a \
                 declaration: {serialised}",
            );
        }
    }

    #[tokio::test]
    async fn generated_channel_choices_are_public_but_connection_material_is_not() {
        let (status, body) = get_json("/api/catalogue/connectors/slack/channels").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["connector"], "slack");
        assert!(!body["channels"].as_array().expect("an array").is_empty());
        assert_eq!(body["channels"][0]["name"], "socket");
        assert_eq!(body["channels"][0]["events"][0]["name"], "app_mention");

        for channel in body["channels"].as_array().expect("an array") {
            let keys = channel
                .as_object()
                .expect("a channel object")
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            assert_eq!(
                keys,
                BTreeSet::from(["description", "events", "name", "service", "transport",]),
                "a channel declaration exposed more than its public selection metadata: {channel}",
            );
        }
    }

    /// `404` naming the id, never an empty `200`.
    #[tokio::test]
    async fn an_unknown_connector_is_refused_and_named() {
        let (status, body) = get_json("/api/catalogue/connectors/no-such-vendor/operations").await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            body,
            serde_json::json!({
                "error": "unknown connector",
                "connector": "no-such-vendor",
            }),
        );
    }

    /// Adding a connector upstream must change nothing here, so the route is asserted against the
    /// catalogue itself rather than against a fixture somebody would have to maintain beside it.
    #[tokio::test]
    async fn the_listing_is_the_catalogue_and_not_a_fixture() {
        let (_, body) = get_json("/api/catalogue/connectors").await;
        let listed = body["connectors"].as_array().expect("an array");

        assert_eq!(listed.len(), connector_catalog::providers().len());

        for (entry, provider) in listed.iter().zip(connector_catalog::providers()) {
            assert_eq!(entry["id"], provider.id);
            assert_eq!(entry["operation_count"], provider.operations.len());
            assert_eq!(entry["channel_count"], provider.channels.len());
        }
    }
}
