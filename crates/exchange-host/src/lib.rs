//! `exchange-host` — the flux-exchange host, stated as ports.
//!
//! This crate is the part a product embeds. It carries the vocabulary and the rules, and it
//! deliberately carries **no** identity provider and **no** transport: those are traits a composing
//! binary binds. That is what lets one implementation serve a self-serve deployment and a company's
//! internal one without either learning about the other.
//!
//! It does carry one concrete binding — [`CredentialStore`] — and that is a narrowing of the claim
//! above rather than an exception to it, so it is worth saying exactly where the line is. What the
//! boundary protects is that the public crate has **no downstream dependency to leak through**. The
//! store behind that type is `connector_secrets::FileStore`, a flux-family crate this already
//! depends on and not a product's; the port a credential is resolved through is still
//! `SecretStore`, which [`CredentialStore::secrets`] hands back; and a deployment that wants Vault,
//! or a store of its own, binds that instead and never constructs this type. A default a composing
//! binary can decline puts no product concern in the shared code. A default it *could not* decline
//! would — so the moment this type stops being declinable, this paragraph has stopped being true.
//!
//! The design this implements is `docs/designs/ecosystem.md` in
//! [codewandler/flux](https://github.com/codewandler/flux). Three rules from it are enforced here
//! rather than described, because a rule that lives only in prose is a rule that erodes:
//!
//! 1. **The runtime is declared by the connector, never chosen by the caller** — so there is no
//!    constructor on [`Runtime`] that takes caller input.
//! 2. **A locally-executing runtime cannot be safely multi-tenant in one process** —
//!    [`Deployment::admits`] refuses one, and the refusal names what would have worked.
//! 3. **A grant selects operations by declared metadata, not by name** — [`Selector`] is a
//!    predicate over `risk`/`effects`/`idempotency`, with explicit ids only as exceptions.
//!
//! # Status
//!
//! Early. The types below are real and tested; the service around them is not built. See the
//! repository README for what exists today.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

mod connections;
// Unix only, and for the reason `connector_secrets::file` is: the whole of what protects a value in
// the file store is `0600` and `0700`, and a platform that cannot spell those would get a store that
// implied a safety it did not have.
#[cfg(unix)]
mod credentials;
mod grant;
mod lease;
mod principal;
mod runtime;

/// Re-exported so a composing binary can implement [`Identity`] without taking its own dependency
/// on `async-trait`.
///
/// The version matters as much as the convenience: `#[async_trait]` desugars to a signature that
/// both sides must agree on, so an implementor resolving a *different* version of the macro gets a
/// mismatched `resolve` and an error that names lifetimes rather than the actual problem. A port
/// that cannot be implemented without guessing a dependency version is not much of a port.
pub use async_trait::async_trait;

/// The addressing and the store port, re-exported rather than redefined.
///
/// Same argument as [`async_trait`] above, and the same failure it prevents. A composition binds a
/// [`SecretStore`] and holds [`CredentialRef`]s that this crate derived; if it had to name
/// `connector-secrets` itself to do so, it would be guessing a version, and a `CredentialRef` from a
/// different one is a different type with an identical name — an error about mismatched types where
/// the actual problem is a dependency line. There is one credential addressing scheme in this
/// ecosystem and this is a doorway to it, not a second copy.
pub use connector_secrets::{CredentialRef, Secret, SecretStore, StoreError, TENANTS_ROOT};

pub use connections::{address_path, ConnectionRefusal, ConnectorDeclaration, DeclaredCredential};
#[cfg(unix)]
pub use credentials::{CredentialStore, CredentialStoreError, CREDENTIAL_STORE_SETTING};
pub use grant::{Effect, Grant, Idempotency, OperationFacts, Risk, Selector};
pub use lease::{Lease, LeaseId, LeaseState};
pub use principal::{Principal, PrincipalKind, Tenant, TenantError};
pub use runtime::{Deployment, Runtime, RuntimeRefusal};

