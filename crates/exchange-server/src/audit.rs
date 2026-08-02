//! Structured records of authority successfully exercised.
//!
//! These functions deliberately do not accept credential values, setting values, tokens, request
//! bodies or OIDC material. A caller can log a non-secret catalogue/address label only through the
//! narrow signature for that action, which makes “never the material” structural rather than a
//! convention attached to each handler.

use exchange_host::Principal;
use tracing::info;

pub(crate) fn signed_in(principal: &Principal) {
    info!(audit = true, action = "signed_in", actor = %principal, "authority exercised");
}

pub(crate) fn signed_out(principal: &Principal) {
    info!(audit = true, action = "signed_out", actor = %principal, "authority exercised");
}

pub(crate) fn agent_minted(actor: &Principal, agent: &Principal) {
    info!(audit = true, action = "agent_minted", actor = %actor, target = %agent, "authority exercised");
}

pub(crate) fn connection_created(actor: &Principal, connector: &str) {
    info!(audit = true, action = "connection_created", actor = %actor, connector, "authority exercised");
}

pub(crate) fn credential_rotated(actor: &Principal, connector: &str, credential: &str) {
    info!(audit = true, action = "credential_rotated", actor = %actor, connector, credential, "authority exercised");
}

pub(crate) fn connection_removed(actor: &Principal, connector: &str) {
    info!(audit = true, action = "connection_removed", actor = %actor, connector, "authority exercised");
}

pub(crate) fn setting_set(actor: &Principal, connector: &str, service: &str, field: &str) {
    info!(audit = true, action = "setting_set", actor = %actor, connector, service, field, "authority exercised");
}

pub(crate) fn setting_cleared(actor: &Principal, connector: &str, service: &str, field: &str) {
    info!(audit = true, action = "setting_cleared", actor = %actor, connector, service, field, "authority exercised");
}

pub(crate) fn grants_replaced(actor: &Principal, count: usize) {
    info!(audit = true, action = "grants_replaced", actor = %actor, count, "authority exercised");
}

pub(crate) fn invocation_completed(actor: &Principal, operation: &str) {
    info!(audit = true, action = "invocation_completed", actor = %actor, operation, "authority exercised");
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{Arc, Mutex};

    use exchange_host::{PrincipalKind, Tenant};
    use tracing_subscriber::layer::SubscriberExt as _;

    #[derive(Clone, Default)]
    struct Events(Arc<Mutex<Vec<String>>>);

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Events {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _: tracing_subscriber::layer::Context<'_, S>,
        ) {
            let mut rendered = String::new();
            event.record(&mut Fields(&mut rendered));
            self.0.lock().expect("no test poisons this").push(rendered);
        }
    }

    struct Fields<'a>(&'a mut String);

    impl tracing::field::Visit for Fields<'_> {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            use std::fmt::Write as _;
            let _ = write!(self.0, "{}={value:?} ", field.name());
        }
    }

    #[test]
    fn every_success_kind_has_a_stable_structured_event_and_no_material_argument() {
        let actor = Principal::new(
            PrincipalKind::User,
            "alice",
            Tenant::new("acme").expect("a literal tenant"),
        );
        let agent = Principal::new(
            PrincipalKind::Agent,
            "triage",
            Tenant::new("acme").expect("a literal tenant"),
        );
        let events = Events::default();
        let _subscriber =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(events.clone()));

        signed_in(&actor);
        signed_out(&actor);
        agent_minted(&actor, &agent);
        connection_created(&actor, "github");
        credential_rotated(&actor, "github", "token");
        connection_removed(&actor, "github");
        setting_set(&actor, "zendesk", "default", "endpoint.subdomain");
        setting_cleared(&actor, "zendesk", "default", "endpoint.subdomain");
        grants_replaced(&actor, 2);
        invocation_completed(&actor, "github-repo-get");

        let rendered = events.0.lock().expect("no test poisons this").join("\n");
        for action in [
            "signed_in",
            "signed_out",
            "agent_minted",
            "connection_created",
            "credential_rotated",
            "connection_removed",
            "setting_set",
            "setting_cleared",
            "grants_replaced",
            "invocation_completed",
        ] {
            assert!(
                rendered.contains(&format!("action={action:?}")),
                "missing audit action {action}: {rendered}"
            );
        }
        assert_eq!(
            events.0.lock().expect("no test poisons this").len(),
            10,
            "one event per successful authority exercise"
        );
    }
}
