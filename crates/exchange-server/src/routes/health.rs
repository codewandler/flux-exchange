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
        // Anonymous, and it has to be: an operator must be able to ask whether the process is up
        // before it can tell them anything else. What keeps the anonymous surface honest is not this
        // line — it is `super::tests::the_anonymous_surface_is_only_what_was_declared_anonymous`,
        // which asks the assembled app rather than reading this table.
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
