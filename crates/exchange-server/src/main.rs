//! The `flux-exchange` binary — one composition of [`exchange_host`].
//!
//! # What it serves
//!
//! An HTTP surface with a health route, bound to loopback by default. It holds no credential, binds
//! no identity provider and runs no operation yet; the README carries the itemized inventory of what
//! is not built.
//!
//! What it does carry is the rule that makes the rest safe to add: **a reachable bind with no way to
//! resolve a principal is refused at startup**, not warned about and served anyway. See
//! [`bind::admit_bind`] and `docs/designs/http-surface.md`.

mod bind;
mod routes;
mod state;

use std::net::SocketAddr;
use std::process::ExitCode;

use exchange_host::{Deployment, Runtime};
use tokio::net::TcpListener;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use crate::bind::{admit_bind, StartupRefusal, BIND_ENV, DEFAULT_BIND};
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
    let state = AppState::without_identity();
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
