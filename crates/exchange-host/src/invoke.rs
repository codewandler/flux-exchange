//! **Running one operation.** The caller names an operation id, and nothing else about the request
//! is theirs.
//!
//! This is the module `docs/designs/invoke.md` describes, and the whole of its argument is a
//! negative: *this host constructs no request of its own*. Every path through here ends in
//! [`connector_pack::resolve`], and the request is evaluated from the operation's own compiled Flux
//! by code that lives upstream. What is left here is wiring — a catalogue lookup, a deployment
//! gate, two ports bound to one tenant, and a context — and the wiring is deliberately the boring
//! part.
//!
//! # What a caller cannot name
//!
//! Each of these is a claim about the *shape* of [`Invoker::invoke`], not about a validator. A
//! validator can be relaxed by one person in one commit; the absence of a parameter cannot.
//!
//! - **Not a tenant.** `invoke` takes a [`Principal`] and has no parameter of type [`Tenant`] or
//!   `&str` that could stand in for one. A `Principal` carries its tenant and has no constructor
//!   taking one separately from an identity.
//! - **Not a host.** `params` is the operation's declared parameter object, handed to
//!   `Tool::execute` unread. The destination comes from the operation's compiled Flux, evaluated
//!   inside the pack. And behind that: this crate has no HTTP client to consume a URL with, which
//!   `tests/no_second_request_path.rs` holds it to.
//! - **Not a credential.** There is no field and no lookup a caller can steer. The address is
//!   composed inside `connector-pack` from the tenant plus three `&'static str`s read out of a
//!   compiled-in catalogue. **There is no code path from a request body to `CredentialRef::new`.**
//! - **Not a runtime, and not the transport.** [`Runtime`] has no constructor taking caller input,
//!   and the transport is an [`Egress`] supplied once at construction.
//!
//! # The transport is a port, and so is the context
//!
//! [`Egress`] is `connector-pack`'s own transport port, taken one level up: a composition hands in
//! the `http.request` it has already configured with its egress allow-list and its audit sink.
//! [`Contexts`] is the second port and exists for a narrower reason — `flux_runtime::ToolContext`
//! can only be built over a `flux_system::System`, and `flux-system` carries `flux_system::net`,
//! which dials. A crate whose safety argument is "there is no transport here" does not take a
//! dependency that contains one in order to save a composition four lines.

use std::sync::Arc;

use connector_pack::{ConfigStore, Configuration, Credentials, Egress};
use flux_runtime::{Tool, ToolContext};
use serde_json::Value;

use crate::{
    Admitted, ConnectorSurface, Deployment, Principal, Runtime, RuntimeRefusal, SecretStore,
};

/// A source of fresh [`ToolContext`]s — **one per invocation**.
///
/// The redactor lives on the context, so a credential registered for one call must not outlive it
/// into another tenant's. A pooled or long-lived context would also defeat `connector-pack`'s
/// idempotence check, which asks the redactor *in hand* rather than consulting a memo — precisely
/// because a remembered registration against a redactor that never received the value is a
/// credential travelling unheld.
///
/// It is a port rather than something this crate constructs because `ToolContext::new` takes an
/// `Arc<flux_system::System>` and `flux-system` is where flux's real IO lives. See this module's
/// documentation.
pub trait Contexts: Send + Sync {
    /// A context nothing else holds.
    fn fresh(&self) -> ToolContext;
}

impl<F> Contexts for F
where
    F: Fn() -> ToolContext + Send + Sync,
{
    fn fresh(&self) -> ToolContext {
        self()
    }
}

