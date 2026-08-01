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
