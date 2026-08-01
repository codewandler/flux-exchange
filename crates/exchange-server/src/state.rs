//! What every route is built with.

use std::sync::Arc;

use exchange_host::Identity;

use crate::bind::IdentityBinding;

/// The state the router hands to every route.
///
/// It carries the *ports* a composition bound, never a credential and never a tenant. A tenant is
/// read from a resolved principal and from nothing else — there is deliberately nothing here that
/// a route could reach for instead.
#[derive(Clone)]
pub struct AppState {
    /// The identity port, if this composition bound one.
    ///
    /// `None` today. Binding a real provider is X-03; until then the only honest answer for a route
    /// that needs a principal is a refusal, and that is what `crate::routes` returns.
    identity: Option<Arc<dyn Identity>>,
}

impl AppState {
    /// A composition with no identity provider bound.
    pub fn without_identity() -> Self {
        Self { identity: None }
    }

    /// Whether a request could become a principal at all.
    ///
    /// This is what [`admit_bind`](crate::bind::admit_bind) decides on: the bind rule cares only
    /// that *something* could authenticate a caller, never how.
    pub fn identity_binding(&self) -> IdentityBinding {
        match self.identity {
            Some(_) => IdentityBinding::Bound,
            None => IdentityBinding::Unbound,
        }
    }

    /// The bound identity port.
    pub fn identity(&self) -> Option<&Arc<dyn Identity>> {
        self.identity.as_ref()
    }
}