/// **The runtime gate**, as the one function [`Invoker::invoke`] calls.
///
/// Factored out rather than inlined because of a measured gap: every connector in the shipped
/// catalogue declares [`Runtime::Http`], so **no shipped connector exercises this refusal**. A test
/// that could only reach it through a real catalogue entry would be a test of nothing. This
/// function takes a [`ConnectorSurface`] — a value a test can construct — so the refusal is driven
/// directly, on the same terms `connector-pack` records for its own query-placement branch.
///
/// There is deliberately no override, no force flag and no per-connector exception. The refusal
/// exists because isolating a locally-executing runtime is an OS or pod concern that cannot be done
/// from inside one Rust process, and an override would be a lie about that.
///
/// **It returns [`Admitted`] rather than `()`**, and that is the load-bearing part rather than a
/// convenience. `Admitted::resolve` is the only route from this module to `connector_pack::resolve`,
/// and an `Admitted` cannot be constructed — so calling this and discarding the answer does not
/// type-check any more than not calling it at all. Read [`Admitted`] for the whole argument and for
/// what it still does not cover; the short version is that the previous guard was a source scanner
/// and a source scanner cannot tell a live call from a string literal.
///
/// # Errors
///
/// [`RuntimeRefusal`] when this deployment will not serve the connector's declared runtime.
pub fn admit_runtime(
    deployment: Deployment,
    surface: &ConnectorSurface,
) -> Result<Admitted, RuntimeRefusal> {
    Admitted::of(deployment, surface.runtime)
}

impl Admitted {
    /// **The dispatch seam, hung off the gate's answer.**
    ///
    /// `self` by value, and an inherent method on [`Admitted`] rather than a free call inside
    /// [`Invoker::invoke`], because that placement *is* the enforcement: `connector_pack::resolve`
    /// has no other caller in this crate, `Admitted` has a private field in `runtime.rs`, and so
    /// there is no expression in `invoke` that reaches the pack without an `Ok` from the gate. The
    /// three mutations that defeated X-48's first attempt — a discarded `Result`, a call under
    /// `if false`, and the marker moved into a string literal — are each a compile error here.
    ///
    /// It is a method on the witness rather than a private method of [`Invoker`] taking one,
    /// because an ignored `_admitted: Admitted` parameter is a parameter the next person deletes as
    /// dead weight. A receiver is not deletable without rewriting the call.
    ///
    /// What it does **not** hold: somebody writing `connector_pack::resolve(…)` directly in
    /// `invoke` again. That leaves this method unused, which is `dead_code` — an error under the
    /// gate's `clippy -D warnings` — and it puts a second pack entry point in the crate, which
    /// `tests/no_second_request_path.rs` counts. Two mechanisms, neither of them a regex.
    ///
    /// # Errors
    ///
    /// Whatever the pack refuses with, unshaped. [`InvokeRefusal::classify`] sorts it into "was it
    /// sent?" at the call site, where the invocation's redactor is in hand.
    fn resolve(
        self,
        entry: &'static connector_catalog::Operation,
        egress: Egress,
        credentials: Credentials,
        settings: Configuration,
    ) -> flux_core::Result<Arc<dyn Tool>> {
        connector_pack::resolve(entry, egress, credentials, settings)
    }
}

/// What this host will run an operation with.
///
/// Every field is a port a composition bound. None of them is reachable from a request, and that is
/// the property rather than a convention: a caller cannot influence the transport, the store, the
/// settings or the deployment class, because there is no parameter through which any of them
/// arrives.
pub struct Invoker {
    /// Whether this process serves one tenant or many. Decides which runtimes may run at all.
    deployment: Deployment,
    /// **flux's egress, not ours.** The one transport every operation's bytes leave through, and
    /// the only thing in this struct that can reach a network. It is handed straight to the pack.
    egress: Egress,
    /// Where credentials live, as the port. This host reads none of them itself — `connector-pack`
    /// resolves at an address this host never composes.
    credentials: Arc<dyn SecretStore>,
    /// A tenant's non-secret connection settings — the `{subdomain}` in a templated base URL.
    settings: Arc<dyn ConfigStore>,
    /// Where a fresh per-invocation context comes from.
    contexts: Arc<dyn Contexts>,
}

