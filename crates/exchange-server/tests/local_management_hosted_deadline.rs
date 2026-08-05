use std::process::Command;

/// The dedicated target makes the exact hosted selector report zero filtered tests.
#[test]
fn hosted_slot_idle_and_ping_traffic_expire_on_the_admission_clock() {
    let output = Command::new(env!("CARGO_BIN_EXE_flux-exchange"))
        .arg("hosted-deadline-test-seam")
        .output()
        .expect("run the production hosted deadline fixture");
    assert!(
        output.status.success(),
        "hosted deadline fixture refused without value-bearing output: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
