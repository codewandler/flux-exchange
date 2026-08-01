//! The `flux-exchange` binary — one composition of [`exchange_host`].
//!
//! # What it serves
//!
//! An HTTP surface bound to loopback by default: a health route, and the connector catalogue —
//! what this binary *could* run, never what a caller may run, which is why every operation it
//! serves carries `admitted: null`. It holds no credential, binds no identity provider and runs no
//! operation yet; the README carries the itemized inventory of what is not built.
//!
//! What it does carry is the rule that makes the rest safe to add: **a reachable bind with no way to
//! resolve a principal is refused at startup**, not warned about and served anyway. See
//! [`bind::admit_bind`] and `docs/designs/http-surface.md`.

mod bind;
mod connection_guard;
mod dev_identity;
mod routes;
mod session;
mod state;

use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;

use exchange_host::{Deployment, Runtime};
use tokio::net::TcpListener;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use crate::bind::{admit_bind, StartupRefusal, BIND_ENV, DEFAULT_BIND};
use crate::dev_identity::{DevIdentity, DEV_IDENTITY_ENV};
use crate::state::AppState;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    match serve().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(refusal) => {
            // The refusal is the product here: it names the address it would not serve and what
            // would have worked, so an operator does not have to guess which half to change.
            error!("{refusal}");
            ExitCode::FAILURE
        }
    }
}

/// Start the server, or refuse and say why.
async fn serve() -> Result<(), StartupRefusal> {
    let state = compose()?;
    let bind = configured_bind()?;

    // Before the socket, not after: a listener that opens and then closes has still been open.
    admit_bind(bind, state.identity_binding())?;

    report_deployments();
    report_surface();

    let listener = TcpListener::bind(bind)
        .await
        .map_err(|source| StartupRefusal::BindUnavailable { bind, source })?;
    let local = listener
        .local_addr()
        .map_err(|source| StartupRefusal::BindUnavailable { bind, source })?;

    info!(%local, "flux-exchange is listening");

    axum::serve(listener, routes::app(state))
        .with_graceful_shutdown(stop_requested())
        .await
        .map_err(|source| StartupRefusal::Serving { source })
}

/// Bind the ports this composition serves with: an identity provider, and a credential store.
///
/// Both are independent, both default to *bound to nothing*, and neither has a fallback. The
/// argument is the same in each case and it is the one that decides the shape: a safety property
/// that depends on a setting staying at its default is one setting away from gone, so nothing has
/// to be turned off to be safe — only turned on to be useful.
fn compose() -> Result<AppState, StartupRefusal> {
    let state = compose_identity()?;

    match credential_store()? {
        Some(store) => Ok(state.with_credentials(store)),
        None => Ok(state),
    }
}

/// Bind the credential store the environment names, or bind none.
///
/// The same three states as the development identity, and for the same reason. **Unset binds
/// nothing**, and every route that would have used a store then refuses with `503` naming this
/// setting — which is not the in-memory fallback X-09 refuses, because nothing is served from
/// somewhere else; the host says it cannot hold a credential. **Set and unusable refuses to
/// start**, since a store the operator named and this process could not open is a mistake with no
/// later moment at which it announces itself.
///
/// `#[cfg(unix)]` because `CredentialStore` is: what protects a value in the file store is `0600`
/// and `0700`, and a platform that cannot spell those would get a store implying a safety it does
/// not have. The *port* is not gated, so another platform's composition binds its own.
#[cfg(unix)]
fn credential_store() -> Result<Option<Arc<dyn exchange_host::SecretStore>>, StartupRefusal> {
    use exchange_host::{CredentialStore, CREDENTIAL_STORE_SETTING};

    let Ok(configured) = std::env::var(CREDENTIAL_STORE_SETTING) else {
        warn!(
            "no credential store is bound ({CREDENTIAL_STORE_SETTING} is unset), so connecting a \
             connector will refuse. Set it to a path outside every working tree to hold \
             credentials",
        );
        return Ok(None);
    };

    let store = CredentialStore::bind_configured(Some(&configured)).map_err(|source| {
        StartupRefusal::CredentialStore {
            reason: source.to_string(),
        }
    })?;

    // Read back off the bound store, so this line cannot name a file this process did not open.
    info!("{}", store.banner());

    Ok(Some(store.secrets()))
}

/// No file store on this platform; a composition here binds its own or holds none.
#[cfg(not(unix))]
fn credential_store() -> Result<Option<Arc<dyn exchange_host::SecretStore>>, StartupRefusal> {
    Ok(None)
}

/// Bind the identity port this composition serves with.
///
/// There is exactly one on offer and it is the development one, armed only by
/// [`DEV_IDENTITY_ENV`] being set. Unset is the default and the default binds nothing, which is
/// the state a reachable bind is already refused in. A real provider is X-04.
fn compose_identity() -> Result<AppState, StartupRefusal> {
    let Some(dev) = DevIdentity::armed()? else {
        return Ok(AppState::without_identity());
    };

    // Named as such at startup, and at `warn` rather than `info`: this line is the difference
    // between a host whose principals are authenticated and one whose principals are asserted, and
    // an operator who scrolls past it should still have seen it.
    let roster: Vec<String> = dev
        .roster()
        .map(|(handle, principal)| format!("{handle} -> {principal}"))
        .collect();

    warn!(
        roster = %roster.join(", "),
        "DEVELOPMENT identity armed by {DEV_IDENTITY_ENV}. Any caller presenting one of these \
         handles becomes that principal, with no secret required. This host will refuse to serve \
         on any address but loopback while it is armed",
    );

    Ok(AppState::with_development_identity(Arc::new(dev)))
}

