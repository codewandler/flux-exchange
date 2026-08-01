//! The `flux-exchange` binary — one composition of [`exchange_host`].
//!
//! # There is no service here yet
//!
//! This binary starts, reports the deployment it would serve, and exits. It binds no port, holds no
//! credential, and answers no request. That is deliberate and it is not a stub: a scaffold that
//! *looked* like a running service — a health endpoint over an empty host, a fake catalogue — would
//! be indistinguishable from a working one to anyone evaluating the repository, which is a worse
//! outcome than an honest refusal.
//!
//! What it does do is exercise the host's rules, so the repository's central claims are executed
//! rather than merely written down.

use exchange_host::{Deployment, Runtime};

mod catalogue;

fn main() {
    println!("flux-exchange {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("This binary does not serve anything yet. See README.md § Status.");
    println!();

    report(Deployment::SingleTenant);
    report(Deployment::MultiTenant);

    println!();
    println!("The runtime a connector declares is what decides the two lists above.");
    println!("A caller never chooses it — see crates/exchange-host/src/runtime.rs.");
}

/// Print which runtimes a deployment would serve, and which it would refuse.
fn report(deployment: Deployment) {
    const RUNTIMES: [Runtime; 6] = [
        Runtime::Http,
        Runtime::Socket,
        Runtime::Process,
        Runtime::Container,
        Runtime::Plugin,
        Runtime::Remote,
    ];

    let (served, refused): (Vec<_>, Vec<_>) = RUNTIMES
        .iter()
        .partition(|runtime| deployment.admits(**runtime).is_ok());

    println!("{deployment:?}");
    println!("  serves:  {}", names(&served));
    if refused.is_empty() {
        println!("  refuses: nothing");
    } else {
        println!(
            "  refuses: {}  (they execute on this host)",
            names(&refused)
        );
    }
}

fn names(runtimes: &[&Runtime]) -> String {
    runtimes
        .iter()
        .map(|runtime| format!("{runtime:?}").to_lowercase())
        .collect::<Vec<_>>()
        .join(", ")
}
