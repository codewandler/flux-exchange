#[path = "../src/local_management/proposal.rs"]
mod proposal;

use proposal::{
    ConnectBegin, CredentialAction, CredentialBegin, CredentialRevision, PlanRevision,
    ProposalDigest, ReceiptId, TargetFact, TargetPartition, TargetRevision,
};

const PLAN: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const NAME_REVISION: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const ORIGIN_REVISION: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const TOKEN_REVISION: &str = "4444444444444444444444444444444444444444444444444444444444444444";
const HEAD: &str = "5555555555555555555555555555555555555555555555555555555555555555";

const CONNECT: &str = r#"{"authorities":[{"revision":null,"target":"authority.origin"}],"connector":"gitlab","label":"work","plan_revision":"1111111111111111111111111111111111111111111111111111111111111111","settings":[{"target":"authority.origin","value":"https://gitlab.example"}],"targets":[{"revision":"2222222222222222222222222222222222222222222222222222222222222222","target":"connection.name"},{"revision":"3333333333333333333333333333333333333333333333333333333333333333","target":"authority.origin"},{"revision":"4444444444444444444444444444444444444444444444444444444444444444","target":"credential.token"}]}"#;

const CREDENTIAL: &str = r#"{"action":"rotate","connector":"gitlab","credential_revision":"5555555555555555555555555555555555555555555555555555555555555555","label":"work","plan_revision":"1111111111111111111111111111111111111111111111111111111111111111","targets":[{"revision":"4444444444444444444444444444444444444444444444444444444444444444","target":"credential.token"}]}"#;

fn universe() -> Vec<TargetFact<'static>> {
    vec![
        TargetFact {
            target: "connection.name",
            revision: NAME_REVISION,
            required: true,
            partition: TargetPartition::ConnectionName,
        },
        TargetFact {
            target: "authority.origin",
            revision: ORIGIN_REVISION,
            required: true,
            partition: TargetPartition::Authority,
        },
        TargetFact {
            target: "credential.token",
            revision: TOKEN_REVISION,
            required: true,
            partition: TargetPartition::Credential,
        },
    ]
}