/// Where to listen, from the environment or from the loopback default.
fn configured_bind() -> Result<SocketAddr, StartupRefusal> {
    let Ok(configured) = std::env::var(BIND_ENV) else {
        return Ok(DEFAULT_BIND);
    };

    configured
        .parse()
        .map_err(|source| StartupRefusal::UnreadableBind {
            value: configured,
            source,
        })
}

/// Wait for the operator to ask for a stop, so in-flight requests finish rather than being cut.
async fn stop_requested() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        // Never returns: a process that cannot hear its own stop signal must keep serving rather
        // than read the failure as "stop now" and drop every request in flight.
        error!(%error, "cannot listen for ctrl-c; graceful shutdown is unavailable");
        std::future::pending::<()>().await;
    }
}

/// Log the surface this process publishes, and which part of it needs a principal.
fn report_surface() {
    for (module, route) in routes::published() {
        info!(
            module = module.name,
            path = route.path,
            access = ?route.access,
            "route",
        );
    }
}

/// Log which runtimes each deployment shape would serve, and which it refuses.
///
/// Kept from the pre-service binary deliberately. It is the one line of startup output that shows
/// the multi-tenancy rule is in force and decided from the manifest, rather than from a setting
/// somebody could have turned off.
fn report_deployments() {
    const RUNTIMES: [Runtime; 6] = [
        Runtime::Http,
        Runtime::Socket,
        Runtime::Process,
        Runtime::Container,
        Runtime::Plugin,
        Runtime::Remote,
    ];

    for deployment in [Deployment::SingleTenant, Deployment::MultiTenant] {
        let (served, refused): (Vec<_>, Vec<_>) = RUNTIMES
            .iter()
            .partition(|runtime| deployment.admits(**runtime).is_ok());

        info!(
            deployment = ?deployment,
            serves = %names(&served),
            refuses = %names(&refused),
            "runtime admission",
        );
    }
}

fn names(runtimes: &[&Runtime]) -> String {
    if runtimes.is_empty() {
        return "nothing".to_string();
    }

    runtimes
        .iter()
        .map(|runtime| format!("{runtime:?}").to_lowercase())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::ErrorKind;

    use tokio::net::TcpStream;

    /// Send a whole request.
    ///
    /// Spelled against tokio's readiness API rather than `AsyncWriteExt` because this crate does not
    /// carry tokio's `io-util` feature, and the manifest is not this story's to change.
    async fn send(stream: &TcpStream, mut remaining: &[u8]) {
        while !remaining.is_empty() {
            stream
                .writable()
                .await
                .expect("the socket becomes writable");

            match stream.try_write(remaining) {
                Ok(written) => remaining = &remaining[written..],
                Err(error) if error.kind() == ErrorKind::WouldBlock => continue,
                Err(error) => panic!("writing the request failed: {error}"),
            }
        }
    }

    /// Read until the server closes the connection, which `Connection: close` makes it do.
    async fn receive(stream: &TcpStream) -> String {
        let mut received = Vec::new();
        let mut chunk = [0_u8; 1024];

        loop {
            stream
                .readable()
                .await
                .expect("the socket becomes readable");

            match stream.try_read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => received.extend_from_slice(&chunk[..read]),
                Err(error) if error.kind() == ErrorKind::WouldBlock => continue,
                Err(error) => panic!("reading the response failed: {error}"),
            }
        }

        String::from_utf8_lossy(&received).into_owned()
    }

    /// End to end over a real socket. The router tests prove what the surface answers; this proves
    /// the process listens where the default says it does, and that a plain HTTP request to it gets
    /// an answer.
    ///
    /// Port `0` rather than the default `8080`: the address under test is the *interface*, and a
    /// fixed port would make this test fail whenever anything else on the machine holds it.
    #[tokio::test]
    async fn health_answers_over_a_socket_on_the_default_interface() {
        let bind = SocketAddr::new(DEFAULT_BIND.ip(), 0);
        assert!(bind.ip().is_loopback(), "the default bind must be loopback");

        let listener = TcpListener::bind(bind).await.expect("loopback is bindable");
        let local = listener
            .local_addr()
            .expect("a bound listener has an address");
        let server = tokio::spawn(async move {
            axum::serve(listener, routes::app(AppState::without_identity())).await
        });

        let stream = TcpStream::connect(local)
            .await
            .expect("the server is listening");
        send(
            &stream,
            format!("GET /health HTTP/1.1\r\nHost: {local}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await;

        let response = receive(&stream).await;

        server.abort();

        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        assert!(response.contains(r#""status":"ok""#), "{response}");
    }

    /// The bind an operator gets when they set nothing.
    #[test]
    fn the_default_is_used_when_nothing_is_configured() {
        // Only meaningful with the variable unset, which is the state a test process runs in unless
        // something set it; assert the branch rather than mutating the process environment, which
        // would race the other tests in this binary.
        assert!(
            std::env::var(BIND_ENV).is_err(),
            "{BIND_ENV} must not be set"
        );
        assert_eq!(
            configured_bind().expect("an unset variable falls back to the default"),
            DEFAULT_BIND,
        );
    }
}
