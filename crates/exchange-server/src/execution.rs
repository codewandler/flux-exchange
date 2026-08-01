//! **This composition's transport.** The one place in this repository that holds an HTTP client.
//!
//! `exchange-host` runs every operation and has no way to send anything: its transport arrives as
//! an [`Egress`], and `crates/exchange-host/tests/no_second_request_path.rs` holds it to that by
//! reading its manifest and scanning its sources. The complementary half is here — this crate names
//! `flux_web` and **never names `connector_pack`**, which the same test asserts. The crate that can
//! build a request cannot name the pack; the crate that names the pack cannot build a request.
//!
//! # What is configured here, and why each choice is the strict one
//!
//! [`WebOptions`] is flux's egress envelope, and two of its fields are safety decisions rather than
//! preferences. Both are set to the strictest available value, because a property that depends on a
//! default staying where it is, is one edit away from gone.

use std::sync::Arc;

use exchange_host::{ConfigStore, Contexts, Deployment, Egress, Invoker, SecretStore, ToolContext};
use flux_system::{System, Workspace};
use flux_web::http::HttpRequestTool;
use flux_web::WebOptions;

/// A fresh [`ToolContext`] per invocation, over one guarded system.
///
/// The **context** is per invocation and the **system** is not, and the split is the point. What
/// must not be shared is the redactor, because a credential registered for one tenant's call must
/// not still be held when the next tenant's result is scrubbed. The `System` is a guarded-IO handle
/// with no per-request state, so minting one per request would buy nothing and cost a workspace
/// probe on every call.
struct GuardedSystem {
    system: Arc<System>,
}

impl Contexts for GuardedSystem {
    fn fresh(&self) -> ToolContext {
        ToolContext::new(self.system.clone())
    }
}

/// Build the invoker this composition serves operations with, or say why it could not.
///
/// `settings` is where a tenant's **non-secret** connection values are read from — the
/// `{subdomain}` in a templated base URL. It arrives as the port rather than as the store for the
/// same reason `credentials` does, and it is a *separate* argument from `credentials` because they
/// are separate stores: a subdomain is not a secret, and `exchange_host::settings` carries the
/// argument for why keeping the two apart is a decision rather than a filing convention.
///
/// A composition that binds no settings store passes an empty one and gets X-12's behaviour
/// unchanged: the seventeen connectors that need a per-connection value refuse by name,
/// quoting the field and the service, and the other thirty-seven run. That is the honest gap rather
/// than a fallback — nothing is served from somewhere else.
///
/// # Errors
///
/// When the process's working directory cannot be made into a flux workspace. That is the only
/// fallible step, and it is fallible at *startup* rather than on the first request, which is where
/// a composition problem should announce itself.
pub fn invoker(
    credentials: Arc<dyn SecretStore>,
    settings: Arc<dyn ConfigStore>,
) -> Result<Invoker, String> {
    let options = WebOptions {
        // **Deny-all**, explicitly. `None` here does not mean "no secrets": it means "fall back to
        // the `FLUX_WEB_SECRET_ALLOW` environment variable", which would let a `{"$secret": "NAME"}`
        // header reference resolve a value out of *this process's* environment and put it on a
        // tenant's request. Nothing this host sends carries such a reference — `connector-pack`
        // assembles every credential itself from the bound store — so the correct list is empty,
        // and saying so is what stops an operator's unrelated `FLUX_WEB_SECRET_ALLOW` from
        // widening it.
        allowed_secrets: Some(Vec::new()),
        // The full SSRF guard: private, loopback, link-local and cloud-metadata addresses are
        // refused, including a public hostname that resolves to one. This is `PrivateNetAllow`'s
        // default and it is written out rather than inherited, because "the default happened to be
        // strict" is not a property anybody can rely on.
        private_net: flux_system::net::PrivateNetAllow::None,
        ..WebOptions::default()
    };

    // A workspace nothing in this path reads. The registry an invocation resolves holds exactly one
    // operation and its egress; neither touches the filesystem, and `ToolContext`'s spawner is left
    // unbound, so no process can be spawned through it either. It exists because
    // `ToolContext::new` takes a `System` and there is no constructor that does not.
    let root = std::env::current_dir()
        .map_err(|error| format!("the working directory is unreadable: {error}"))?;
    let workspace = Workspace::new(&root).map_err(|error| {
        format!(
            "`{}` is not a usable workspace root: {error}",
            root.display()
        )
    })?;

    Ok(Invoker::new(
        // **Multi-tenant**, which is the class that refuses more. This host serves many principals
        // over a socket, so it is multi-tenant by construction; and even if it were not, choosing
        // the permissive class as a default would make "a locally-executing runtime is refused" a
        // property that depends on a setting nobody set. Every shipped connector declares `http`,
        // which this admits — see `exchange_host::admit_runtime`.
        Deployment::MultiTenant,
        Egress::new(Arc::new(HttpRequestTool::new(&options))),
        credentials,
        // **A tenant's non-secret connection settings**, as the port. X-47 gave them a store of
        // their own — deliberately not the credential store, see `exchange_host::settings` — and
        // this is where it reaches the thing that reads it. Nothing a caller supplies can influence
        // which tenant is read: `Invoker::invoke` binds this port and the credential port to one
        // tenant, in one expression, off the resolved principal.
        settings,
        Arc::new(GuardedSystem {
            system: Arc::new(System::new(workspace)),
        }),
    ))
}
