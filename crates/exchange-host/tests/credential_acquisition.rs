use exchange_host::{
    async_trait, AcquiredCredential, AcquisitionRefusal, AuthorizationCodeRedemption,
    CredentialAcquirer, PasswordRedemption, RefreshRedemption, Secret,
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

/// **X-147.** The delegated leg is a *default* method, and a performer that does not perform it
/// refuses by name rather than failing to compile.
///
/// [`RefusingAcquirer`] above implements only the two legs X-75 declared, exactly as an existing
/// downstream performer does. That it still satisfies [`CredentialAcquirer`] is the assertion: a
/// required method here would be a breaking change to a published crate, and the refusal is what a
/// composition binding an old performer to a connector declaring `authorization_code` gets — before
/// any vendor request, and naming no value.
#[tokio::test]
async fn an_existing_performer_refuses_the_delegated_leg_without_being_rewritten() {
    let code = Secret::from("authorization-code-never-print-this".to_owned());
    let verifier = Secret::from("code-verifier-never-print-this".to_owned());

    let refusal = RefusingAcquirer
        .redeem_authorization_code(AuthorizationCodeRedemption::new(&code, &verifier))
        .await
        .expect_err("a performer that does not perform the grant refuses it");

    assert_eq!(refusal, AcquisitionRefusal::GrantNotPerformed);
    assert_eq!(refusal.code(), "grant_not_performed");
    for rendered in [
        format!("{refusal}"),
        format!("{refusal:?}"),
        format!("{:?}", AuthorizationCodeRedemption::new(&code, &verifier)),
    ] {
        assert!(!rendered.contains("authorization-code-never-print-this"));
        assert!(!rendered.contains("code-verifier-never-print-this"));
    }

    // The same rule X-75 wrote for the two legs it declared: the port carries no endpoint, no
    // redirect and no browser vocabulary. A delegated grant needs all three, and every one of them
    // is deployment configuration the composing binary owns.
    let source = include_str!("../src/acquisition.rs");
    assert!(!source.contains("redirect_uri"));
    assert!(!source.contains("authorization_endpoint"));
    assert!(!source.contains("code_challenge"));
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