impl Invoker {
    /// Bind every port an invocation runs through.
    ///
    /// All five together, rather than a builder, because there is no useful partial composition: an
    /// invoker missing any one of them could not run an operation, and an `Option` would push that
    /// discovery from startup to the first request.
    pub fn new(
        deployment: Deployment,
        egress: Egress,
        credentials: Arc<dyn SecretStore>,
        settings: Arc<dyn ConfigStore>,
        contexts: Arc<dyn Contexts>,
    ) -> Self {
        Self {
            deployment,
            egress,
            credentials,
            settings,
            contexts,
        }
    }

    /// **Run one catalogue operation for `principal`'s tenant.**
    ///
    /// `params` is the operation's own declared parameter object, verbatim — there is no envelope,
    /// because an envelope is a place to put a field, and the field that eventually gets added is
    /// `endpoint`, or `base_url`, or `credential`.
    ///
    /// The order below is load-bearing at three points and is the design's §2:
    ///
    /// 1. the catalogue lookup, so an unknown id is refused before anything is bound;
    /// 2. the runtime gate, **before anything touches the credential store** — a refusal after a
    ///    secret has been read has already moved that secret into this process's memory for a
    ///    connector it was never going to run;
    /// 3. both ports built from **one** tenant value in one expression, which is what makes
    ///    `connector-pack`'s `TenantMismatch` unreachable here rather than merely untriggered.
    ///
    /// # Errors
    ///
    /// [`InvokeRefusal`], every variant of which refuses and none of which repairs. See
    /// [`InvokeRefusal::sent`] for the question an agent actually needs answered.
    pub async fn invoke(
        &self,
        principal: &Principal,
        operation: &str,
        params: Value,
    ) -> Result<Invocation, InvokeRefusal> {
        // 1. The catalogue. A global lookup by the id the catalogue itself spells, so the connector
        //    is derived rather than named — a redundant name is a name that can disagree.
        let entry = connector_catalog::operation(connector_catalog::OperationKey::id(operation))
            .ok_or_else(|| InvokeRefusal::UnknownOperation {
                operation: operation.to_owned(),
            })?;
        let provider =
            connector_catalog::provider(connector_catalog::ProviderKey::id(entry.provider))
                .ok_or_else(|| InvokeRefusal::UnknownOperation {
                    operation: operation.to_owned(),
                })?;

        // 2. The deployment gate. Before the store, and before projection: there is no reason to
        //    read a secret for, or project, a connector this deployment will not execute.
        //
        //    The `Admitted` it hands back is not a formality — it is the only key to step 6. See
        //    `Admitted` in `runtime.rs`: the ordering below is a comment, but *that the gate ran at
        //    all* is checked by the compiler rather than by anything anybody has to remember.
        let admitted = admit_runtime(self.deployment, &ConnectorSurface::of(provider))?;

        // 3. (X-13's slot.) The grant check goes **here** — after the runtime refusal, which is a
        //    property of the deployment and the same answer for every principal, and before the
        //    ports are bound. Until X-13 lands, `invoke` is gated by identity alone, and that is a
        //    stated gap rather than a position. Deliberately no interim scheme: a half-grant is
        //    worse than a stated gap, because the next story has to unpick it.

        // 4. One tenant, one expression, two ports. The tenant is read off the resolved principal
        //    and from nothing a caller controls.
        let tenant = principal.tenant().as_str();
        let (credentials, settings) = (
            Credentials::new(self.credentials.clone(), tenant),
            Configuration::new(self.settings.clone(), tenant),
        );
        let credentials = credentials.map_err(|error| InvokeRefusal::refused(entry.id, &error))?;
        let settings = settings.map_err(|error| InvokeRefusal::refused(entry.id, &error))?;

        // 5. A context nothing else holds, before anything resolves a credential into it.
        let ctx = self.contexts.fresh();

        // 6. The seam, reached **through the gate's answer** — `admitted` is consumed here and there
        //    is no other way in, which is what makes step 2 unskippable rather than merely present.
        //
        //    `resolve` rather than `pack` is the caller-facing half of upstream's C-413: `pack`
        //    installs what a host *advertises* and withholds `expose = false` operations, which for
        //    an execute route would withhold the *call* as a side effect of withholding the *tool*.
        //    Both run the identical admission checks.
        let tool = admitted
            .resolve(entry, self.egress.clone(), credentials, settings)
            .map_err(|error| InvokeRefusal::classify(entry, error, &ctx))?;

        // 7. Inside this call and nowhere else: the pack resolves the credential, registers every
        //    travelling form with `ctx.redactor`, verifies the registration took, evaluates the
        //    operation's compiled Flux into a request, places the credential, and hands the whole
        //    thing to flux's own `http.request`. **None of it is ours.**
        let result = tool
            .execute(&ctx, params)
            .await
            .map_err(|error| InvokeRefusal::classify(entry, error, &ctx))?;

        // 8. Rendered through the **same** redactor the credential was registered with. Several
        //    vendors echo a token back in an error body, so this call is the difference between
        //    "the pack kept it off the wire" and "it stayed off every surface".
        Ok(Invocation {
            operation: entry.id.to_owned(),
            content: ctx.redactor.redact(&result.content),
            view: result.view.as_deref().map(|view| ctx.redactor.redact(view)),
            is_error: result.is_error,
        })
    }
}

