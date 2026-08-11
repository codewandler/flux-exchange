//! A deployment's policy over declared weaknesses in credential acquisition.
//!
//! No released connector declares an authentication hazard yet: upstream C-440 owns that
//! declaration. The fixture here therefore stands in for a connector acquisition deliberately.
//! Keeping the declaration in the fixture is load-bearing — a test that asks the posture about no
//! hazard would pass for the wrong reason and claim protection over a path it never exercised.

use exchange_host::{AuthHazard, AuthPosture, AuthPostureRefusal};

struct AcquisitionFixture {
    connector: &'static str,
    hazard: AuthHazard,
}

fn resource_owner_secret_acquisition() -> AcquisitionFixture {
    AcquisitionFixture {
        connector: "fixture-password-grant",
        hazard: AuthHazard::ResourceOwnerSecretShared,
    }
}

#[test]
fn a_declared_hazard_is_refused_unset_and_admitted_only_when_armed() {
    let acquisition = resource_owner_secret_acquisition();

    let refusal = AuthPosture::fail_closed()
        .admit(acquisition.connector, acquisition.hazard)
        .expect_err("unset means every declared authentication hazard is refused");
    assert!(matches!(
        refusal,
        AuthPostureRefusal::HazardNotAllowed {
            connector,
            hazard: AuthHazard::ResourceOwnerSecretShared,
        } if connector == acquisition.connector
    ));

    AuthPosture::allowing([AuthHazard::ResourceOwnerSecretShared])
        .admit(acquisition.connector, acquisition.hazard)
        .expect("the one explicitly armed hazard is admitted");
}

#[test]
fn the_refusal_names_the_connector_and_hazard_but_has_no_value_field() {
    let acquisition = resource_owner_secret_acquisition();
    let refusal = AuthPosture::fail_closed()
        .admit(acquisition.connector, acquisition.hazard)
        .expect_err("the fixture declares a refused hazard");
    let message = refusal.to_string();

    assert!(message.contains(acquisition.connector), "{message}");
    assert!(
        message.contains("resource_owner_secret_shared"),
        "{message}"
    );
}

/// One arm per policy refusal, following the server's
/// `every_refusal_states_the_status_it_answers_with`. A vendor refusal is deliberately not on this
/// type: it happens after policy admission and receives its own status in X-75.
#[test]
fn every_posture_refusal_states_the_status_it_answers_with() {
    let refusal = AuthPostureRefusal::HazardNotAllowed {
        connector: "fixture".to_owned(),
        hazard: AuthHazard::ResourceOwnerSecretShared,
    };

    assert_eq!(refusal.status(), 403);
    assert_ne!(
        refusal.status(),
        401,
        "a deployment-policy refusal must not read as vendor credentials being rejected"
    );
}
