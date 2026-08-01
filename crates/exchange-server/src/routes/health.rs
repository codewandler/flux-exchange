//! Liveness — the one route that answers a caller this host has not identified.

use axum::routing::{get, MethodRouter};
use axum::Json;
use serde_json::{json, Value};

use super::{Access, Module, Route};
use crate::state::AppState;

/// This module's contribution to the surface.
pub(super) const MODULE: Module = Module {
    name: "health",
    routes: &[Route {
        path: "/health",
        // The only anonymous route in the app. What keeps it the only one is not this line — it is
        // `super::tests::health_is_the_only_route_reachable_without_a_principal`, which asks the
        // assembled app rather than reading this table.
        access: Access::Anonymous,
        method_router: route,
    }],
};

fn route() -> MethodRouter<AppState> {
    get(health)
}

/// Answer that the process is up, and say nothing else.
///
/// Deliberately not a readiness report. A health route that names its dependencies is a health
/// route an unauthenticated caller can use to map the deployment, and this is the one route such a
/// caller can reach.
async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}
