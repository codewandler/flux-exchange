#![cfg(unix)]

use std::process::Command;

/// The dedicated target makes the exact Unix selector report zero filtered tests.
#[test]
fn authenticated_native_idle_and_partial_traffic_expire_on_one_absolute_clock() {
    let output = Command::new(env!("CARGO_BIN_EXE_flux-exchange"))
        .arg("unix-deadline-test-seam")
        .output()
        .expect("run the production Unix deadline fixture");
    assert!(
        output.status.success(),
        "Unix deadline fixture refused without value-bearing output: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
