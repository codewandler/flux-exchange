use std::sync::atomic::{AtomicU64, Ordering};

use exchange_host::{
    editor_catalog, validate_workflow, PureEditorTools, Tenant, ToolRegistry, WorkflowEdit,
    WorkflowRefusal, WorkflowStore,
};

static NEXT_STORE: AtomicU64 = AtomicU64::new(1);

fn store() -> WorkflowStore {
    let suffix = NEXT_STORE.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "flux-exchange-workflow-test-{}-{suffix}",
        std::process::id()
    ));
    WorkflowStore::bind(&root).expect("a store outside the checkout binds")
}

fn pure_tools() -> PureEditorTools {
    let mut tools = ToolRegistry::new();
    flux_tools::cognition::try_register_cognition(&mut tools)
        .expect("the upstream cognition pack registers");
    PureEditorTools::new(tools).expect("the cognition pack is pure")
}

fn valid(source: &str) -> exchange_host::ValidatedWorkflow {
    validate_workflow(
        WorkflowEdit::Source {
            source: source.to_string(),
        },
        None,
        &pure_tools(),
    )
    .expect("the source validates")
}

#[test]
fn drafts_are_scoped_by_the_resolved_tenant() {
    let store = store();
    let acme = Tenant::new("acme").unwrap();
    let other = Tenant::new("other").unwrap();

    store
        .create(
            &acme,
            "triage",
            "Triage",
            valid("flow triage\n  return true\n"),
        )
        .unwrap();

    assert_eq!(store.list(&acme).unwrap().len(), 1);
    assert!(store.list(&other).unwrap().is_empty());
    assert!(matches!(
        store.get(&other, "triage"),
        Err(WorkflowRefusal::UnknownWorkflow(_))
    ));
}

#[test]
fn a_stale_draft_revision_is_refused_with_the_current_revision() {
    let store = store();
    let tenant = Tenant::new("acme").unwrap();
    store
        .create(
            &tenant,
            "triage",
            "Triage",
            valid("flow triage\n  return true\n"),
        )
        .unwrap();

    let saved = store
        .save(
            &tenant,
            "triage",
            1,
            "Triage better",
            valid("flow triage\n  return false\n"),
        )
        .unwrap();
    assert_eq!(saved.revision, 2);

    assert!(matches!(
        store.save(
            &tenant,
            "triage",
            1,
            "stale",
            valid("flow triage\n  return true\n"),
        ),
        Err(WorkflowRefusal::RevisionConflict {
            expected: 1,
            current: 2
        })
    ));
}

#[test]
fn publication_is_immutable_while_the_draft_keeps_moving() {
    let store = store();
    let tenant = Tenant::new("acme").unwrap();
    store
        .create(
            &tenant,
            "triage",
            "Triage",
            valid("flow triage\n  return true\n"),
        )
        .unwrap();

    let first = store.publish(&tenant, "triage", 1).unwrap();
    store
        .save(
            &tenant,
            "triage",
            1,
            "Triage",
            valid("flow triage\n  return false\n"),
        )
        .unwrap();

    let frozen = store.version(&tenant, "triage", 1).unwrap();
    assert_eq!(first, frozen);
    assert!(frozen.source.contains("return true"));
    assert!(store
        .get(&tenant, "triage")
        .unwrap()
        .source
        .contains("return false"));

    let second = store.publish(&tenant, "triage", 2).unwrap();
    assert_eq!(second.version, 2);
    assert!(second.source.contains("return false"));
    assert!(store
        .version(&tenant, "triage", 1)
        .unwrap()
        .source
        .contains("return true"));
}

#[test]
fn comments_remain_exact_and_source_only() {
    let source = "flow triage\n  # keep this operator note\n  return true\n";
    let checked = valid(source);

    assert_eq!(checked.source, source);
    assert!(checked.graph.is_none());
    assert_eq!(checked.diagnostics[0].code, "editor.source_trivia");
}

#[test]
fn the_editor_catalogue_is_connectors_plus_pure_cognition_only() {
    let catalog = editor_catalog(&pure_tools()).unwrap();

    assert!(catalog
        .iter()
        .any(|operation| operation.kind == "connector"));
    assert!(catalog
        .iter()
        .any(|operation| operation.kind == "cognition"));
    assert!(catalog
        .iter()
        .filter(|operation| operation.kind == "cognition")
        .all(|operation| operation.risk == "low"
            && operation.idempotency == "idempotent"
            && operation.effects.is_empty()
            && operation.access.is_empty()));
    assert!(catalog
        .iter()
        .all(|operation| operation.kind == "connector" || operation.kind == "cognition"));
}
