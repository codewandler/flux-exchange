//! The security posture has one discoverable source of truth (X-89).
//!
//! Security reasoning already exists throughout the repository. This test prevents the index and
//! the four designs that define the important boundaries from drifting away from the document that
//! gathers them. It deliberately checks links rather than copied claims: implementation details
//! remain owned by their source and design records.

const SECURITY: &str = include_str!("../../../docs/security.md");
const ROOT_README: &str = include_str!("../../../README.md");
const DOCS_README: &str = include_str!("../../../docs/README.md");

const AUTHORITATIVE_DESIGNS: &[&str] = &[
    "designs/identity-and-session.md",
    "designs/oidc-signin.md",
    "designs/invoke.md",
    "designs/public-service-hardening.md",
    "designs/remote-deployment.md",
];

#[test]
fn the_security_posture_is_discoverable_and_points_to_its_authorities() {
    assert!(
        ROOT_README.contains("docs/security.md"),
        "README.md must link to the contributor security posture",
    );
    assert!(
        DOCS_README.contains("[security.md](security.md)"),
        "the contributor docs index must link to the security posture",
    );

    for design in AUTHORITATIVE_DESIGNS {
        assert!(
            SECURITY.contains(design),
            "docs/security.md must point to the authoritative `{design}` design",
        );
    }

    for label in [
        "**Enforced in code.**",
        "**Deployment-dependent.**",
        "**Known limitation.**",
    ] {
        assert!(
            SECURITY.contains(label),
            "docs/security.md must use the posture label `{label}`",
        );
    }
}