fn expected_preimage(domain: &str, payload: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(domain.len() + 1 + payload.len());
    bytes.extend_from_slice(domain.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(payload.as_bytes());
    bytes
}

#[test]
fn connect_begin_has_the_exact_closed_preimage_and_digest() {
    let begin = ConnectBegin::parse_and_validate(CONNECT.as_bytes(), &universe())
        .expect("canonical connect BEGIN and complete ordered target closure");

    assert_eq!(begin.canonical_bytes(), CONNECT.as_bytes());
    assert_eq!(begin.connector(), "gitlab");
    assert_eq!(begin.label(), "work");
    assert_eq!(begin.plan_revision(), PLAN);
    assert_eq!(begin.targets().len(), 3);
    assert_eq!(begin.settings()[0].value(), "https://gitlab.example");
    assert_eq!(begin.authorities()[0].revision(), None);
    assert_eq!(
        begin.proposal_preimage(),
        expected_preimage("exchange.local-management.v1.connect-proposal", CONNECT)
    );
    assert_eq!(
        begin.proposal_digest().as_str(),
        "3f2c36ccbddac87bd4df528d33bca9975aa69c5cf188e61eb32c4f96fbb5bcf1"
    );
}

#[test]
fn credential_begin_has_the_exact_closed_preimage_and_digest() {
    let begin = CredentialBegin::parse_and_validate(CREDENTIAL.as_bytes(), &universe())
        .expect("canonical credential BEGIN and complete ordered credential partition");

    assert_eq!(begin.canonical_bytes(), CREDENTIAL.as_bytes());
    assert_eq!(begin.action(), CredentialAction::Rotate);
    assert_eq!(begin.connector(), "gitlab");
    assert_eq!(begin.label(), "work");
    assert_eq!(begin.plan_revision(), PLAN);
    assert_eq!(begin.credential_revision(), HEAD);
    assert_eq!(begin.targets().len(), 1);
    assert_eq!(
        begin.proposal_preimage(),
        expected_preimage(
            "exchange.local-management.v1.credential-proposal",
            CREDENTIAL
        )
    );
    assert_eq!(
        begin.proposal_digest().as_str(),
        "d3dffce094cf9d22275943c8b224b14d5d0ce0b84eed9cd3141f2ef455b22ff5"
    );
}

#[test]
fn opaque_lowerhex_identities_are_closed() {
    assert_eq!(
        CredentialRevision::parse(HEAD)
            .expect("nonzero credential head")
            .as_str(),
        HEAD
    );
    assert!(CredentialRevision::parse("0".repeat(64)).is_err());
    assert!(CredentialRevision::parse("A".repeat(64)).is_err());
    assert_eq!(
        PlanRevision::parse(PLAN).expect("plan revision").as_str(),
        PLAN
    );
    assert_eq!(
        TargetRevision::parse(TOKEN_REVISION)
            .expect("target revision")
            .as_str(),
        TOKEN_REVISION
    );
    assert!(PlanRevision::parse("A".repeat(64)).is_err());
    assert!(TargetRevision::parse("f".repeat(65)).is_err());
    assert!(ProposalDigest::parse("a".repeat(63)).is_err());
    assert!(ProposalDigest::parse("0".repeat(64)).is_ok());
    assert!(ReceiptId::parse("0".repeat(64)).is_err());
    assert_eq!(
        ReceiptId::parse("f".repeat(64))
            .expect("nonzero receipt")
            .as_str(),
        "f".repeat(64)
    );
}

#[test]
fn every_closed_object_shape_mutation_refuses() {
    let connect_cases = [
        CONNECT.replacen(r#","connector":"gitlab""#, "", 1),
        CONNECT.replacen(r#""connector":"gitlab""#, r#""connector":null"#, 1),
        CONNECT.replacen(r#""label":"work""#, r#""label":7"#, 1),
        CONNECT.replacen(r#""settings":["#, r#""settings":null,"#, 1),
        CONNECT.replacen(
            r#""connector":"gitlab""#,
            r#""connector":"gitlab","connector":"gitlab""#,
            1,
        ),
        CONNECT.replacen(
            r#""target":"authority.origin"}"#,
            r#""target":"authority.origin","unknown":null}"#,
            1,
        ),
        CONNECT.replacen(
            r#""label":"work""#,
            r#""label":"work","secret":"must-not-enter-json""#,
            1,
        ),
    ];
    for case in connect_cases {
        assert!(
            ConnectBegin::parse_canonical(case.as_bytes()).is_err(),
            "closed connect mutation was admitted: {case}"
        );
    }

    let credential_cases = [
        CREDENTIAL.replacen(r#""action":"rotate""#, r#""action":null"#, 1),
        CREDENTIAL.replacen(r#""action":"rotate""#, r#""action":"replace""#, 1),
        CREDENTIAL.replacen(
            r#""label":"work""#,
            r#""credential":"must-not-enter-json","label":"work""#,
            1,
        ),
    ];
    for case in credential_cases {
        assert!(
            CredentialBegin::parse_canonical(case.as_bytes()).is_err(),
            "closed credential mutation was admitted: {case}"
        );
    }

    let sentinel = "raw-vendor-secret-sentinel";
    let attempted_reflection = CREDENTIAL.replacen("rotate", sentinel, 1);
    let refusal = CredentialBegin::parse_canonical(attempted_reflection.as_bytes())
        .expect_err("an unknown action must refuse");
    assert_eq!(refusal.to_string(), "invalid proposal control object");
    assert!(!refusal.to_string().contains(sentinel));
}

#[test]
fn canonical_spelling_and_every_identity_position_refuse_mutation() {
    assert!(ConnectBegin::parse_canonical(format!(" {CONNECT}").as_bytes()).is_err());
    assert!(ConnectBegin::parse_canonical(format!("{CONNECT}\n").as_bytes()).is_err());
    assert!(ConnectBegin::parse_canonical(
        CONNECT
            .replacen(
                r#""authorities":["#,
                r#""connector":"gitlab","authorities":["#,
                1,
            )
            .replacen(r#","connector":"gitlab""#, "", 1)
            .as_bytes()
    )
    .is_err());
    assert!(
        ConnectBegin::parse_canonical(CONNECT.replacen(PLAN, &"A".repeat(64), 1).as_bytes())
            .is_err()
    );
    assert!(ConnectBegin::parse_canonical(
        CONNECT
            .replacen(ORIGIN_REVISION, &"a".repeat(63), 1)
            .as_bytes()
    )
    .is_err());
    assert!(CredentialBegin::parse_canonical(
        CREDENTIAL.replacen(HEAD, &"0".repeat(64), 1).as_bytes()
    )
    .is_err());
}

#[test]
fn scalar_and_collection_bounds_refuse_before_target_validation() {
    assert!(ConnectBegin::parse_canonical(
        CONNECT.replacen("gitlab", &"c".repeat(129), 1).as_bytes()
    )
    .is_err());
    assert!(
        ConnectBegin::parse_canonical(CONNECT.replacen("work", &"l".repeat(65), 1).as_bytes())
            .is_err()
    );
    assert!(
        ConnectBegin::parse_canonical(CONNECT.replacen("work", "not valid", 1).as_bytes()).is_err()
    );
    assert!(ConnectBegin::parse_canonical(
        CONNECT
            .replacen("https://gitlab.example", &"v".repeat(1025), 1)
            .as_bytes()
    )
    .is_err());

    let targets = (0..65)
        .map(|index| format!(r#"{{"revision":"{TOKEN_REVISION}","target":"credential.t{index}"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let oversized = format!(
        r#"{{"action":"rotate","connector":"gitlab","credential_revision":"{HEAD}","label":"work","plan_revision":"{PLAN}","targets":[{targets}]}}"#
    );
    assert!(CredentialBegin::parse_canonical(oversized.as_bytes()).is_err());
}

#[test]
fn connect_target_closure_rejects_omission_extra_duplicate_order_partition_and_revision() {
    let omitted_origin = CONNECT
        .replacen(r#"{"revision":null,"target":"authority.origin"}"#, "", 1)
        .replacen(
            r#"{"target":"authority.origin","value":"https://gitlab.example"}"#,
            "",
            1,
        )
        .replacen(
            &format!(r#",{{"revision":"{ORIGIN_REVISION}","target":"authority.origin"}}"#),
            "",
            1,
        );
    assert!(ConnectBegin::parse_and_validate(omitted_origin.as_bytes(), &universe()).is_err());

    let mut optional = universe();
    optional[1].required = false;
    ConnectBegin::parse_and_validate(omitted_origin.as_bytes(), &optional)
        .expect("an unselected optional target is omitted from every projection");

    let missing_setting = CONNECT.replacen(
        r#"{"target":"authority.origin","value":"https://gitlab.example"}"#,
        "",
        1,
    );
    assert!(ConnectBegin::parse_and_validate(missing_setting.as_bytes(), &universe()).is_err());

    let duplicate = CONNECT.replacen(
        &format!(
            r#"{{"revision":"{TOKEN_REVISION}","target":"credential.token"}}"#
        ),
        &format!(
            r#"{{"revision":"{TOKEN_REVISION}","target":"credential.token"}},{{"revision":"{TOKEN_REVISION}","target":"credential.token"}}"#
        ),
        1,
    );
    assert!(ConnectBegin::parse_canonical(duplicate.as_bytes()).is_err());

    let name = format!(r#"{{"revision":"{NAME_REVISION}","target":"connection.name"}}"#);
    let origin = format!(r#"{{"revision":"{ORIGIN_REVISION}","target":"authority.origin"}}"#);
    let reordered = CONNECT.replacen(&format!("{name},{origin}"), &format!("{origin},{name}"), 1);
    let reordered = ConnectBegin::parse_canonical(reordered.as_bytes())
        .expect("array order remains canonical JSON");
    assert!(reordered.validate_target_closure(&universe()).is_err());

    let revised = ConnectBegin::parse_canonical(
        CONNECT
            .replacen(ORIGIN_REVISION, &"6".repeat(64), 1)
            .as_bytes(),
    )
    .expect("well-formed changed revision");
    assert!(revised.validate_target_closure(&universe()).is_err());

    let mut wrong_partition = universe();
    wrong_partition[1].partition = TargetPartition::Setting;
    let parsed = ConnectBegin::parse_canonical(CONNECT.as_bytes()).expect("canonical connect");
    assert!(parsed.validate_target_closure(&wrong_partition).is_err());

    let nonnull_authority = ConnectBegin::parse_canonical(
        CONNECT
            .replacen(r#""revision":null"#, r#""revision":"1""#, 1)
            .as_bytes(),
    )
    .expect("canonical decimal authority revision");
    assert!(nonnull_authority
        .validate_target_closure(&universe())
        .is_err());
}

#[test]
fn credential_target_closure_is_the_complete_nonempty_partition_in_plan_order() {
    let with_cross_partition = CREDENTIAL.replacen(
        &format!(
            r#"{{"revision":"{TOKEN_REVISION}","target":"credential.token"}}"#
        ),
        &format!(
            r#"{{"revision":"{ORIGIN_REVISION}","target":"authority.origin"}},{{"revision":"{TOKEN_REVISION}","target":"credential.token"}}"#
        ),
        1,
    );
    let parsed = CredentialBegin::parse_canonical(with_cross_partition.as_bytes())
        .expect("well-formed cross-partition control object");
    assert!(parsed.validate_target_closure(&universe()).is_err());

    let mut no_credentials = universe();
    no_credentials[2].partition = TargetPartition::Setting;
    let parsed = CredentialBegin::parse_canonical(CREDENTIAL.as_bytes()).expect("credential begin");
    assert!(parsed.validate_target_closure(&no_credentials).is_err());

    let empty = CREDENTIAL.replacen(
        &format!(r#"{{"revision":"{TOKEN_REVISION}","target":"credential.token"}}"#),
        "",
        1,
    );
    assert!(CredentialBegin::parse_canonical(empty.as_bytes()).is_err());
}
