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
//! Three settings in [`invoker`] are safety decisions rather than preferences: two fields of
//! [`WebOptions`], flux's egress envelope, and the [`Sandbox`] posture of the [`System`] every
//! [`ToolContext`] is built over. Each is set to the strictest available value and **each is
//! written out longhand**, because a property that depends on a default staying where it is, is one
//! edit away from gone. The sandbox was the one that used to be inherited; X-48 wrote it out.

use std::sync::Arc;

use exchange_host::{
    ConfigStore, Contexts, Deployment, Egress, Grants, Invoker, SecretStore, ToolContext,
};
use flux_system::sandbox::{Sandbox, SandboxMode, SandboxSettings};
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
/// `grants` is **not** optional in the same way, and the difference is the whole of X-13. A missing
/// settings store costs a tenant thirteen connectors; a missing grant store would cost the gate that
/// decides what may run at all, and the only plausible thing to do without one is to admit
/// everything — which is the exposure this story closed. So there is no empty default here: the
/// binary binds a store or builds no invoker, and `POST /api/operations/{operation}/invoke` refuses
/// with `503` naming the setting.
///
/// # Errors
///
/// When the process's working directory cannot be made into a flux workspace. That is the only
/// fallible step, and it is fallible at *startup* rather than on the first request, which is where
/// a composition problem should announce itself.
pub fn invoker(
    credentials: Arc<dyn SecretStore>,
    settings: Arc<dyn ConfigStore>,
    grants: Arc<dyn Grants>,
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
    // operation and its egress, and neither touches the filesystem. It exists because
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
        // **What each tenant may run** (X-13), as the port. Read at the same tenant, off the same
        // resolved principal, and consulted before anything reads a credential — so a call this
        // tenant holds no grant for is refused without a secret having been moved into this
        // process's memory for it.
        grants,
        Arc::new(GuardedSystem {
            system: Arc::new(guarded_system(workspace)),
        }),
    ))
}

/// The guarded IO handle every [`ToolContext`] is built over, **with its sandbox posture stated**.
///
/// A named function rather than an expression inside [`invoker`], so the posture is a value a test
/// can hold — [`tests::the_sandbox_posture_is_chosen_and_not_inherited`] asserts what this returns.
///
/// Be exact about what that buys, because the first draft of this sentence was not. **The test
/// pins this function, not [`invoker`]'s call to it.** Reverting `invoker` to a bare
/// `System::new(workspace)` would leave the test green. What catches *that* is a different
/// mechanism: this function would then have no non-test caller, which is `dead_code`, which is an
/// error under the gate's `clippy --all-targets -- -D warnings`. Two mechanisms, one of them not
/// the one the shape of this code suggests.
///
/// # Why `Require`, written out field by field
///
/// `System::new` alone leaves `Sandbox::disabled()`, and upstream's own doc on that constructor
/// says production entry points should use `from_env`/`with_sandbox` instead — it is the shape
/// hermetic tests want. Inheriting it in [`invoker`] was the same mistake `allowed_secrets` and
/// `private_net` are written out longhand a few lines earlier to avoid.
///
/// `Require` is the strict end, with three outcomes rather than the two it is easy to state:
///
/// 1. a real backend (bubblewrap, Seatbelt) confines the child;
/// 2. no usable backend, and `Sandbox::ensure_available` **refuses the spawn** — *refuse; never
///    repair*, applied to the one capability this composition holds and does not use;
/// 3. a truthy `FLUX_SANDBOXED` in this process's environment resolves `Backend::AlreadyConfined`
///    **without probing**, and satisfies `Require` with no backend of this process's own. That is
///    upstream's inheritance marker for "an outer flux sandbox already confines this whole tree",
///    and it is operator-controlled ambient state: nothing on a request path can set it, but an
///    operator who exports it by accident gets outcome 3 where they expected outcome 2.
///
/// `network: false` narrows what a **sandboxed child** may dial and is unrelated to `private_net`,
/// which guards the requests this host actually sends. `extra_writable` is empty because widening a
/// writable path is a decision and nobody has made one. Every field is named rather than spread
/// from a default, so a field upstream adds is a compile error here rather than one more inherited
/// posture.
///
/// # What this is **not**
///
/// It is not a claim that nothing can spawn — that claim used to sit at this call site and it was
/// false. [`ToolContext::system`](exchange_host::ToolContext::system) hands any holder of a context
/// this `System`, whose `run`/`run_with_env` spawn processes and whose `read_file`/`write_file`
/// reach the workspace root; an unbound `spawner` closes the *sub-agent* seam, which is a different
/// door.
///
/// Nor does it cover *every* spawn, which is the other easy overstatement. `System::build_command`
/// consults `Sandbox::ensure_available` only for `Confinement::Sandboxed`, so the exempt paths —
/// `run_exempt`, `spawn_debug_pipe` — skip the posture entirely. The claim is about
/// `run`/`run_with_env` and their streamed forms, and no further.
///
/// What holds the rest is `crates/exchange-host/tests/no_second_request_path.rs`, which bounds
/// which files in the dispatching crate may hold a `ToolContext` at all. Read its "What lock 2 is,
/// and what it is not" for the boundary that matters here: **this function is in
/// `exchange-server` and that scanner never reads it**, so this posture is a property of this
/// repository's composition and not of the published crate. A downstream binary implements
/// `Contexts` itself and gets whatever `System` it built.
fn guarded_system(workspace: Workspace) -> System {
    System::new(workspace).with_sandbox(Sandbox::resolve(SandboxSettings {
        mode: SandboxMode::Require,
        network: false,
        extra_writable: Vec::new(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The finding X-48 answers.** The sandbox posture is the one this composition chose, not the
    /// one `System::new` happens to hand out.
    ///
    /// Asserted against `Sandbox::disabled()` as well as against the literal mode, because the two
    /// say different things: the literal pins *what was chosen*, and the comparison pins *that a
    /// choice was made at all*.
    ///
    /// What it does **not** pin is that [`invoker`] still calls [`guarded_system`] — this test
    /// holds the helper, and a revert at the call site would leave it green. `dead_code` under the
    /// gate's `-D warnings` is what covers that, and it is named on [`guarded_system`] so the next
    /// reader does not infer a guarantee from the shape of the code.
    #[test]
    fn the_sandbox_posture_is_chosen_and_not_inherited() {
        let workspace = Workspace::new(std::env::temp_dir())
            .expect("the temp directory is a usable workspace root");
        let system = guarded_system(workspace);
        let settings = system.sandbox().settings();

        assert_eq!(
            settings.mode,
            SandboxMode::Require,
            "a spawn this composition cannot confine must refuse rather than run unconfined",
        );
        assert_ne!(
            settings.mode,
            Sandbox::disabled().settings().mode,
            "the sandbox is back to `System::new`'s inherited default, which upstream documents as \
             the hermetic-test constructor and which this composition deliberately does not take",
        );
        assert!(
            !settings.network,
            "a sandboxed child of this process has no business dialling anything; the requests \
             this host sends go through the `Egress`, under `private_net`",
        );
        assert!(
            settings.extra_writable.is_empty(),
            "widening a writable path beyond the workspace is a decision, and this is not where \
             one was made: {:?}",
            settings.extra_writable,
        );
    }
}
