//! Where the server may listen, and the refusal that decides it.
//!
//! One rule lives here, and it is the reason this module exists at all: **an address other machines
//! can reach requires something that can turn a request into a principal.** See
//! `docs/designs/http-surface.md`.

use std::fmt;
use std::net::{AddrParseError, IpAddr, Ipv4Addr, SocketAddr};

use crate::dev_identity::{DevIdentityRefusal, DEV_IDENTITY_ENV};

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
    /// The **development** identity is armed: it mints principals from a roster the operator wrote
    /// at startup, and a roster handle is a credential with no secret in it.
    ///
    /// Deliberately not [`IdentityBinding::Bound`]. It *can* turn a request into a principal, so
    /// collapsing the two would be easy and would be exactly the hole the story warns about: a
    /// reachable bind whose authentication is a name anybody can guess is worse than no
    /// authentication, because the surface in front of it believes every caller.
    Development,
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
    if bind.ip().is_loopback() {
        return Ok(());
    }

    match identity {
        IdentityBinding::Bound => Ok(()),
        IdentityBinding::Unbound => Err(StartupRefusal::ReachableBindWithoutIdentity { bind }),
        // The development identity is the one binding that can resolve a principal and still must
        // not be exposed. This is the refusal the story asks for in place of a default.
        IdentityBinding::Development => {
            Err(StartupRefusal::ReachableBindWithDevelopmentIdentity { bind })
        }
    }
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

    /// The bind is reachable from outside this machine and the development identity is armed.
    ///
    /// Distinct from [`StartupRefusal::ReachableBindWithoutIdentity`] because the remedy is the
    /// opposite one: there, the operator adds an identity provider; here, the operator *removes*
    /// the one they have. A single refusal covering both would tell half of its readers to do the
    /// wrong thing.
    ReachableBindWithDevelopmentIdentity {
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

    /// The development identity was armed and could not be read.
    DevIdentity {
        /// Which part of the roster was unreadable.
        source: DevIdentityRefusal,
    },

    /// A credential store was named and could not be bound.
    ///
    /// Carries the store's refusal already rendered rather than as a typed source, because
    /// `CredentialStoreError` is `#[cfg(unix)]` — only the file store is — and a cfg-gated variant
    /// would make this enum two different types depending on the platform. Nothing is lost: that
    /// refusal names the path, the mode and what would have worked, and never a value.
    CredentialStore {
        /// The store's own refusal.
        reason: String,
    },
}

impl From<DevIdentityRefusal> for StartupRefusal {
    fn from(source: DevIdentityRefusal) -> Self {
        Self::DevIdentity { source }
    }
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
            Self::ReachableBindWithDevelopmentIdentity { bind } => write!(
                f,
                "refusing to serve on {bind}: it is reachable from outside this machine and the \
                 development identity is armed, so anyone who can reach it becomes any principal \
                 on the roster by naming it. Either bind loopback \
                 ({BIND_ENV}={DEFAULT_BIND}), or unset {DEV_IDENTITY_ENV} and configure a real \
                 identity provider",
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
            Self::DevIdentity { source } => write!(f, "{source}"),
            Self::CredentialStore { reason } => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for StartupRefusal {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ReachableBindWithoutIdentity { .. }
            | Self::ReachableBindWithDevelopmentIdentity { .. }
            | Self::CredentialStore { .. } => None,
            Self::DevIdentity { source } => Some(source),
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

    /// The hole X-03 must not open: arming a development identity resolves principals, so it would
    /// satisfy a rule that only asked "can anything authenticate a caller". It must not satisfy
    /// this one — a roster handle is a credential with no secret in it, and a network-reachable
    /// port that accepts one is worse than an unauthenticated port, because everything downstream
    /// believes the principal.
    #[test]
    fn a_reachable_bind_with_the_development_identity_is_refused() {
        for open in ["0.0.0.0:8080", "[::]:8080", "192.168.1.10:8080"] {
            let refusal = admit_bind(addr(open), IdentityBinding::Development)
                .expect_err("the development identity must never be reachable");

            assert!(
                matches!(
                    refusal,
                    StartupRefusal::ReachableBindWithDevelopmentIdentity { .. }
                ),
                "`{open}` was refused as {refusal:?} rather than for the development identity",
            );
        }
    }

    /// The development identity is for local work, so loopback must still be admitted — otherwise
    /// the safe configuration is also the unusable one, and the rule gets worked around.
    #[test]
    fn loopback_is_admitted_with_the_development_identity() {
        for local in ["127.0.0.1:8080", "[::1]:8080"] {
            assert!(admit_bind(addr(local), IdentityBinding::Development).is_ok());
        }
    }

    /// The two reachable-bind refusals have opposite remedies — add a provider, or remove the one
    /// you have — so each must name its own.
    #[test]
    fn the_development_refusal_names_the_opposite_remedy() {
        let message = admit_bind(addr("0.0.0.0:8080"), IdentityBinding::Development)
            .expect_err("a reachable development bind is refused")
            .to_string();

        assert!(message.contains("0.0.0.0:8080"), "{message}");
        assert!(message.contains(DEV_IDENTITY_ENV), "{message}");
        assert!(message.contains("unset"), "{message}");

        let unbound = admit_bind(addr("0.0.0.0:8080"), IdentityBinding::Unbound)
            .expect_err("a reachable unbound bind is refused")
            .to_string();

        assert_ne!(
            message, unbound,
            "an operator who armed a dev identity and one who armed nothing must not be told the \
             same thing",
        );
    }

    #[test]
    fn the_default_bind_is_loopback() {
        assert!(DEFAULT_BIND.ip().is_loopback());
        assert_eq!(DEFAULT_BIND.to_string(), "127.0.0.1:8080");
    }
}
