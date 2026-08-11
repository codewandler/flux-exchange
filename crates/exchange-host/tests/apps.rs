use std::collections::{BTreeMap, BTreeSet};

use exchange_host::{
    AccessLayer, AppPackage, AppStore, AvailableConnection, ConnectionRequirement, Datasource,
    DatasourceRequirement, InstallRequest, ModelProfile, PackageProvenance, PackageRegistry,
    PackageRequirements, PackageTrigger, Risk, Selector, Tenant, TriggerTarget,
};

fn tenant(raw: &str) -> Tenant {
    Tenant::new(raw).expect("tenant")
}

fn package(require_datasource: bool) -> AppPackage {
    let source = r#"agent assistant
  model "installed"
  tools ["slack.chat.post.message"]
  description "A tenant-installed Slack assistant."

trigger chat
  on "chat"
  agent assistant
"#;
    AppPackage::signed(
        "exchange-apps/slack-bot",
        "1.0.0",
        source,
        PackageProvenance {
            publisher: "codewandler".into(),
            repository: "https://github.com/codewandler/exchange-apps".into(),
            revision: "0123456789abcdef0123456789abcdef01234567".into(),
        },
        PackageRequirements {
            connections: vec![ConnectionRequirement {
                name: "slack".into(),
                connector: "slack".into(),
            }],
            access_layers: vec![AccessLayer {
                name: "reply".into(),
                connector: "slack".into(),
                selector: Selector::at_most(Risk::High),
                required_operations: vec!["slack-chat-post-message".into()],
                required: true,
            }],
            datasources: if require_datasource {
                vec![DatasourceRequirement {
                    name: "handbook".into(),
                    kind: "knowledge".into(),
                    required: true,
                }]
            } else {
                Vec::new()
            },
            model_profile_required: true,
            triggers: vec![PackageTrigger {
                event_type: "chat".into(),
                target: TriggerTarget::ManagedAgent("assistant".into()),
            }],
        },
    )
    .expect("package")
}

fn request() -> InstallRequest {
    InstallRequest {
        id: "support-bot".into(),
        package: "exchange-apps/slack-bot".into(),
        version: "1.0.0".into(),
        connections: BTreeMap::from([("slack".into(), "workspace".into())]),
        model_profile: "demo".into(),
        access_layers: BTreeSet::from(["reply".into()]),
        datasources: BTreeMap::from([("handbook".into(), "support-docs".into())]),
        risk_ceiling: Risk::High,
        scopes: BTreeSet::from(["chat".into()]),
        review: None,
    }
}

fn registry(package: AppPackage) -> PackageRegistry {
    PackageRegistry::new([package]).expect("curated package registry")
}

#[test]
fn package_event_bindings_must_match_the_programs_exact_target() {
    let mut requirements = package(false).requirements;
    requirements.triggers[0].target = TriggerTarget::ManagedAgent("reviewer".into());
    let refusal = AppPackage::signed(
        "exchange-apps/mismatched",
        "1.0.0",
        r#"agent assistant
  model "installed"

agent reviewer
  model "installed"

trigger chat
  on "chat"
  agent assistant
"#,
        PackageProvenance {
            publisher: "codewandler".into(),
            repository: "https://github.com/codewandler/exchange-apps".into(),
            revision: "0123456789abcdef0123456789abcdef01234567".into(),
        },
        requirements,
    )
    .expect_err("package metadata must not redirect a Program trigger");
    assert!(refusal.to_string().contains("not bound"));
}

#[test]
fn installation_refuses_every_missing_requirement_without_a_partial_binding() {
    let tenant = tenant("acme");
    let store = AppStore::in_memory(registry(package(true)));
    let base = request();

    for (name, prepare, connections) in [
        ("Connection", 0, Vec::<AvailableConnection>::new()),
        (
            "Model Profile",
            0,
            vec![AvailableConnection::for_test("slack", "workspace")],
        ),
        (
            "Datasource",
            1,
            vec![AvailableConnection::for_test("slack", "workspace")],
        ),
    ] {
        let isolated = AppStore::in_memory(registry(package(true)));
        if prepare >= 1 {
            isolated
                .put_model_profile(&tenant, ModelProfile::static_reply("demo", "ready"))
                .expect("profile");
        }
        if prepare >= 2 {
            // Deliberately leave support-docs absent.
        }
        let refusal = isolated
            .install(&tenant, base.clone(), &connections)
            .expect_err("missing requirement must refuse")
            .to_string();
        assert!(refusal.contains(name), "{refusal}");
        assert!(isolated.list(&tenant).expect("list").is_empty());
    }

    store
        .put_model_profile(&tenant, ModelProfile::static_reply("demo", "ready"))
        .expect("profile");
    store
        .put_datasource(&tenant, Datasource::new("support-docs", "wrong-kind"))
        .expect("datasource");
    let refusal = store
        .install(
            &tenant,
            base,
            &[AvailableConnection::for_test("slack", "workspace")],
        )
        .expect_err("wrong datasource kind")
        .to_string();
    assert!(refusal.contains("Datasource"), "{refusal}");
    assert!(store.list(&tenant).expect("list").is_empty());
}

