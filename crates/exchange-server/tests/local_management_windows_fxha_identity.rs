#![cfg(windows)]

use std::process::Command;

/// Native MSVC executes the production pinned-client identity predicate without a fake peer seam.
#[test]
fn pinned_fxha_client_refuses_each_pid_creation_sid_session_and_liveness_substitution() {
    let output = Command::new(env!("CARGO_BIN_EXE_flux-exchange"))
        .arg("native-fxha-identity-test-seam")
        .output()
        .expect("run the production FXHA identity fixture");
    assert!(
        output.status.success(),
        "Windows FXHA identity fixture refused value-free: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