/// What an operation answered with.
///
/// Whatever `http.request` returned, redacted and otherwise **whole**. A vendor's `404` is an
/// *answer*, and flattening it into a host error would destroy the distinction between "the vendor
/// said no" and "we could not ask".
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Invocation {
    /// The operation that ran, as the catalogue spells it.
    pub operation: String,
    /// The canonical result. At the pinned engine line this is the record
    /// `{status, headers, body}`, JSON encoded.
    pub content: String,
    /// The model-facing rendering, when the tool produced one distinct from `content`.
    pub view: Option<String>,
    /// Whether the tool reported its own failure. **Not** whether the vendor answered `4xx`.
    pub is_error: bool,
}

/// Whether the request reached the vendor.
///
/// The question an agent actually needs and the one the HTTP status space cannot express — a `502`
/// says nothing about whether the effect happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Sent {
    /// The refusal **provably precedes dispatch**. Nothing left this process.
    No,
    /// The failure follows dispatch, or cannot be told apart from one that does.
    Maybe,
}

/// Why an invocation did not produce a result. Every variant refuses; none repairs.
#[derive(Debug, thiserror::Error)]
pub enum InvokeRefusal {
    /// Nothing in the catalogue spells the id the caller named.
    #[error("no operation in this catalogue is called `{operation}`")]
    UnknownOperation {
        /// The id that was asked for.
        operation: String,
    },