/// Errors the host raises. Every variant refuses; none repairs.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The tenant identifier is not usable as an address segment.
    #[error("tenant: {0}")]
    Tenant(#[from] TenantError),

    /// The deployment will not serve this connector's declared runtime.
    #[error("{0}")]
    Runtime(#[from] RuntimeRefusal),

    /// No grant held by this principal admits the operation.
    #[error("principal `{principal}` holds no grant admitting operation `{operation}`")]
    NotGranted {
        /// The principal that asked.
        principal: String,
        /// The operation it asked for.
        operation: String,
    },

    /// The lease named is not open, or is not this caller's.
    #[error("lease `{0}` is not open for this caller")]
    NoSuchLease(LeaseId),
}

/// A convenient result alias.
pub type Result<T> = std::result::Result<T, Error>;

/// The identity port: turn whatever a request carries into a [`Principal`].
///
/// A composing binary binds exactly one of these. The host never inspects the credential itself —
/// a bearer token, an OIDC id token, a mutual-TLS certificate and an internal service header are
/// all the same shape from here, which is the point.
///
/// **The tenant is read from the resolved principal and from nothing a caller controls.** Not a
/// path segment, not a body field, not a header a client may set. That single rule is what keeps
/// one tenant's session from reaching another tenant's credential, and an implementation that
/// derives the tenant from request input has broken it regardless of how it authenticates.
#[async_trait::async_trait]
pub trait Identity: Send + Sync {
    /// Resolve a request's credential material to a principal, or refuse.
    ///
    /// Returning `Ok(None)` means *"no principal"* — anonymous — and is distinct from an error,
    /// which means the credential was present and bad. A caller that collapses the two cannot tell
    /// "not signed in" from "your token expired", and neither can its users.
    async fn resolve(
        &self,
        presented: &str,
    ) -> std::result::Result<Option<Principal>, IdentityError>;
}

/// Why an identity could not be resolved.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    /// The credential was presented and is not valid.
    #[error("credential rejected")]
    Rejected,

    /// The identity provider could not be reached.
    ///
    /// Deliberately distinct from [`IdentityError::Rejected`]: an operator responds to these two
    /// in opposite ways, and collapsing them turns an outage into a wave of "your login is broken"
    /// reports.
    #[error("identity provider unreachable: {0}")]
    Unreachable(String),
}

/// A connector's declared surface, as much of it as the host needs to make a decision.
///
/// This is a *view* of a connector manifest, not a second model of one. The authority on what a
/// connector declares is `codewandler-connector-catalog`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorSurface {
    /// The connector id, as the catalogue spells it.
    pub connector: String,
    /// How its operations execute. Declared by the connector; never chosen by a caller.
    pub runtime: Runtime,
    /// The operations it publishes, with the metadata a [`Selector`] reads.
    pub operations: Vec<OperationFacts>,
}

impl ConnectorSurface {
    /// Every operation of this connector that `grants` admit for `principal`.
    ///
    /// Returns them sorted and deduplicated, so the answer is stable across calls — a catalogue
    /// that reorders between two requests would make a UI flicker and a diff meaningless.
    pub fn admitted<'a>(&'a self, grants: &[Grant]) -> BTreeSet<&'a str> {
        self.operations
            .iter()
            .filter(|op| grants.iter().any(|g| g.admits(&self.connector, op)))
            .map(|op| op.id.as_str())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(id: &str, risk: Risk) -> OperationFacts {
        OperationFacts {
            id: id.to_string(),
            risk,
            idempotency: Idempotency::Idempotent,
            effects: BTreeSet::from([Effect::Network]),
        }
    }

    /// The composed case the whole crate exists for: an agent holding a read-only grant sees the
    /// reads and does not see the writes — decided from declared metadata, with no list of names
    /// maintained anywhere.
    #[test]
    fn a_read_only_grant_admits_reads_and_refuses_writes() {
        let surface = ConnectorSurface {
            connector: "zendesk".into(),
            runtime: Runtime::Http,
            operations: vec![
                op("zendesk-ticket-show", Risk::Low),
                op("zendesk-ticket-list", Risk::Low),
                op("zendesk-ticket-delete", Risk::Destructive),
                op("zendesk-ticket-comment-add", Risk::Medium),
            ],
        };

        let read_only = Grant::for_connector("zendesk", Selector::at_most(Risk::Low));

        assert_eq!(
            surface.admitted(&[read_only]),
            BTreeSet::from(["zendesk-ticket-show", "zendesk-ticket-list"]),
        );
    }

    #[test]
    fn no_grant_admits_nothing() {
        let surface = ConnectorSurface {
            connector: "zendesk".into(),
            runtime: Runtime::Http,
            operations: vec![op("zendesk-ticket-show", Risk::Low)],
        };

        assert!(surface.admitted(&[]).is_empty());
    }

    /// A grant is scoped to its connector: holding one for `zendesk` must not reach `slack`, even
    /// for an operation whose metadata the selector would otherwise admit.
    #[test]
    fn a_grant_does_not_reach_another_connector() {
        let slack = ConnectorSurface {
            connector: "slack".into(),
            runtime: Runtime::Http,
            operations: vec![op("slack-chat-post-message", Risk::Low)],
        };

        let zendesk_grant = Grant::for_connector("zendesk", Selector::at_most(Risk::Low));

        assert!(slack.admitted(&[zendesk_grant]).is_empty());
    }
}
