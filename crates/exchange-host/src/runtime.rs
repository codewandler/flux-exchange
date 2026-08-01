//! How a connector's operations execute, and which deployments may serve which.

use serde::{Deserialize, Serialize};

/// How a connector's operations reach the outside world.
///
/// **Declared by the connector, never chosen by a caller.** There is deliberately no constructor
/// here that takes request input: a caller who can pick the runtime is a caller who can pick an
/// effect, which is the whole confused-deputy problem in a new dress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Runtime {
    /// A guarded HTTP request. The overwhelming majority of connectors.
    Http,
    /// A guarded dial — TCP, UDP or ICMP.
    Socket,
    /// A guarded, argv-only process spawn, optionally sandboxed.
    Process,
    /// A process spawn inside a container or pod.
    Container,
    /// The flux plugin protocol over stdio — a special case of [`Runtime::Process`].
    Plugin,
    /// Delegation to another substrate that serves the guarded-IO port.
    Remote,
}

impl Runtime {
    /// Does executing this runtime consume *this host's* identity, network position or filesystem?
    ///
    /// This is the property that decides multi-tenant safety, and it is a property of the runtime
    /// rather than of the connector: an HTTP request leaves the machine and carries only the
    /// credential it was given, while a spawned process runs as whoever the host runs as.
    ///
    /// [`Runtime::Remote`] is local-executing **from this host's perspective** only in that it
    /// hands work to a substrate the host trusts; the isolation question moves to that substrate
    /// rather than disappearing, so it is treated as shareable here and the delegate is expected to
    /// answer for itself.
    pub const fn executes_locally(self) -> bool {
        match self {
            Runtime::Http | Runtime::Remote => false,
            Runtime::Socket | Runtime::Process | Runtime::Container | Runtime::Plugin => true,
        }
    }
}

/// Whether this process serves one tenant or many.
///
/// The distinction is not cosmetic and not a feature flag: it decides which runtimes may run at
/// all. A single-tenant deployment is the local-development mode — one operator, no sign-in — and
/// is also a perfectly good production shape for one team.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Deployment {
    /// One operator, one tenant, this process. Every runtime is available.
    SingleTenant,
    /// Many principals across many tenants. Locally-executing runtimes are refused.
    MultiTenant,
}

impl Deployment {
    /// May this deployment serve a connector declaring `runtime`?
    ///
    /// The refusal is the point. Process spawning, container exec and raw sockets consume the
    /// host's own identity, network position, filesystem and descriptors, so sharing one process
    /// between tenants shares those too. Isolating them is an OS or pod concern and cannot be done
    /// from inside one Rust process — so this refuses rather than pretending.
    pub fn admits(self, runtime: Runtime) -> Result<(), RuntimeRefusal> {
        match self {
            Deployment::SingleTenant => Ok(()),
            Deployment::MultiTenant if !runtime.executes_locally() => Ok(()),
            Deployment::MultiTenant => Err(RuntimeRefusal { runtime }),
        }
    }
}

/// **Proof that a deployment admitted a runtime**, as a value rather than as a line somebody ran.
///
/// This type exists because of a measured failure. X-48's first attempt held "`Invoker::invoke`
/// consults the runtime gate" with a source scanner, and a scanner cannot tell a call from a
/// substring: `let _ = admit_runtime(…);`, `if false { admit_runtime(…)?; }` and a bare
/// `let _note = "… admit_runtime(…) …";` all left the gate dead and the guard green. Any check that
/// reads text rather than meaning has that hole, so the answer is not a better regex.
///
/// The answer is that **the gate produces something the dispatch path cannot proceed without.**
/// [`runtime`](Self::runtime) is private and there is no public constructor, no `Default`, no
/// `Clone` — a value of this type cannot be spelled into existence anywhere, including in the
/// module that dispatches. The only producer is [`Admitted::of`] below, on its success arm. So a
/// function that requires an `Admitted` has, by construction, been reached through a gate that said
/// yes, and `rustc` is what checks it.
///
/// The chain that property hangs on, each link held by a different mechanism:
///
/// 1. `Invoker::invoke` cannot dispatch without one — **the compiler**, because
///    `connector_pack::resolve` is reachable from there only through `Admitted::resolve`.
/// 2. An `Admitted` comes only from [`Admitted::of`] — **the compiler**, via the private field.
/// 3. [`Admitted::of`] refuses a locally-executing runtime in a shared deployment — **tests**, in
///    this module and in `tests/invoke.rs`, driven over every such runtime and both deployments.
///    This link is behavioural because `of` takes the runtime as an argument, so a test can hand it
///    one no catalogue connector declares.
/// 4. The runtime handed in is the one the catalogue declares — **tests**, via
///    `ConnectorSurface::of`'s exhaustive match and `the_whole_catalogue_declares_http`.
///
/// What no link covers: a future edit that calls `Admitted::of(deployment, Runtime::Http)` with a
/// hardcoded runtime instead of the connector's. Link 3's tests do not see it, because `Http` is
/// the honest answer for all 53 shipped connectors — the same measured gap that makes the refusal
/// undrivable through `invoke` at all. It is named here rather than left for the next reviewer to
/// find.
#[derive(Debug)]
pub struct Admitted {
    /// The runtime that was admitted.
    ///
    /// Private, with no constructor beside [`Admitted::of`]. That is not encapsulation for its own
    /// sake — it is the entire enforcement, and a `pub` here would delete the argument above.
    runtime: Runtime,
}