    /// This deployment will not serve the connector's declared runtime. Routed through unchanged.
    #[error("{0}")]
    Runtime(#[from] RuntimeRefusal),

    /// **The pack refused before the request was built.** Nothing was sent.
    ///
    /// Every `connector_pack::Error` — a missing credential, an unaddressable connector, an
    /// unredactable value, an incomplete connection, a configuration value that would reshape the
    /// request — arrives here, because upstream converts all of them into
    /// `flux_core::Error::Config` on the way out of `Tool::execute`. That collapse is real and is
    /// recorded rather than papered over: this host cannot tell a *missing* credential from a
    /// credential store that was *unreachable*, so it reports the conservative answer for both —
    /// not sent, not retryable. `message` is upstream's own, which names the **address** and never
    /// the value, and never a length, because a length is a fingerprint.
    #[error("{message}")]
    Refused {
        /// The operation that refused.
        operation: String,
        /// Upstream's diagnostic, redacted through this invocation's own redactor.
        message: String,
    },

    /// The transport failed, and this host cannot prove the vendor did not receive the request.
    ///
    /// The one genuinely ambiguous row of the design's §5 table. Retrying is safe only when the
    /// catalogue declares the operation idempotent — `Idempotency::Conditional` is **not** treated
    /// as retryable, because the condition a conditional write depends on is stated in prose this
    /// host cannot evaluate, and a host that guesses turns "safe to repeat if you pass the same
    /// key" into a duplicated write.
    #[error("{message}")]
    Transport {
        /// The operation that was dispatched.
        operation: String,
        /// What the transport said, redacted through this invocation's own redactor.
        message: String,
        /// Whether repeating this call is safe, decided from the catalogue's declared idempotency.
        retryable: bool,
    },
}

impl InvokeRefusal {
    /// A pre-dispatch refusal from a value that is already a `connector_pack::Error`.
    ///
    /// Used for the two port constructions, which fail only for a tenant id that is not addressable
    /// — unreachable here, because [`Tenant`](crate::Tenant) validates at construction and this
    /// host holds no other spelling of one.
    fn refused(operation: &str, error: &connector_pack::Error) -> Self {
        Self::Refused {
            operation: operation.to_owned(),
            message: error.to_string(),
        }
    }

    /// Sort one `flux_core::Error` into "was it sent?".
    ///
    /// The classification rests on one upstream fact: `connector_pack::Error` converts into
    /// `flux_core::Error::Config`, and every one of its variants is raised **before** the request
    /// reaches the transport. Anything else came from the transport itself and may have been sent.
    ///
    /// The bias is deliberate and is the safe one in both directions: reporting "not sent, do not
    /// retry" for something that was in fact sent costs a caller a retry it might have wanted;
    /// reporting the reverse duplicates a write.
    ///
    /// The message is redacted through **this invocation's** redactor before it is kept, because a
    /// diagnostic is a surface like any other and the pack has already registered every travelling
    /// form of the credential with it.
    fn classify(
        entry: &'static connector_catalog::Operation,
        error: flux_core::Error,
        ctx: &ToolContext,
    ) -> Self {
        let message = ctx.redactor.redact(&error.to_string());

        match error {
            flux_core::Error::Config(_) => Self::Refused {
                operation: entry.id.to_owned(),
                message,
            },
            _ => Self::Transport {
                operation: entry.id.to_owned(),
                message,
                retryable: matches!(
                    entry.idempotency,
                    connector_catalog::Idempotency::Idempotent
                ),
            },
        }
    }

    /// Whether the request reached the vendor.
    pub fn sent(&self) -> Sent {
        match self {
            Self::UnknownOperation { .. } | Self::Runtime(_) | Self::Refused { .. } => Sent::No,
            Self::Transport { .. } => Sent::Maybe,
        }
    }

    /// Whether repeating this call could produce a different answer.
    ///
    /// **A missing credential is terminal**, and that is the load-bearing case. The request was
    /// never sent, so there is no ambiguity about whether the effect happened; nothing a retry can
    /// change is inside this system, because the refusal names an address a human has to go and put
    /// a value at; and a retrying agent against a credential-less connector is a loop against
    /// somebody else's rate limit.
    pub fn retryable(&self) -> bool {
        match self {
            Self::UnknownOperation { .. } | Self::Runtime(_) | Self::Refused { .. } => false,
            Self::Transport { retryable, .. } => *retryable,
        }
    }

    /// A stable machine-readable label for this class of refusal.
    ///
    /// Separate from the message so a caller can branch without parsing prose.
    pub fn label(&self) -> &'static str {
        match self {
            Self::UnknownOperation { .. } => "unknown_operation",
            Self::Runtime(_) => "runtime_refused",
            Self::Refused { .. } => "refused",
            Self::Transport { .. } => "transport",
        }
    }