#[test]
fn installation_refuses_a_missing_operation_and_freezes_selector_results() {
    let tenant = tenant("acme");
    let mut missing = package(false);
    missing.requirements.access_layers[0].required_operations = vec!["slack-no-such-op".into()];
    missing.refresh_integrity();
    let store = AppStore::in_memory(registry(missing));
    store
        .put_model_profile(&tenant, ModelProfile::static_reply("demo", "ready"))
        .expect("profile");
    let refusal = store
        .install(
            &tenant,
            request(),
            &[AvailableConnection::for_test("slack", "workspace")],
        )
        .expect_err("missing operation")
        .to_string();
    assert!(refusal.contains("Operation"), "{refusal}");
    assert!(store.list(&tenant).expect("list").is_empty());

    let store = AppStore::in_memory(registry(package(false)));
    store
        .put_model_profile(&tenant, ModelProfile::static_reply("demo", "ready"))
        .expect("profile");
    let installed = store
        .install(
            &tenant,
            request(),
            &[AvailableConnection::for_test("slack", "workspace")],
        )
        .expect("install");
    assert!(installed
        .operations
        .iter()
        .any(|operation| operation.catalogue_id == "slack-chat-post-message"));
    assert!(!installed.review_fingerprint.is_empty());

    let token = store
        .runtime_token(&tenant, "support-bot", "assistant")
        .expect("opaque runtime authority");
    assert!(!format!("{token:?}").contains("acme"));
    assert!(!format!("{token:?}").contains("slack"));
    let authority = store
        .authorize_operation(&token, "slack-chat-post-message")
        .expect("frozen operation");
    assert_eq!(authority.catalogue_id, "slack-chat-post-message");
    assert!(store
        .authorize_operation(&token, "slack-conversations-create")
        .is_err());
}

#[test]
fn a_package_upgrade_that_widens_authority_requires_its_new_review_fingerprint() {
    let tenant = tenant("acme");
    let v1 = package(false);
    let mut v2 = package(false);
    v2.version = "1.1.0".into();
    v2.requirements.access_layers.push(AccessLayer {
        name: "channels".into(),
        connector: "slack".into(),
        selector: Selector::at_most(Risk::High),
        required_operations: vec!["slack-reactions-add".into()],
        required: false,
    });
    v2.refresh_integrity();
    let store = AppStore::in_memory(PackageRegistry::new([v1, v2]).expect("registry"));
    store
        .put_model_profile(&tenant, ModelProfile::static_reply("demo", "ready"))
        .expect("profile");
    let connections = [AvailableConnection::for_test("slack", "workspace")];
    store
        .install(&tenant, request(), &connections)
        .expect("first install");

    let mut upgrade = request();
    upgrade.version = "1.1.0".into();
    upgrade.access_layers.insert("channels".into());
    let refusal = store
        .install(&tenant, upgrade.clone(), &connections)
        .expect_err("widening needs review");
    let fingerprint = refusal
        .required_review()
        .expect("widening refusal carries review fingerprint")
        .to_owned();
    upgrade.review = Some(fingerprint);
    let installed = store
        .install(&tenant, upgrade, &connections)
        .expect("reviewed upgrade");
    assert_eq!(installed.version, "1.1.0");
}

#[test]
fn deliveries_retry_only_when_the_frozen_effect_set_is_safe_and_projections_are_tenant_scoped() {
    let acme = tenant("acme");
    let other = tenant("other");
    let mut safe = package(false);
    safe.program = safe
        .program
        .replace("tools [\"slack.chat.post.message\"]", "tools []");
    safe.requirements.access_layers.clear();
    safe.refresh_integrity();
    let store = AppStore::in_memory(registry(safe));
    for tenant in [&acme, &other] {
        store
            .put_model_profile(tenant, ModelProfile::static_reply("demo", "ready"))
            .expect("profile");
        let mut install = request();
        install.access_layers.clear();
        store
            .install(
                tenant,
                install,
                &[AvailableConnection::for_test("slack", "workspace")],
            )
            .expect("install");
    }

    let delivery = store
        .enqueue_delivery(
            &acme,
            "support-bot",
            "chat",
            serde_json::json!({"text":"secret body"}),
        )
        .expect("delivery");
    store
        .finish_delivery(&acme, &delivery.id, false, "temporary refusal")
        .expect("failure");
    store
        .retry_delivery(&acme, &delivery.id)
        .expect("safe retry");
    let activity = store.activity(&acme).expect("activity");
    assert!(activity
        .iter()
        .any(|event| event.kind == "delivery_retried"));
    let encoded = serde_json::to_string(&activity).expect("activity JSON");
    assert!(!encoded.contains("secret body"));
    assert!(store
        .activity(&other)
        .expect("other activity")
        .iter()
        .all(|event| event.kind == "app_installed"));
}

#[test]
fn an_unsafe_retry_persists_indeterminate_and_the_delivery_view_has_no_payload() {
    let tenant = tenant("acme");
    let store = AppStore::in_memory(registry(package(false)));
    store
        .put_model_profile(&tenant, ModelProfile::static_reply("demo", "ready"))
        .expect("profile");
    store
        .install(
            &tenant,
            request(),
            &[AvailableConnection::for_test("slack", "workspace")],
        )
        .expect("install");
    let delivery = store
        .enqueue_delivery(
            &tenant,
            "support-bot",
            "chat",
            serde_json::json!({"text":"must stay private"}),
        )
        .expect("delivery");
    store
        .finish_delivery(&tenant, &delivery.id, false, "ambiguous vendor outcome")
        .expect("failure");
    assert!(store.retry_delivery(&tenant, &delivery.id).is_err());
    let view = store
        .delivery(&tenant, &delivery.id)
        .expect("delivery view");
    assert_eq!(view.status, "indeterminate");
    assert!(!serde_json::to_string(&view)
        .expect("delivery JSON")
        .contains("must stay private"));
}
