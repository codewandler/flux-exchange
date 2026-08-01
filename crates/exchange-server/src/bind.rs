//! Where the server may listen, and the refusal that decides it.
//!
//! One rule lives here, and it is the reason this module exists at all: **an address other machines
//! can reach requires something that can turn a request into a principal.** See
//! `docs/designs/http-surface.md`.

use std::fmt;
use std::net::{AddrParseError, IpAddr, Ipv4Addr, SocketAddr};

/// Where the server listens when nothing says otherwise.
///
/// Loopback, because a service that holds other people's credentials and is reachable *by default*
/// is reachable before anybody decided it should be.
pub const DEFAULT_BIND: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);

/// The environment variable that overrides [`DEFAULT_BIND`].
pub const BIND_ENV: &str = "FLUX_EXCHANGE_BIND";

/// Whether this composition bound an [`Identity`](exchange_host::Identity) port.
///
/// A two-state answer rather than the port itself: the bind decision does not care *how* a caller
/// would be authenticated, only whether anything could.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityBinding {
    /// An identity provider is configured, so a request can become a principal.
    Bound,
    /// None is configured, so every caller is anonymous whatever it presents.
    Unbound,
}

/// Decide whether the server may listen on `bind`.
///
/// flux's own HTTP server refuses this same shape for the same reason: a daemon that auto-approves
/// behind an open listener is remote code execution. A host that holds other people's credentials
/// behind one is worse, because the loss outlives the process.
///
/// This **refuses**; it does not warn and start. An operator who misses a warning is running an
/// open credential-holding service, and there is no later moment at which that mistake announces
/// itself — the first time anyone finds out is from the outside.
pub fn admit_bind(bind: SocketAddr, identity: IdentityBinding) -> Result<(), StartupRefusal> {
    // Only loopback is unreachable from elsewhere. In particular the unspecified addresses
    // (`0.0.0.0`, `::`) are *not* loopback, which is the case an operator most often reaches for.
    if bind.ip().is_loopback() || identity == IdentityBinding::Bound {
        return Ok(());
    }

    Err(StartupRefusal::ReachableBindWithoutIdentity { bind })
}

/// Why the server would not start. Every variant refuses; none repairs.
///
/// Hand-written rather than derived: `thiserror` is the library's convention and this binary does
/// not carry the dependency. The obligation the convention encodes — name the address, never the
/// value, and distinguish failures an operator answers differently — is met below.
#[derive(Debug)]
pub enum StartupRefusal {
    /// The bind is reachable from outside this machine and nothing could authenticate a caller.
    ReachableBindWithoutIdentity {
        /// The address that was asked for.
        bind: SocketAddr,
    },

    /// The configured bind is not a socket address.
    UnreadableBind {
        /// What was configured. A bind address is not a secret, so naming it is the fastest fix.
        value: String,
        /// The parse failure underneath.
        source: AddrParseError,
    },

    /// The address could not be listened on — in use, or not permitted.
    ///
    /// Deliberately distinct from [`StartupRefusal::ReachableBindWithoutIdentity`]: one is this
    /// host refusing a configuration, the other is the machine refusing a socket, and an operator
    /// does entirely different things about them.
    BindUnavailable {
        /// The address that was asked for.
        bind: SocketAddr,
        /// The operating system's reason.
        source: std::io::Error,
    },

    /// The server was listening and stopped serving.
    Serving {
        /// The transport's reason.
        source: std::io::Error,
    },
}

impl fmt::Display for StartupRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Names both things that would have worked, because the operator cannot tell from the
            // outside which half of the pair they meant to change.
            Self::ReachableBindWithoutIdentity { bind } => write!(
                f,
                "refusing to serve on {bind}: it is reachable from outside this machine and no \
                 identity provider is configured, so every caller would be anonymous. Either bind \
                 loopback ({BIND_ENV}={DEFAULT_BIND}), or configure an identity provider and start \
                 again",
            ),
            Self::UnreadableBind { value, .. } => write!(
                f,
                "{BIND_ENV} is not a socket address: {value:?}. Expected `host:port`, \
                 e.g. {DEFAULT_BIND}",
            ),
            Self::BindUnavailable { bind, source } => {
                write!(f, "cannot listen on {bind}: {source}")
            }
            Self::Serving { source } => write!(f, "stopped serving: {source}"),
        }
    }
}

impl std::error::Error for StartupRefusal {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ReachableBindWithoutIdentity { .. } => None,
            Self::UnreadableBind { source, .. } => Some(source),
            Self::BindUnavailable { source, .. } => Some(source),
            Self::Serving { source } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(raw: &str) -> SocketAddr {
        raw.parse().expect("a literal socket address")
    }

    /// The refusal this story exists for: a bind every machine on the network can reach, with
    /// nothing configured that could turn a request into a principal, must not start.
    #[test]
    fn a_reachable_bind_without_identity_is_refused() {
        let refusal = admit_bind(addr("0.0.0.0:8080"), IdentityBinding::Unbound)
            .expect_err("a reachable bind with no identity must be refused at startup");

        assert!(matches!(
            refusal,
            StartupRefusal::ReachableBindWithoutIdentity { .. }
        ));
    }

    /// A refusal that does not say what would have worked just moves the guessing to the operator.
    #[test]
    fn the_refusal_names_what_would_have_worked() {
        let refusal = admit_bind(addr("0.0.0.0:8080"), IdentityBinding::Unbound)
            .expect_err("a reachable bind with no identity must be refused at startup");
        let message = refusal.to_string();

        assert!(message.contains("0.0.0.0:8080"), "{message}");
        assert!(message.contains(BIND_ENV), "{message}");
        assert!(message.contains("127.0.0.1:8080"), "{message}");
        assert!(message.contains("identity provider"), "{message}");
    }

    /// The unspecified addresses are the ones an operator actually reaches for, and neither is
    /// loopback. If either slipped through, the rule would be decorative.
    #[test]
    fn the_unspecified_addresses_are_reachable() {
        for open in ["0.0.0.0:8080", "[::]:8080", "192.168.1.10:8080"] {
            assert!(
                admit_bind(addr(open), IdentityBinding::Unbound).is_err(),
                "`{open}` must be refused without an identity provider",
            );
        }
    }

    #[test]
    fn loopback_is_admitted_with_no_identity_at_all() {
        for local in ["127.0.0.1:8080", "127.0.0.2:9000", "[::1]:8080"] {
            assert!(
                admit_bind(addr(local), IdentityBinding::Unbound).is_ok(),
                "`{local}` is unreachable from elsewhere and must be admitted",
            );
        }
    }

    /// The refusal is about *anonymity*, not about the address. Bind wherever you like once a
    /// caller can be resolved to a principal.
    #[test]
    fn a_reachable_bind_is_admitted_once_an_identity_is_bound() {
        assert!(admit_bind(addr("0.0.0.0:8080"), IdentityBinding::Bound).is_ok());
    }

    #[test]
    fn the_default_bind_is_loopback() {
        assert!(DEFAULT_BIND.ip().is_loopback());
        assert_eq!(DEFAULT_BIND.to_string(), "127.0.0.1:8080");
    }
}
