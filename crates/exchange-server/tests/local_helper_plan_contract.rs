#[path = "../src/local_helper_plan.rs"]
mod local_helper_plan;

use local_helper_plan::{VendorBegin, VendorOperation};

const OLD_HEAD: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const CURRENT_HEAD: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const PLAN_REVISION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const NAME_REVISION: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const ORIGIN_REVISION: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const TOKEN_REVISION: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const REFRESH_REVISION: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

fn credential_begin() -> Vec<u8> {
    format!(
        "{{\"action\":\"rotate\",\"connector\":\"gitlab\",\"credential_revision\":\"{OLD_HEAD}\",\"label\":\"work\",\"plan_revision\":\"{PLAN_REVISION}\",\"targets\":[{{\"revision\":\"{TOKEN_REVISION}\",\"target\":\"credential.token\"}},{{\"revision\":\"{REFRESH_REVISION}\",\"target\":\"credential.refresh\"}}]}}"
    )
    .into_bytes()
}

fn connect_begin() -> Vec<u8> {
    format!(
        "{{\"authorities\":[{{\"revision\":null,\"target\":\"authority.origin\"}}],\"connector\":\"gitlab\",\"label\":\"new-work\",\"plan_revision\":\"{PLAN_REVISION}\",\"settings\":[{{\"target\":\"authority.origin\",\"value\":\"https://gitlab.example\"}}],\"targets\":[{{\"revision\":\"{NAME_REVISION}\",\"target\":\"connection.name\"}},{{\"revision\":\"{ORIGIN_REVISION}\",\"target\":\"authority.origin\"}},{{\"revision\":\"{TOKEN_REVISION}\",\"target\":\"credential.token\"}},{{\"revision\":\"{REFRESH_REVISION}\",\"target\":\"credential.refresh\"}}]}}"
    )
    .into_bytes()
}

fn plan() -> serde_json::Value {
    serde_json::json!({
        "connector": "gitlab",
        "credential_revision": CURRENT_HEAD,
        "fields": [
            field("connection.name", false, true, Some(("connection.name", NAME_REVISION)), None),
            field("origin", false, true, Some(("authority.origin", ORIGIN_REVISION)), Some(serde_json::json!({"actions":["revoke"],"revision":"1","state":"approved"}))),
            field("token", true, true, Some(("credential.token", TOKEN_REVISION)), None),
            field("refresh", true, true, Some(("credential.refresh", REFRESH_REVISION)), None)
        ],
        "labels": ["work"],
        "plan_revision": PLAN_REVISION,
        "selection": "work",
        "state": "complete",
        "vendor": "GitLab",
        "version": "exchange.connection-plan.v2"
    })
}

fn unselected_plan() -> serde_json::Value {
    let mut plan = plan();
    plan["credential_revision"] = serde_json::Value::Null;
    plan["selection"] = serde_json::Value::Null;
    plan["state"] = serde_json::json!("incomplete");
    plan["fields"][0]["set"] = serde_json::json!(false);
    plan["fields"][1]["set"] = serde_json::json!(false);
    plan["fields"][1]["authority"] =
        serde_json::json!({"actions":[],"revision":null,"state":"unset"});
    plan
}

fn field(
    identity: &str,
    secret: bool,
    required: bool,
    target: Option<(&str, &str)>,
    authority: Option<serde_json::Value>,
) -> serde_json::Value {
    serde_json::json!({
        "aliases": if secret { serde_json::json!([]) } else { serde_json::json!([format!("--{identity}")]) },
        "also_binds": [],
        "authority": authority,
        "binds": null,
        "choices": null,
        "help": "help",
        "identity": identity,
        "input": if secret { "password" } else { "text" },
        "label": identity,
        "name": identity,
        "provenance": "connector",
        "reason": null,
        "required": required,
        "routable": true,
        "secret": secret,
        "service": null,
        "set": if secret { serde_json::Value::Null } else { serde_json::Value::Bool(true) },
        "target": target.map(|(id, revision)| serde_json::json!({"id":id,"revision":revision}))
    })
}

#[test]
fn helper_admits_a_noncurrent_old_head_for_server_side_replay_lookup() {
    let begin = VendorBegin::parse(&credential_begin(), VendorOperation::Credential)
        .expect("closed credential BEGIN");
    assert!(begin.admits_plan(&serde_json::to_vec(&plan()).expect("plan")));
}

#[test]
fn connect_revalidates_the_unselected_plan_and_complete_target_partition() {
    let begin = VendorBegin::parse(&connect_begin(), VendorOperation::Connect)
        .expect("closed connect BEGIN");
    assert_eq!(begin.connector(), "gitlab");
    assert_eq!(begin.label(), "new-work");
    assert!(begin.admits_plan(&serde_json::to_vec(&unselected_plan()).expect("plan")));
}

#[test]
fn every_plan_projection_fact_is_revalidated_before_connection_two() {
    let begin = VendorBegin::parse(&credential_begin(), VendorOperation::Credential)
        .expect("closed credential BEGIN");
    let original = plan();
    assert!(begin.admits_plan(&serde_json::to_vec(&original).expect("plan")));

    let mutations = [
        "/connector",
        "/credential_revision",
        "/labels/0",
        "/plan_revision",
        "/selection",
        "/fields/0/target/revision",
        "/fields/1/authority/actions/0",
        "/fields/1/authority/revision",
        "/fields/1/authority/state",
        "/fields/1/target/revision",
        "/fields/2/secret",
        "/fields/2/set",
        "/fields/2/target/revision",
        "/fields/3/target/revision",
        "/state",
        "/version",
    ];
    for pointer in mutations {
        let mut changed = original.clone();
        *changed.pointer_mut(pointer).expect("mutation pointer") = serde_json::json!("mutated");
        assert!(
            !begin.admits_plan(&serde_json::to_vec(&changed).expect("mutated plan")),
            "helper admitted mutated plan fact {pointer}"
        );
    }

    let mut reordered = original;
    reordered["fields"]
        .as_array_mut()
        .expect("plan fields")
        .swap(2, 3);
    assert!(
        !begin.admits_plan(&serde_json::to_vec(&reordered).expect("reordered plan")),
        "helper admitted a reordered credential target projection"
    );
}