impl Admitted {
    /// Consult the gate, and hand back the proof when it says yes.
    ///
    /// `pub(crate)`, deliberately: a composition asking *"may this deployment serve that runtime?"*
    /// wants [`Deployment::admits`], which answers in `bool`-shaped terms and is the published
    /// surface. The proof only means something to the one dispatch path that requires it, and
    /// exporting a way to mint one would hand a downstream crate the ability to fabricate the very
    /// thing this type exists to make unfabricable.
    ///
    /// # Errors
    ///
    /// [`RuntimeRefusal`] when this deployment will not serve `runtime`, unchanged from
    /// [`Deployment::admits`] — this adds a value on the success arm and decides nothing of its own.
    pub(crate) fn of(deployment: Deployment, runtime: Runtime) -> Result<Self, RuntimeRefusal> {
        deployment.admits(runtime)?;
        Ok(Self { runtime })
    }

    /// The runtime that was admitted.
    pub fn runtime(&self) -> Runtime {
        self.runtime
    }
}

/// A deployment refused a connector's declared runtime.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "a multi-tenant deployment cannot serve the `{runtime:?}` runtime: it executes on this host, so \
     one tenant's operation would run with the host's identity and network position. Run this \
     connector in a single-tenant deployment, or isolate it per tenant at the OS or pod level."
)]
pub struct RuntimeRefusal {
    /// The runtime that was refused.
    pub runtime: Runtime,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_multi_tenant_deployment_refuses_every_locally_executing_runtime() {
        for runtime in [
            Runtime::Socket,
            Runtime::Process,
            Runtime::Container,
            Runtime::Plugin,
        ] {
            assert!(
                Deployment::MultiTenant.admits(runtime).is_err(),
                "{runtime:?} must be refused in a shared deployment",
            );
        }
    }

    #[test]
    fn a_multi_tenant_deployment_serves_http_and_remote() {
        assert!(Deployment::MultiTenant.admits(Runtime::Http).is_ok());
        assert!(Deployment::MultiTenant.admits(Runtime::Remote).is_ok());
    }

    #[test]
    fn a_single_tenant_deployment_serves_everything() {
        for runtime in [
            Runtime::Http,
            Runtime::Socket,
            Runtime::Process,
            Runtime::Container,
            Runtime::Plugin,
            Runtime::Remote,
        ] {
            assert!(Deployment::SingleTenant.admits(runtime).is_ok());
        }
    }

    /// **Link 3 of [`Admitted`]'s chain**, and the only link in it a test can drive.
    ///
    /// The proof is not handed out for a runtime this deployment refuses. That is what makes the
    /// compiler's part of the argument worth anything: a witness that were minted on both arms
    /// would type-check every dispatch path exactly as it does now, and prove nothing at all.
    #[test]
    fn no_proof_is_minted_for_a_runtime_a_shared_deployment_refuses() {
        for runtime in [
            Runtime::Socket,
            Runtime::Process,
            Runtime::Container,
            Runtime::Plugin,
        ] {
            let refusal = Admitted::of(Deployment::MultiTenant, runtime)
                .expect_err("a shared deployment must refuse {runtime:?}");
            assert_eq!(refusal.runtime, runtime);

            assert_eq!(
                Admitted::of(Deployment::SingleTenant, runtime)
                    .expect("a single-tenant deployment serves every runtime")
                    .runtime(),
                runtime,
                "and the proof must carry the runtime it was asked about, not some other one",
            );
        }
    }

    /// And the gate does not refuse what it must serve, or the safe deployment is the unusable one.
    #[test]
    fn proof_is_minted_for_the_runtime_every_shipped_connector_declares() {
        for runtime in [Runtime::Http, Runtime::Remote] {
            assert_eq!(
                Admitted::of(Deployment::MultiTenant, runtime)
                    .expect("a shared deployment serves the runtimes that do not execute here")
                    .runtime(),
                runtime,
            );
        }
    }

    /// The refusal has to tell an operator what would have worked, or it is just a wall.
    #[test]
    fn the_refusal_names_the_way_out() {
        let refusal = Deployment::MultiTenant
            .admits(Runtime::Process)
            .expect_err("process must be refused");
        let message = refusal.to_string();

        assert!(message.contains("single-tenant"), "{message}");
        assert!(message.contains("per tenant"), "{message}");
    }
}
