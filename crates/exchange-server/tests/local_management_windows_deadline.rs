#![cfg(windows)]

use std::process::Command;

/// Native MSVC owns the production named-pipe execution evidence for X-135.
#[test]
fn supervised_windows_local_management_deadlines_are_phase_exact() {
    let output = Command::new(env!("CARGO_BIN_EXE_flux-exchange"))
        .arg("native-deadline-test-seam")
        .output()
        .expect("run the production Windows deadline fixture");
    assert!(
        output.status.success(),
        "Windows deadline fixture refused without value-bearing output: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