    /// The operation the caller named, when one was resolved.
    pub fn operation(&self) -> Option<&str> {
        match self {
            Self::UnknownOperation { operation } => Some(operation),
            Self::Runtime(_) => None,
            Self::Refused { operation, .. } | Self::Transport { operation, .. } => Some(operation),
        }
    }
}

impl ConnectorSurface {
    /// The surface a catalogue provider declares.
    ///
    /// The runtime is **read**, not guessed. `docs/designs/invoke.md` records that
    /// `catalog::Provider` published no runtime and that this had to be derived as
    /// [`Runtime::Http`]; upstream C-405 landed the field in `connector-catalog` 0.9, so the
    /// derivation is gone and this is a projection of what the connector actually declares. The
    /// mapping below is exhaustive with no wildcard arm on purpose: a runtime added upstream is a
    /// new effect this host has not decided about, and it should be a compile error here.
    ///
    /// `operations` is left empty. This constructor exists for the runtime decision, which is a
    /// property of the connector; the operation metadata a [`Selector`](crate::Selector) reads is
    /// X-13's business and projecting several hundred `OperationFacts` per invocation to answer a
    /// question about one enum would be work nobody asked for.
    pub fn of(provider: &connector_catalog::Provider) -> Self {
        Self {
            connector: provider.id.to_owned(),
            runtime: match provider.runtime {
                connector_catalog::Runtime::Http => Runtime::Http,
                connector_catalog::Runtime::Socket => Runtime::Socket,
                connector_catalog::Runtime::Process => Runtime::Process,
                connector_catalog::Runtime::Container => Runtime::Container,
                connector_catalog::Runtime::Plugin => Runtime::Plugin,
                connector_catalog::Runtime::Remote => Runtime::Remote,
            },
            operations: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gate refuses every locally-executing runtime in a shared deployment, and says what
    /// would have worked.
    ///
    /// Driven against a constructed [`ConnectorSurface`] because **no shipped connector declares
    /// one** — see [`admit_runtime`]. `the_whole_catalogue_declares_http` below is what keeps that
    /// sentence true rather than remembered.
    #[test]
    fn a_shared_deployment_refuses_a_locally_executing_runtime() {
        for runtime in [
            Runtime::Socket,
            Runtime::Process,
            Runtime::Container,
            Runtime::Plugin,
        ] {
            let surface = ConnectorSurface {
                connector: "fixture".into(),
                runtime,
                operations: Vec::new(),
            };

            let refusal = admit_runtime(Deployment::MultiTenant, &surface)
                .expect_err("a shared deployment must refuse {runtime:?}");
            let message = refusal.to_string();
            assert!(message.contains("single-tenant"), "{message}");
            assert!(message.contains("per tenant"), "{message}");

            assert!(
                admit_runtime(Deployment::SingleTenant, &surface).is_ok(),
                "and a single-tenant deployment must still serve {runtime:?}",
            );
        }
    }

    /// Every connector this build carries declares `http`, which is why the test above needs a
    /// fixture.
    ///
    /// It is not a restatement of upstream's table: the day a connector ships declaring `process`,
    /// this fails and the fixture above stops being the only coverage the refusal has.
    #[test]
    fn the_whole_catalogue_declares_http() {
        for provider in connector_catalog::providers() {
            assert_eq!(
                ConnectorSurface::of(provider).runtime,
                Runtime::Http,
                "`{}` no longer declares http; `admit_runtime` now has live coverage and the \
                 fixture test above is no longer the only thing exercising it",
                provider.id,
            );
        }
    }

    /// A refusal raised before dispatch is terminal, and says so through both accessors.
    #[test]
    fn a_pre_dispatch_refusal_is_terminal() {
        let refusal = InvokeRefusal::UnknownOperation {
            operation: "no-such-operation".into(),
        };

        assert_eq!(refusal.sent(), Sent::No);
        assert!(!refusal.retryable());
        assert_eq!(refusal.label(), "unknown_operation");
    }
}
