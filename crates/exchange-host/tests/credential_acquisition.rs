use exchange_host::{
    async_trait, AcquiredCredential, AcquisitionRefusal, CredentialAcquirer, PasswordRedemption,
    RefreshRedemption, Secret,
};

struct RefusingAcquirer;

#[async_trait]
impl CredentialAcquirer for RefusingAcquirer {
    async fn redeem_password(
        &self,
        _redemption: PasswordRedemption<'_>,
    ) -> Result<AcquiredCredential, AcquisitionRefusal> {
        Err(AcquisitionRefusal::MfaRequired)
    }

    async fn redeem_refresh(
        &self,
        _redemption: RefreshRedemption<'_>,
    ) -> Result<AcquiredCredential, AcquisitionRefusal> {
        Err(AcquisitionRefusal::CredentialsRejected)
    }
}

#[tokio::test]
async fn the_port_accepts_secrets_without_exposing_transport_or_lifetime_requests() {
    let username = Secret::from("alice@example.test".to_owned());
    let password = Secret::from("never-print-this".to_owned());
    let refresh = Secret::from("also-never-print-this".to_owned());

    let password_refusal = RefusingAcquirer
        .redeem_password(PasswordRedemption::new(&username, &password))
        .await
        .expect_err("fixture refuses password redemption");
    let refresh_refusal = RefusingAcquirer
        .redeem_refresh(RefreshRedemption::new(&refresh))
        .await
        .expect_err("fixture refuses refresh redemption");

    assert_eq!(password_refusal.code(), "mfa_required");
    assert_eq!(refresh_refusal.code(), "credentials_rejected");
    for rendered in [
        format!("{password_refusal}"),
        format!("{password_refusal:?}"),
        format!("{refresh_refusal}"),
        format!("{refresh_refusal:?}"),
        format!("{:?}", PasswordRedemption::new(&username, &password)),
        format!("{:?}", RefreshRedemption::new(&refresh)),
    ] {
        assert!(!rendered.contains("never-print-this"));
        assert!(!rendered.contains("also-never-print-this"));
    }

    let source = include_str!("../src/acquisition.rs");
    assert!(!source.contains("requested_ttl"));
    assert!(!source.contains("account_id"));
    assert!(!source.contains("endpoint_url"));
}

#[test]
fn acquired_credentials_redact_their_values() {
    let acquired = AcquiredCredential::new(
        Secret::from("access-secret".to_owned()),
        Some(Secret::from("refresh-secret".to_owned())),
        Some(1_900_000_000),
    );

    let rendered = format!("{acquired:?}");
    assert!(!rendered.contains("access-secret"));
    assert!(!rendered.contains("refresh-secret"));
    assert!(rendered.contains("1900000000"));
}
